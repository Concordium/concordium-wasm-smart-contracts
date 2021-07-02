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

impl ChainMetadataOpt {
    fn new() -> Self {
        Self {
            slot_time: None,
        }
    }
}

impl Default for ChainMetadataOpt {
    fn default() -> Self { Self::new() }
}

impl HasChainMetadata for ChainMetadataOpt {
    fn slot_time(&self) -> ExecResult<SlotTime> { unwrap_ctx_field(self.slot_time, "slotTime") }
}

/// An init context with optional fields.
/// Used when simulating contracts to allow the user to only specify the
/// context fields used by the contract.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitContextOpt {
    #[serde(default)]
    metadata:        ChainMetadataOpt,
    init_origin:     Option<AccountAddress>,
    sender_policies: Option<Vec<OwnedPolicy>>,
}

impl InitContextOpt {
    pub fn new() -> Self {
        Self {
            metadata:        ChainMetadataOpt::new(),
            init_origin:     None,
            sender_policies: None,
        }
    }
}

impl HasInitContext for InitContextOpt {
    type MetadataType = ChainMetadataOpt;
    type PolicyBytesType = Vec<u8>;
    type PolicyType = Vec<OwnedPolicy>;

    fn metadata(&self) -> &Self::MetadataType { &self.metadata }

    fn init_origin(&self) -> ExecResult<&AccountAddress> {
        unwrap_ctx_field(self.init_origin.as_ref(), "initOrigin")
    }

    fn sender_policies(&self) -> ExecResult<&Self::PolicyType> {
        unwrap_ctx_field(self.sender_policies.as_ref(), "senderPolicies")
    }
}

/// A receive context with optional fields.
/// Used when simulating contracts to allow the user to only specify the
/// context fields used by the contract.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReceiveContextOpt {
    #[serde(default)]
    metadata:                ChainMetadataOpt,
    invoker:                 Option<AccountAddress>,
    self_address:            Option<ContractAddress>,
    // This is pub(crate) because it is overwritten when `--balance` is used.
    pub(crate) self_balance: Option<Amount>,
    sender:                  Option<Address>,
    owner:                   Option<AccountAddress>,
    sender_policies:         Option<Vec<OwnedPolicy>>,
}

impl ReceiveContextOpt {
    pub fn new() -> Self {
        Self {
            metadata:        ChainMetadataOpt::new(),
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
    type PolicyBytesType = Vec<u8>;
    type PolicyType = Vec<OwnedPolicy>;

    fn metadata(&self) -> &Self::MetadataType { &self.metadata }

    fn invoker(&self) -> ExecResult<&AccountAddress> {
        unwrap_ctx_field(self.invoker.as_ref(), "metadata")
    }

    fn self_address(&self) -> ExecResult<&ContractAddress> {
        unwrap_ctx_field(self.self_address.as_ref(), "selfAddress")
    }

    fn self_balance(&self) -> ExecResult<Amount> {
        unwrap_ctx_field(self.self_balance, "selfBalance")
    }

    fn sender(&self) -> ExecResult<&Address> { unwrap_ctx_field(self.sender.as_ref(), "sender") }

    fn owner(&self) -> ExecResult<&AccountAddress> {
        unwrap_ctx_field(self.owner.as_ref(), "owner")
    }

    fn sender_policies(&self) -> ExecResult<&Self::PolicyType> {
        unwrap_ctx_field(self.sender_policies.as_ref(), "senderPolicies")
    }
}

// Error handling when unwrapping
fn unwrap_ctx_field<A>(opt: Option<A>, name: &str) -> ExecResult<A> {
    match opt {
        Some(v) => Ok(v),
        None => Err(anyhow!(
            "Missing field '{}' in the context. Make sure to provide a context file with all the \
             fields the contract uses.",
            name,
        )),
    }
}
