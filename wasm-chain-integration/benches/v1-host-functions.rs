//! Benchmarks to help establish costs for V1 host functions. The benchmarks
//! are written with the intent that they measure representative or worst-case
//! uses of functions. Execution time, as well as energy throughput are
//! measured. These are then used as input to assigning costs to relevant
//! operations. Note that often there are other concerns than just execution
//! time when assigning costs, so benchmarks here should generally only ensure
//! that a sufficiently low upper bound is there.
use concordium_contracts_common::{
    Address, Amount, ChainMetadata, ContractAddress, OwnedEntrypointName, Timestamp,
};
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use std::time::Duration;
use wasm_chain_integration::{
    constants::MAX_ACTIVATION_FRAMES,
    v0,
    v1::{
        trie::{
            self, low_level::MutableTrie, EmptyCollector, Loader, MutableState, PersistentState,
        },
        ConcordiumAllowedImports, InstanceState, ProcessedImports, ReceiveContext, ReceiveHost,
        StateLessReceiveHost,
    },
    InterpreterEnergy,
};
use wasm_transform::{machine, parse, validate};

static CONTRACT_BYTES_HOST_FUNCTIONS: &[u8] = include_bytes!("./code/v1/host-functions.wasm");

fn mk_state<A: AsRef<[u8]>, B: Copy>(inputs: &[(A, B)]) -> (MutableState, Loader<Vec<u8>>)
where
    Vec<u8>: From<B>, {
    let mut node = MutableTrie::empty();
    let mut loader = Loader {
        inner: Vec::new(),
    };
    for (k, v) in inputs {
        node.insert(&mut loader, k.as_ref(), trie::Value::from(*v))
            .expect("No locks, so cannot fail.");
    }
    if let Some(trie) = node.freeze(&mut loader, &mut EmptyCollector) {
        (PersistentState::from(trie).thaw(), loader)
    } else {
        (PersistentState::Empty.thaw(), loader)
    }
}

/// Benchmarks for host functions.
/// The preconditions (expected state and param) for each function are specified
/// in host-functions.wat
pub fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("v1 host functions");

    let nrg = 1000;

    let start_energy = InterpreterEnergy {
        energy: nrg * 1000,
    };

    // the throughput is meant to correspond to 1NRG. The reported throughput should
    // be around 1M elements per second.
    group
        .measurement_time(Duration::from_secs(10))
        .throughput(criterion::Throughput::Elements(nrg));

    let skeleton = parse::parse_skeleton(black_box(CONTRACT_BYTES_HOST_FUNCTIONS)).unwrap();
    let module = {
        let mut module = validate::validate_module(&ConcordiumAllowedImports, &skeleton).unwrap();
        module.inject_metering().expect("Metering injection should succeed.");
        module
    };

    let artifact = module.compile::<ProcessedImports>().unwrap();

    let owner = concordium_contracts_common::AccountAddress([0u8; 32]);

    let receive_ctx: ReceiveContext<&[u8]> = ReceiveContext {
        common:     v0::ReceiveContext {
            metadata: ChainMetadata {
                slot_time: Timestamp::from_timestamp_millis(0),
            },
            invoker: owner,
            self_address: ContractAddress {
                index:    0,
                subindex: 0,
            },
            self_balance: Amount::from_ccd(1000),
            sender: Address::Account(owner),
            owner,
            sender_policies: &[],
        },
        entrypoint: OwnedEntrypointName::new_unchecked("entrypoint".into()),
    };

    let mut add_benchmark = |name: &str, args: [_; 1], n, empty_state: bool| {
        let params = vec![17u8; n];
        let inputs = if empty_state {
            Vec::new()
        } else {
            let mut inputs = Vec::with_capacity(n + 1);
            // construct the trie with the most nodes on the path to the
            // key we will look up.
            for i in 0..=n {
                inputs.push((params[0..i].to_vec(), i.to_be_bytes()));
            }
            inputs
        };
        let artifact = &artifact;
        let params = &params;
        let mk_data = || {
            let (a, b) = mk_state(&inputs);
            (a, b, vec![params.clone()])
        };
        let receive_ctx = &receive_ctx;
        let args = &args[..];
        group.bench_function(format!("{} n = {}", name, n), move |b: &mut criterion::Bencher| {
            b.iter_batched(
                mk_data,
                |(mut mutable_state, _, parameters)| {
                    let backing_store = Loader {
                        inner: Vec::new(),
                    };
                    let state = InstanceState::new(0, backing_store, mutable_state.get_inner());
                    let mut host = ReceiveHost::<_, Vec<u8>, _> {
                        energy: start_energy,
                        stateless: StateLessReceiveHost {
                            activation_frames: MAX_ACTIVATION_FRAMES,
                            logs: v0::Logs::new(),
                            receive_ctx,
                            return_value: Vec::new(),
                            parameters,
                        },
                        state,
                    };
                    let r = artifact
                        .run(&mut host, name, args)
                        .expect_err("Execution should fail due to out of energy.");
                    // Should fail due to out of energy.
                    assert!(
                        r.downcast_ref::<wasm_chain_integration::OutOfEnergy>().is_some(),
                        "Execution did not fail due to out of energy: {}.",
                        r
                    );
                    let params = std::mem::take(&mut host.stateless.parameters);
                    // it is not ideal to drop the host here since it might contain iterators and
                    // entries which do take a bit of time to drop.
                    drop(host);
                    // return the state so that its drop is not counted in the benchmark.
                    (mutable_state, params)
                },
                if n <= 10 {
                    BatchSize::SmallInput
                } else {
                    BatchSize::LargeInput
                },
            )
        });
    };

    for n in [0, 2, 10, 20, 40, 50, 100, 1000] {
        let name = "hostfn.state_create_entry";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, false);
    }

    for n in [0, 2, 10, 20, 50, 100, 1000] {
        let name = "hostfn.state_lookup_entry";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, false);
    }

    for n in [0, 2, 10, 20, 50, 100, 1000] {
        let name = "hostfn.state_entry_size";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, false);
    }

    for n in [0, 2, 10, 20, 50, 100, 1000] {
        let name = "hostfn.state_entry_read";
        let args = [machine::Value::I64(n as i64)];
        add_benchmark(name, args, n, false);
    }

    for n in [0, 2, 10, 20, 50, 100, 1000, 10000] {
        let name = "hostfn.state_entry_write";
        let args = [machine::Value::I64(n as i64)];
        add_benchmark(name, args, n, false)
    }

    for n in [0, 2, 10, 20, 50, 100, 1000, 10000] {
        let name = "hostfn.state_delete_entry";
        let args = [machine::Value::I64(n as i64)];
        add_benchmark(name, args, n, false)
    }

    for n in [0, 2, 10, 20, 50, 100, 1000, 10000] {
        let name = "hostfn.state_delete_entry_nonexistent";
        let args = [machine::Value::I64(n as i64)];
        add_benchmark(name, args, n, false);
    }

    for n in [0, 2, 10, 20, 50, 100, 1000, 10000] {
        let name = "hostfn.state_iterate_prefix";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, false);
    }

    for n in [0, 2, 10, 20, 50, 100, 1000, 10000] {
        let name = "hostfn.state_delete_prefix";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, false);
    }

    for n in [0, 2, 10, 20, 50, 100, 1000, 10000] {
        let name = "hostfn.state_iterator_key_size";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, false)
    }

    for n in [0, 2, 10, 20, 50, 100, 1000, 10000] {
        let name = "hostfn.state_iterator_key_read";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, false);
    }

    for n in [0, 2, 10, 20, 50, 100, 1000, 10000] {
        let name = "hostfn.state_iterator_delete";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, false)
    }

    for n in [0, 2, 10, 20, 50, 100, 10000] {
        let name = "hostfn.state_iterator_next";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, false)
    }

    for n in [0, 2, 10, 20, 50, 100, 10000] {
        let name = "hostfn.write_output";
        let args = [machine::Value::I64(0)];
        add_benchmark(name, args, n, true)
    }

    let mut add_invoke_benchmark = |name: &'static str, params: Vec<u8>, name_ext| {
        let args = [machine::Value::I64(0)];
        let inputs: Vec<(Vec<u8>, [u8; 1])> = Vec::new();
        let artifact = &artifact;
        let params = &params;
        let mk_data = || {
            let (a, b) = mk_state(&inputs);
            (a, b, vec![params.clone()])
        };
        let receive_ctx = &receive_ctx;
        let args = &args[..];
        let bench_name = if let Some(n) = name_ext {
            format!("{} n = {}", name, n)
        } else {
            name.to_string()
        };
        group.bench_function(bench_name, move |b: &mut criterion::Bencher| {
            b.iter_batched(
                mk_data,
                |(mut mutable_state, _, parameters)| {
                    let backing_store = Loader {
                        inner: Vec::new(),
                    };
                    let state = InstanceState::new(0, backing_store, mutable_state.get_inner());
                    let mut host = ReceiveHost::<_, Vec<u8>, _> {
                        energy: start_energy,
                        stateless: StateLessReceiveHost {
                            activation_frames: MAX_ACTIVATION_FRAMES,
                            logs: v0::Logs::new(),
                            receive_ctx,
                            return_value: Vec::new(),
                            parameters,
                        },
                        state,
                    };
                    match artifact.run(&mut host, name, args) {
                        Ok(r) => match r {
                            machine::ExecutionOutcome::Success {
                                ..
                            } => panic!("Execution terminated, but it was not expected to."),
                            machine::ExecutionOutcome::Interrupted {
                                reason: _,
                                config,
                            } => {
                                let mut current_config = config;
                                loop {
                                    current_config.push_value(0u64); // push the response to the stack, the value is not inspected.
                                    match artifact.run_config(&mut host, current_config) {
                                        Ok(r) => {
                                            match r {
                                                machine::ExecutionOutcome::Success { .. } => panic!("Execution terminated, but it was not expected to."),
                                                machine::ExecutionOutcome::Interrupted { config,.. } => {
                                                    current_config = config;
                                                }
                                            }
                                        }
                                        Err(r) => {
                                            // Should fail due to out of energy.
                                            assert!(
                                                r.downcast_ref::<wasm_chain_integration::OutOfEnergy>().is_some(),
                                                "Execution did not fail due to out of energy: {}.",
                                                r
                                            );
                                            break;
                                        }
                                    }
                                }
                            }
                        },
                        Err(err) => {
                            panic!("Initial invocation should not fail: {}", err);
                        }
                    }
                    // it is not ideal to drop the host here since it might contain iterators and
                    // entries which do take a bit of time to drop.
                    drop(host);
                    // return the state so that its drop is not counted in the benchmark.
                    (mutable_state, params)
                },
                BatchSize::SmallInput
            )
        });
    };

    {
        let name = "hostfn.invoke_transfer";
        let params = vec![0u8; 32 + 8]; // address + amount
        add_invoke_benchmark(name, params, None);
    }

    {
        // n is the length of the parameter
        for n in [0, 10, 20, 50, 100, 1000, 10000] {
            let name = "hostfn.invoke_contract";
            let mut params = vec![0u8; 16 + 2 + n + 2 + 8]; // address + amount
            params[16..16 + 2].copy_from_slice(&(n as u16).to_le_bytes());
            add_invoke_benchmark(name, params, Some(n));
        }
    }

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
