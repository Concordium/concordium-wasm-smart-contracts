mod constants;
#[cfg(feature = "enable-ffi")]
mod ffi;
mod types;

use anyhow::{anyhow, bail, ensure};
use constants::MAX_CONTRACT_STATE;
use contracts_common::*;
use machine::Value;
use std::{
    collections::{BTreeMap, LinkedList},
    convert::TryInto,
    io::Write,
};
pub use types::*;
use wasm_transform::{
    artifact::{Artifact, ArtifactNamedImport, RunnableCode, TryFromImport},
    machine,
    parse::{parse_custom, parse_skeleton},
    types::{ExportDescription, Module, Name},
    utils,
    validate::ValidateImportExport,
};

pub type ExecResult<A> = anyhow::Result<A>;

#[derive(Clone, Default)]
/// Structure to support logging of events from smart contracts.
pub struct Logs {
    pub logs: LinkedList<Vec<u8>>,
}

impl Logs {
    pub fn new() -> Self {
        Self {
            logs: LinkedList::new(),
        }
    }

    pub fn log_event(&mut self, event: Vec<u8>) { self.logs.push_back(event); }

    pub fn iterate(&self) -> impl Iterator<Item = &Vec<u8>> { self.logs.iter() }

    pub fn to_bytes(&self) -> Vec<u8> {
        let len = self.logs.len();
        let mut out = Vec::with_capacity(4 * len + 4);
        out.extend_from_slice(&(len as u32).to_be_bytes());
        for v in self.iterate() {
            out.extend_from_slice(&(v.len() as u32).to_be_bytes());
            out.extend_from_slice(v);
        }
        out
    }
}

#[derive(Clone, Copy)]
pub struct Energy {
    /// Energy left to use
    pub energy: u64,
}

/// Cost of allocation of one page of memory in relation to execution cost.
/// FIXME: It is unclear whether this is really necessary with the hard limit we
/// have on memory use.
/// If we keep it, the cost must be analyzed and put into perspective
pub const MEMORY_COST_FACTOR: u32 = 100;

impl Energy {
    pub fn tick_energy(&mut self, amount: u64) -> ExecResult<()> {
        if self.energy >= amount {
            self.energy -= amount;
            Ok(())
        } else {
            self.energy = 0;
            bail!("Out of energy.")
        }
    }

    /// TODO: This needs more specification. At the moment it is not used, but
    /// should be.
    pub fn charge_stack(&mut self, amount: u64) -> ExecResult<()> {
        if self.energy >= amount {
            self.energy -= amount;
            Ok(())
        } else {
            self.energy = 0;
            bail!("Out of energy.")
        }
    }

    /// TODO: This needs more specification. At the moment it is not used, but
    /// should be.
    pub fn charge_memory_alloc(&mut self, num_pages: u32) -> ExecResult<()> {
        let to_charge = u64::from(num_pages) * u64::from(MEMORY_COST_FACTOR); // this cannot overflow because of the cast.
        if self.energy >= to_charge {
            self.energy -= to_charge;
        } else {
            self.energy = 0;
            bail!("Out of energy.");
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
/// The Default instance of this type constructs and empty list of outcomes.
pub struct Outcome {
    pub cur_state: Vec<Action>,
}

impl Outcome {
    pub fn new() -> Outcome { Self::default() }

    pub fn accept(&mut self) -> u32 {
        let response = self.cur_state.len();
        self.cur_state.push(Action::Accept);
        response as u32
    }

    pub fn simple_transfer(&mut self, bytes: &[u8], amount: u64) -> ExecResult<u32> {
        let response = self.cur_state.len();
        let addr: [u8; 32] = bytes.try_into()?;
        let to_addr = AccountAddress(addr);
        self.cur_state.push(Action::SimpleTransfer {
            to_addr,
            amount,
        });
        Ok(response as u32)
    }

    pub fn send(
        &mut self,
        addr_index: u64,
        addr_subindex: u64,
        receive_name_bytes: &[u8],
        amount: u64,
        parameter_bytes: &[u8],
    ) -> ExecResult<u32> {
        let response = self.cur_state.len();

        let name_str = std::str::from_utf8(receive_name_bytes)?;
        ensure!(is_valid_receive_name(name_str), "Not a valid receive name.");
        let name = receive_name_bytes.to_vec();

        let parameter = parameter_bytes.to_vec();

        let to_addr = ContractAddress {
            index:    addr_index,
            subindex: addr_subindex,
        };
        self.cur_state.push(Action::Send {
            to_addr,
            name,
            amount,
            parameter,
        });
        Ok(response as u32)
    }

    pub fn combine_and(&mut self, l: u32, r: u32) -> ExecResult<u32> {
        let response = self.cur_state.len() as u32;
        ensure!(l < response && r < response, "Combining unknown actions.");
        self.cur_state.push(Action::And {
            l,
            r,
        });
        Ok(response)
    }

    pub fn combine_or(&mut self, l: u32, r: u32) -> ExecResult<u32> {
        let response = self.cur_state.len() as u32;
        ensure!(l < response && r < response, "Combining unknown actions.");
        self.cur_state.push(Action::Or {
            l,
            r,
        });
        Ok(response)
    }
}

/// Smart contract state.
#[derive(Clone)]
pub struct State {
    pub state: Vec<u8>,
}

impl State {
    pub fn is_empty(&self) -> bool { self.state.is_empty() }

    // FIXME: This should not be copying so much data around, but for POC it is
    // fine. We should probably do some sort of copy-on-write here in the near term,
    // and in the long term we need to keep track of which parts were written.
    pub fn new(st: Option<&[u8]>) -> Self {
        match st {
            None => Self {
                state: Vec::new(),
            },
            Some(bytes) => Self {
                state: Vec::from(bytes),
            },
        }
    }

    pub fn len(&self) -> u32 { self.state.len() as u32 }

    pub fn write_state(&mut self, offset: u32, bytes: &[u8]) -> ExecResult<u32> {
        let length = bytes.len();
        ensure!(offset <= self.len(), "Cannot write past the offset.");
        let offset = offset as usize;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| anyhow!("Writing past the end of memory."))? as usize;
        let end = std::cmp::min(end, MAX_CONTRACT_STATE as usize) as u32;
        if self.len() < end {
            self.state.resize(end as usize, 0u8);
        }
        let written = (&mut self.state[offset..end as usize]).write(bytes)?;
        Ok(written as u32)
    }

    pub fn load_state(&self, offset: u32, mut bytes: &mut [u8]) -> ExecResult<u32> {
        let offset = offset as usize;
        if offset >= self.state.len() {
            Ok(0)
        } else {
            // Write on slices overwrites the buffer and returns how many bytes were
            // written.
            let amt = bytes.write(&self.state[offset..])?;
            Ok(amt as u32)
        }
    }

    pub fn resize_state(&mut self, new_size: u32) -> u32 {
        if new_size > MAX_CONTRACT_STATE {
            0
        } else {
            self.state.resize(new_size as usize, 0u8);
            1
        }
    }
}

struct InitHost<'a> {
    /// Remaining energy for execution.
    energy: Energy,
    /// Logs produced during execution.
    logs: Logs,
    /// The contract's state.
    state: State,
    /// The parameter to the init method.
    param: &'a [u8],
    /// The init context for this invocation.
    init_ctx: &'a InitContext,
}

struct ReceiveHost<'a> {
    /// Remaining energy for execution.
    energy: Energy,
    /// Logs produced during execution.
    logs: Logs,
    /// The contract's state.
    state: State,
    /// The parameter to the init method.
    param: &'a [u8],
    /// Outcomes of the execution, i.e., the actions tree.
    outcomes: Outcome,
    /// The receive context for this call.
    receive_ctx: &'a ReceiveContext,
}

pub trait HasCommon {
    fn logs(&mut self) -> &mut Logs;
    fn state(&mut self) -> &mut State;
    fn param(&self) -> &[u8];
    fn metadata(&self) -> &ChainMetadata;
}

impl<'a> HasCommon for InitHost<'a> {
    fn logs(&mut self) -> &mut Logs { &mut self.logs }

    fn state(&mut self) -> &mut State { &mut self.state }

    fn param(&self) -> &[u8] { &self.param }

    fn metadata(&self) -> &ChainMetadata { &self.init_ctx.metadata }
}

impl<'a> HasCommon for ReceiveHost<'a> {
    fn logs(&mut self) -> &mut Logs { &mut self.logs }

    fn state(&mut self) -> &mut State { &mut self.state }

    fn param(&self) -> &[u8] { &self.param }

    fn metadata(&self) -> &ChainMetadata { &self.receive_ctx.metadata }
}

fn call_common<C: HasCommon>(
    host: &mut C,
    f: CommonFunc,
    memory: &mut Vec<u8>,
    stack: &mut machine::RuntimeStack,
) -> machine::RunResult<()> {
    match f {
        CommonFunc::GetParameterSize => {
            stack.push_value(host.param().len() as u32);
        }
        CommonFunc::GetParameterSection => {
            let offset = unsafe { stack.pop_u32() } as usize;
            let length = unsafe { stack.pop_u32() } as usize;
            let start = unsafe { stack.pop_u32() } as usize;
            let write_end = start + length; // this cannot overflow on 64-bit machines.
            ensure!(write_end <= memory.len(), "Illegal memory access.");
            let end = std::cmp::min(offset + length, host.param().len());
            ensure!(offset <= end, "Attempting to read non-existent parameter.");
            let amt = (&mut memory[start..write_end]).write(&host.param()[offset..end])?;
            stack.push_value(amt as u32);
        }
        CommonFunc::LogEvent => {
            let length = unsafe { stack.pop_u32() } as usize;
            let start = unsafe { stack.pop_u32() } as usize;
            let end = start + length;
            ensure!(end <= memory.len(), "Illegal memory access.");
            host.logs().log_event(memory[start..end].to_vec());
        }
        CommonFunc::LoadState => {
            let offset = unsafe { stack.pop_u32() };
            let length = unsafe { stack.pop_u32() } as usize;
            let start = unsafe { stack.pop_u32() } as usize;
            let end = start + length; // this cannot overflow on 64-bit machines.
            ensure!(end <= memory.len(), "Illegal memory access.");
            let res = host.state().load_state(offset, &mut memory[start..end])?;
            stack.push_value(res);
        }
        CommonFunc::WriteState => {
            let offset = unsafe { stack.pop_u32() };
            let length = unsafe { stack.pop_u32() } as usize;
            let start = unsafe { stack.pop_u32() } as usize;
            let end = start + length; // this cannot overflow on 64-bit machines.
            ensure!(end <= memory.len(), "Illegal memory access.");
            let res = host.state().write_state(offset, &memory[start..end])?;
            stack.push_value(res);
        }
        CommonFunc::ResizeState => {
            let new_size = stack.pop();
            let new_size = unsafe { new_size.short } as u32;
            stack.push_value(host.state().resize_state(new_size));
        }
        CommonFunc::StateSize => {
            stack.push_value(host.state().len());
        }
        CommonFunc::GetSlotNumber => {
            stack.push_value(host.metadata().slot_number);
        }
        CommonFunc::GetSlotTime => {
            stack.push_value(host.metadata().slot_time);
        }
        CommonFunc::GetBlockHeight => {
            stack.push_value(host.metadata().block_height);
        }
        CommonFunc::GetFinalizedHeight => {
            stack.push_value(host.metadata().finalized_height);
        }
    }
    Ok(())
}

impl<'a> machine::Host<ProcessedImports> for InitHost<'a> {
    #[inline(always)]
    fn tick_energy(&mut self, x: u64) -> machine::RunResult<()> {
        if self.energy.energy >= x {
            self.energy.energy -= x;
            Ok(())
        } else {
            self.energy.energy = 0;
            bail!("Out of energy.")
        }
    }

    #[inline]
    fn call(
        &mut self,
        f: &ProcessedImports,
        memory: &mut Vec<u8>,
        stack: &mut machine::RuntimeStack,
    ) -> machine::RunResult<()> {
        match f.tag {
            ImportFunc::ChargeEnergy => {
                self.energy.tick_energy(unsafe { stack.pop_u64() })?;
            }
            ImportFunc::ChargeStackSize => {
                self.energy.charge_stack(unsafe { stack.pop_u64() })?;
            }
            ImportFunc::ChargeMemoryAlloc => {
                self.energy.charge_memory_alloc(unsafe { stack.peek_u32() })?;
            }
            ImportFunc::Common(cf) => call_common(self, cf, memory, stack)?,
            ImportFunc::InitOnly(InitOnlyFunc::GetInitOrigin) => {
                let start = unsafe { stack.pop_u32() } as usize;
                ensure!(start <= memory.len(), "Illegal memory access for init origin.");
                (&mut memory[start..start + 32]).write_all(self.init_ctx.init_origin.as_ref())?;
            }
            ImportFunc::ReceiveOnly(_) => {
                bail!("Not implemented for init {:#?}.", f);
            }
        }
        Ok(())
    }
}

impl<'a> ReceiveHost<'a> {
    pub fn call_receive_only(
        &mut self,
        rof: ReceiveOnlyFunc,
        memory: &mut Vec<u8>,
        stack: &mut machine::RuntimeStack,
    ) -> ExecResult<()> {
        match rof {
            ReceiveOnlyFunc::Accept => {
                stack.push_value(self.outcomes.accept());
            }
            ReceiveOnlyFunc::SimpleTransfer => {
                let amount = unsafe { stack.pop_u64() };
                let addr_start = unsafe { stack.pop_u32() } as usize;
                // Overflow is not possible in the next line on 64-bit machines.
                ensure!(addr_start + 32 <= memory.len(), "Illegal memory access.");
                stack.push_value(
                    self.outcomes.simple_transfer(&memory[addr_start..addr_start + 32], amount)?,
                )
            }
            ReceiveOnlyFunc::Send => {
                let parameter_len = unsafe { stack.pop_u32() } as usize;
                let parameter_start = unsafe { stack.pop_u32() } as usize;
                // Overflow is not possible in the next line on 64-bit machines.
                let parameter_end = parameter_start + parameter_len;
                let amount = unsafe { stack.pop_u64() };
                let receive_name_len = unsafe { stack.pop_u32() } as usize;
                let receive_name_start = unsafe { stack.pop_u32() } as usize;
                // Overflow is not possible in the next line on 64-bit machines.
                let receive_name_end = receive_name_start + receive_name_len;
                let addr_subindex = unsafe { stack.pop_u64() };
                let addr_index = unsafe { stack.pop_u64() };
                ensure!(parameter_end <= memory.len(), "Illegal memory access.");
                ensure!(receive_name_end <= memory.len(), "Illegal memory access.");
                let res = self.outcomes.send(
                    addr_index,
                    addr_subindex,
                    &memory[receive_name_start..receive_name_end],
                    amount,
                    &memory[parameter_start..parameter_end],
                )?;
                stack.push_value(res);
            }
            ReceiveOnlyFunc::CombineAnd => {
                let right = unsafe { stack.pop_u32() };
                let left = unsafe { stack.pop_u32() };
                let res = self.outcomes.combine_and(left, right)?;
                stack.push_value(res);
            }
            ReceiveOnlyFunc::CombineOr => {
                let right = unsafe { stack.pop_u32() };
                let left = unsafe { stack.pop_u32() };
                let res = self.outcomes.combine_or(left, right)?;
                stack.push_value(res);
            }
            ReceiveOnlyFunc::GetReceiveInvoker => {
                let start = unsafe { stack.pop_u32() } as usize;
                ensure!(start <= memory.len(), "Illegal memory access for receive owner.");
                (&mut memory[start..start + 32]).write_all(self.receive_ctx.invoker.as_ref())?;
            }
            ReceiveOnlyFunc::GetReceiveSelfAddress => {
                let start = unsafe { stack.pop_u32() } as usize;
                ensure!(start + 16 <= memory.len(), "Illegal memory access for receive owner.");
                (&mut memory[start..start + 8])
                    .write_all(&self.receive_ctx.self_address.index.to_le_bytes())?;
                (&mut memory[start + 8..start + 16])
                    .write_all(&self.receive_ctx.self_address.subindex.to_le_bytes())?;
            }
            ReceiveOnlyFunc::GetReceiveSelfBalance => {
                stack.push_value(self.receive_ctx.self_balance.micro_gtu);
            }
            ReceiveOnlyFunc::GetReceiveSender => {
                let start = unsafe { stack.pop_u32() } as usize;
                ensure!(start <= memory.len(), "Illegal memory access for receive owner.");
                let bytes = to_bytes(self.receive_ctx.sender());
                (&mut memory[start..]).write_all(&bytes)?;
            }
            ReceiveOnlyFunc::GetReceiveOwner => {
                let start = unsafe { stack.pop_u32() } as usize;
                ensure!(start <= memory.len(), "Illegal memory access for receive owner.");
                (&mut memory[start..start + 32]).write_all(self.receive_ctx.owner.as_ref())?;
            }
        }
        Ok(())
    }
}

impl<'a> machine::Host<ProcessedImports> for ReceiveHost<'a> {
    #[inline(always)]
    fn tick_energy(&mut self, x: u64) -> machine::RunResult<()> {
        if self.energy.energy >= x {
            self.energy.energy -= x;
            Ok(())
        } else {
            self.energy.energy = 0;
            bail!("Out of energy.")
        }
    }

    #[inline]
    fn call(
        &mut self,
        f: &ProcessedImports,
        memory: &mut Vec<u8>,
        stack: &mut machine::RuntimeStack,
    ) -> machine::RunResult<()> {
        match f.tag {
            ImportFunc::ChargeEnergy => self.energy.tick_energy(unsafe { stack.pop_u64() })?,
            ImportFunc::ChargeStackSize => self.energy.charge_stack(unsafe { stack.pop_u64() })?,
            ImportFunc::ChargeMemoryAlloc => {
                self.energy.charge_memory_alloc(unsafe { stack.peek_u32() })?
            }
            ImportFunc::Common(cf) => call_common(self, cf, memory, stack)?,
            ImportFunc::ReceiveOnly(cro) => self.call_receive_only(cro, memory, stack)?,
            ImportFunc::InitOnly(InitOnlyFunc::GetInitOrigin) => {
                bail!("Not implemented for receive.");
            }
        }
        Ok(())
    }
}

pub type Parameter = Vec<u8>;

pub fn invoke_init<C: RunnableCode>(
    artifact: Artifact<ProcessedImports, C>,
    amount: u64,
    init_ctx: InitContext,
    init_name: &str,
    parameter: Parameter,
    energy: u64,
) -> ExecResult<InitResult> {
    let mut host = InitHost {
        energy:   Energy {
            energy,
        },
        logs:     Logs::new(),
        state:    State::new(None),
        param:    &parameter,
        init_ctx: &init_ctx,
    };

    let (res, _) = artifact.run(&mut host, init_name, &[Value::I64(amount as i64)])?;
    let remaining_energy = host.energy.energy;
    if let Some(Value::I32(0)) = res {
        Ok(InitResult::Success {
            logs: host.logs,
            state: host.state,
            remaining_energy,
        })
    } else {
        Ok(InitResult::Reject {
            remaining_energy,
        })
    }
}

#[inline]
pub fn invoke_init_from_artifact(
    artifact_bytes: &[u8],
    amount: u64,
    init_ctx: InitContext,
    init_name: &str,
    parameter: Parameter,
    energy: u64,
) -> ExecResult<InitResult> {
    let artifact = utils::parse_artifact(artifact_bytes)?;
    invoke_init(artifact, amount, init_ctx, init_name, parameter, energy)
}

#[inline]
pub fn invoke_init_from_source(
    source_bytes: &[u8],
    amount: u64,
    init_ctx: InitContext,
    init_name: &str,
    parameter: Parameter,
    energy: u64,
) -> ExecResult<InitResult> {
    let artifact = utils::instantiate(&ConcordiumAllowedImports, source_bytes)?;
    invoke_init(artifact, amount, init_ctx, init_name, parameter, energy)
}

pub fn invoke_receive<C: RunnableCode>(
    artifact: Artifact<ProcessedImports, C>,
    amount: u64,
    receive_ctx: ReceiveContext,
    current_state: &[u8],
    receive_name: &str,
    parameter: Parameter,
    energy: u64,
) -> ExecResult<ReceiveResult> {
    let mut host = ReceiveHost {
        energy:      Energy {
            energy,
        },
        logs:        Logs::new(),
        state:       State::new(Some(current_state)),
        param:       &parameter,
        receive_ctx: &receive_ctx,
        outcomes:    Outcome::new(),
    };

    let (res, _) = artifact.run(&mut host, receive_name, &[Value::I64(amount as i64)])?;
    let remaining_energy = host.energy.energy;
    if let Some(Value::I32(n)) = res {
        // FIXME: We should filter out to only return the ones reachable from
        // the root.
        let mut actions = host.outcomes.cur_state;
        if n >= 0 && (n as usize) < actions.len() {
            let n = n as usize;
            actions.truncate(n + 1);
            Ok(ReceiveResult::Success {
                logs: host.logs,
                state: host.state,
                actions,
                remaining_energy,
            })
        } else if n >= 0 {
            bail!("Invalid return.")
        } else {
            // TODO: Here we map all negative values to reject.
            // This is a fine choice, but perhaps we should record
            // the exact failure as well, so it can be inspected.
            Ok(ReceiveResult::Reject {
                remaining_energy,
            })
        }
    } else {
        bail!(
            "Invalid return. Expected a value, but receive nothing. This should not happen for \
             well-formed modules"
        );
    }
}

#[inline]
pub fn invoke_receive_from_artifact(
    artifact_bytes: &[u8],
    amount: u64,
    receive_ctx: ReceiveContext,
    current_state: &[u8],
    receive_name: &str,
    parameter: Parameter,
    energy: u64,
) -> ExecResult<ReceiveResult> {
    let artifact = utils::parse_artifact(artifact_bytes)?;
    invoke_receive(artifact, amount, receive_ctx, current_state, receive_name, parameter, energy)
}

#[inline]
pub fn invoke_receive_from_source(
    source_bytes: &[u8],
    amount: u64,
    receive_ctx: ReceiveContext,
    current_state: &[u8],
    receive_name: &str,
    parameter: Parameter,
    energy: u64,
) -> ExecResult<ReceiveResult> {
    let artifact = utils::instantiate(&ConcordiumAllowedImports, source_bytes)?;
    invoke_receive(artifact, amount, receive_ctx, current_state, receive_name, parameter, energy)
}

/// A host which traps for any function call.
pub struct TrapHost;

impl<I> machine::Host<I> for TrapHost {
    fn tick_energy(&mut self, _x: u64) -> machine::RunResult<()> {
        bail!("TrapHost tick_energy not implemented.")
    }

    fn call(
        &mut self,
        _f: &I,
        _memory: &mut Vec<u8>,
        _stack: &mut machine::RuntimeStack,
    ) -> machine::RunResult<()> {
        bail!("TrapHost traps on all host calls.")
    }
}

/// A host which traps for any function call apart from `report_error` which it
/// prints to standard out.
pub struct TestHost;

impl ValidateImportExport for TestHost {
    /// Simply ensure that there are no duplicates.
    #[inline(always)]
    fn validate_import_function(
        &self,
        duplicate: bool,
        _mod_name: &Name,
        _item_name: &Name,
        _ty: &wasm_transform::types::FunctionType,
    ) -> bool {
        !duplicate
    }

    #[inline(always)]
    fn validate_export_function(
        &self,
        _item_name: &Name,
        _ty: &wasm_transform::types::FunctionType,
    ) -> bool {
        true
    }
}

impl machine::Host<ArtifactNamedImport> for TestHost {
    fn tick_energy(&mut self, _x: u64) -> machine::RunResult<()> {
        bail!("TrapHost tick_energy not implemented.")
    }

    fn call(
        &mut self,
        f: &ArtifactNamedImport,
        memory: &mut Vec<u8>,
        stack: &mut machine::RuntimeStack,
    ) -> machine::RunResult<()> {
        if f.matches("concordium", "report_error") {
            let column = unsafe { stack.pop_u32() };
            let line = unsafe { stack.pop_u32() };
            let filename_length = unsafe { stack.pop_u32() } as usize;
            let filename_start = unsafe { stack.pop_u32() } as usize;
            let msg_length = unsafe { stack.pop_u32() } as usize;
            let msg_start = unsafe { stack.pop_u32() } as usize;
            ensure!(filename_start + filename_length <= memory.len(), "Illegal memory access.");
            ensure!(msg_start + msg_length <= memory.len(), "Illegal memory access.");
            let msg = std::str::from_utf8(&memory[msg_start..msg_start + msg_length])?;
            let filename =
                std::str::from_utf8(&memory[filename_start..filename_start + filename_length])?;
            let location = format!("{}:{}:{}", filename, line, column);
            eprintln!("\nError: {}\n{}\n", msg, location);
        } else {
            bail!("Unsupported host function call.")
        }
        Ok(())
    }
}

/// Instantiates the module with an external function to report back errors.
/// Then tries to run an exported 'main' function.
/// The main function is present in the module if compile using 'cargo test'
pub fn test_run(module_bytes: &[u8]) -> ExecResult<()> {
    eprintln!("\nInstantiating WASM module.");
    let artifact = utils::instantiate::<ArtifactNamedImport, _>(&TestHost, module_bytes)?;
    eprintln!("Running tests");
    // Unable to find a proper source, but it seems that the test main function
    // takes two u32 arguments, which we assume are `argc` and `argv` in a C
    // program. Since we don't use `argc` and `argv` in the test, we can pass
    // any u32.
    if let (Some(Value::I32(n)), _) =
        artifact.run(&mut TestHost, "main", &[Value::I32(0), Value::I32(0)])?
    {
        ensure!(n == 0, "Test failed.");
        eprintln!("Test result: ok.")
    } else {
        eprintln!("Test failed.");
    }
    Ok(())
}

/// Tries to generate a state schema and schemas for parameters of methods.
pub fn generate_contract_schema(module_bytes: &[u8]) -> ExecResult<schema::Contract> {
    let artifact = utils::instantiate::<ArtifactNamedImport, _>(&TestHost, module_bytes)?;
    let state = generate_schema_run(&artifact, "concordium_schema_state").ok();

    let mut method_parameter = BTreeMap::new();
    for name in artifact.export.keys() {
        if let Some(rest) = name.as_ref().strip_prefix("concordium_schema_function_") {
            let schema_type = generate_schema_run(&artifact, name.as_ref())?;
            method_parameter.insert(rest.to_string(), schema_type);
        }
    }

    Ok(schema::Contract {
        state,
        method_parameter,
    })
}

/// Runs the given schema function and reads the resulting schema from memory,
/// attempting to parse it as a type. If this fails, an error is returned.
fn generate_schema_run<I: TryFromImport, C: RunnableCode>(
    artifact: &Artifact<I, C>,
    schema_fn_name: &str,
) -> ExecResult<schema::Type> {
    let (ptr, memory) = if let (Some(Value::I32(ptr)), memory) =
        artifact.run(&mut TrapHost, schema_fn_name, &[])?
    {
        (ptr as u32 as usize, memory)
    } else {
        bail!("Schema derivation function malformed.")
    };

    // First we read an u32 which is the length of the serialized schema
    ensure!(ptr + 4 <= memory.len(), "Illegal memory access.");
    let len = u32::deserial(&mut Cursor::new(&memory[ptr..ptr + 4]))
        .map_err(|_| anyhow!("Cannot read schema length."))?;

    // Read the schema with offset of the u32
    ensure!(ptr + 4 + len as usize <= memory.len(), "Illegal memory access when reading schema.");
    let schema_bytes = &memory[ptr + 4..ptr + 4 + len as usize];
    schema::Type::deserial(&mut Cursor::new(schema_bytes))
        .map_err(|_| anyhow!("Failed deserialising the schema."))
}

/// Get the init methods of the module.
pub fn get_inits(module: &Module) -> Vec<&Name> {
    let mut out = Vec::new();
    for export in module.export.exports.iter() {
        if export.name.as_ref().starts_with("init_") && !export.name.as_ref().contains('.') {
            if let ExportDescription::Func {
                ..
            } = export.description
            {
                out.push(&export.name);
            }
        }
    }
    out
}

/// Get the receive methods of the module.
pub fn get_receives(module: &Module) -> Vec<&Name> {
    let mut out = Vec::new();
    for export in module.export.exports.iter() {
        if export.name.as_ref().contains('.') {
            if let ExportDescription::Func {
                ..
            } = export.description
            {
                out.push(&export.name);
            }
        }
    }
    out
}

/// Get the embedded schema if it exists
pub fn get_embedded_schema(bytes: &[u8]) -> ExecResult<schema::Contract> {
    let skeleton = parse_skeleton(bytes)?;
    let mut schema_sections = Vec::new();
    for ucs in skeleton.custom.iter() {
        let cs = parse_custom(ucs)?;
        if cs.name.as_ref() == "contract-schema" {
            schema_sections.push(cs)
        }
    }
    let section =
        schema_sections.first().ok_or_else(|| anyhow!("No schema found in the module"))?;
    let source = &mut Cursor::new(section.contents);
    schema::Contract::deserial(source).map_err(|_| anyhow!("Failed parsing schema"))
}
