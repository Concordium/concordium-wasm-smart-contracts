use anyhow::anyhow;
use concordium_contracts_common::{
    AccountAddress, Address, Amount, ContractAddress, OwnedPolicy, SlotTime,
};
use wasm_chain_integration::{ExecResult, HasChainMetadata, HasInitContext, HasReceiveContext};

/// A chain metadata with an optional field.
/// Used when simulating contracts to allow the user to only specify the
/// necessary context fields.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChainMetadataOpt {
    slot_time: Option<SlotTime>,
}

impl HasChainMetadata for ChainMetadataOpt {
    fn slot_time(&self) -> ExecResult<SlotTime> { unwrap_ctx_field(self.slot_time, "slot_time") }
}

/// An init context with optional fields.
/// Used when simulating contracts to allow the user to only specify the
/// necessary context fields.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitContextOpt<Policies = Vec<OwnedPolicy>> {
    metadata:        Option<ChainMetadataOpt>,
    init_origin:     Option<AccountAddress>,
    sender_policies: Option<Policies>,
}

impl InitContextOpt {
    pub fn new() -> Self {
        Self {
            metadata:        None,
            init_origin:     None,
            sender_policies: None,
        }
    }
}

impl HasInitContext for InitContextOpt {
    type MetadataType = ChainMetadataOpt;

    fn metadata(&self) -> ExecResult<&Self::MetadataType> {
        unwrap_ctx_field(self.metadata.as_ref(), "metadata")
    }

    fn init_origin(&self) -> ExecResult<AccountAddress> {
        unwrap_ctx_field(self.init_origin, "init_origin")
    }

    fn sender_policies(&self) -> ExecResult<&Vec<OwnedPolicy>> {
        unwrap_ctx_field(self.sender_policies.as_ref(), "sender_policies")
    }
}

/// A receive context with optional fields.
/// Used when simulating contracts to allow the user to only specify the
/// necessary context fields.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReceiveContextOpt<Policies = Vec<OwnedPolicy>> {
    metadata:                Option<ChainMetadataOpt>,
    invoker:                 Option<AccountAddress>,
    self_address:            Option<ContractAddress>,
    pub(crate) self_balance: Option<Amount>,
    sender:                  Option<Address>,
    owner:                   Option<AccountAddress>,
    sender_policies:         Option<Policies>,
}

impl ReceiveContextOpt {
    pub fn new() -> Self {
        Self {
            metadata:        None,
            invoker:         None,
            self_address:    None,
            self_balance:    None,
            sender:          None,
            owner:           None,
            sender_policies: None,
        }
    }
}

impl HasReceiveContext for ReceiveContextOpt {
    type MetadataType = ChainMetadataOpt;

    fn metadata(&self) -> ExecResult<&Self::MetadataType> {
        unwrap_ctx_field(self.metadata.as_ref(), "metadata")
    }

    fn invoker(&self) -> ExecResult<AccountAddress> { unwrap_ctx_field(self.invoker, "metadata") }

    fn self_address(&self) -> ExecResult<ContractAddress> {
        unwrap_ctx_field(self.self_address, "self_address")
    }

    fn self_balance(&self) -> ExecResult<Amount> {
        unwrap_ctx_field(self.self_balance, "self_balance")
    }

    fn sender(&self) -> ExecResult<Address> { unwrap_ctx_field(self.sender, "sender") }

    fn owner(&self) -> ExecResult<AccountAddress> { unwrap_ctx_field(self.owner, "owner") }

    fn sender_policies(&self) -> ExecResult<&Vec<OwnedPolicy>> {
        unwrap_ctx_field(self.sender_policies.as_ref(), "sender_policies")
    }
}

// Error handling when unwrapping
fn unwrap_ctx_field<A>(opt: Option<A>, name: &str) -> ExecResult<A> {
    match opt {
        Some(v) => Ok(v),
        None => Err(anyhow!(
            "Missing field '{}' in the context. Make sure to provide a context file with all the \
             necessary fields for the contract.",
            name,
        )),
    }
}
