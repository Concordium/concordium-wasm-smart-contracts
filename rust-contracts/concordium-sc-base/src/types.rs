/// A type representing the constract state bytes.
#[derive(Default)]
pub struct ContractState {
    pub(crate) current_position: u32,
}

/// Actions that can be produced at the end of a contract execution. This
/// type is deliberately not cloneable so that we can enforce that
/// `and_then` and `or_else` can only be used when more than one event is
/// created.
///
/// This type is marked as `must_use` since functions that produce
/// values of the type are effectful.
#[must_use]
pub struct Action {
    pub(crate) _private: (),
}

/// Result of a successful smart contract execution receive method.
pub enum ReceiveActions {
    /// Simply accept the invocation, with no additional actions.
    Accept,
    /// Accept with the given action tree.
    AcceptWith(Action),
}

/// A non-descript error message, signalling rejection of a smart contract
/// invocation.
#[derive(Default)]
pub struct Reject {}

#[macro_export]
/// The `bail` macro can be used for cleaner error handling. If the function has
/// result type Result<_, Reject> then invoking `bail` will terminate execution
/// early with an error. If the macro is invoked with a string message the
/// message will be logged before the function terminates.
macro_rules! bail {
    () => {
        return Err(Reject {})
    };
    ($e:expr) => {{
        $crate::events::log_str($e);
        return Err(Reject {});
    }};
}

#[macro_export]
/// The `ensure` macro can be used for cleaner error handling. It is analogous
/// to `assert`, but instead of panicking it uses `bail` to terminate execution
/// of the function early.
macro_rules! ensure {
    ($p:expr) => {
        if !$p {
            return Err(Reject {});
        }
    };
    ($p:expr, $e:expr) => {{
        if !$p {
            $crate::bail!($e)
        }
    }};
}

/// The expected return type of the receive method of a smart contract.
pub type ReceiveResult = Result<ReceiveActions, Reject>;

/// The expected return type of the init method of the smart contract,
/// parametrized by the state type of the smart contract.
pub type InitResult<S> = Result<S, Reject>;
