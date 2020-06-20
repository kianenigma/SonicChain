use primitives::*;
use state::{GenericState, TaintState};
use std::cell::RefCell;
use std::sync::Arc;

// TODO: the relation between the Runtime and each module is not ideal. Preferably I'd like to make
// each module be generic over an `<R: GenericRuntime>` and then just work with that, without the
// need to create and pass down an instance of `runtime: R`. But I am not sure how this would look
// like. And indeed the answer will probably be unsafe rust, so something for another day.

/// The state type of the runtime.
pub type RuntimeState = TaintState<Key, Value, ThreadId>;

pub mod balances;
// pub mod staking;

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum DispatchError {
	/// The dispatch attempted at modifying a state key which has been tainted.
	Tainted(ThreadId),
	/// The transaction had a logical error.
	LogicError(&'static str),
}

/// The result of a dispatch.
pub type DispatchResult = Result<(), DispatchError>;

/// The result of the validation of a dispatchable.
pub type ValidationResult = Result<(), ()>;

/// Anything that can be dispatched.
///
/// Both the inner call and the outer call will be of type Dispatchable.
pub trait Dispatchable<R: GenericRuntime> {
	/// Dispatch this dispatchable.
	fn dispatch(&self, runtime: &R, origin: AccountId) -> DispatchResult;

	/// Validate this dispatchable.
	///
	/// This should be cheap and return potentially some useful metadata about the dispatchable.
	fn validate(&self, _: &R, _: AccountId) -> ValidationResult {
		Ok(())
	}
}

/// The outer call of the runtime.
///
/// This is an encoding of all the transactions that can be executed.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum OuterCall {
	Balances(balances::Call),
}

impl<R: GenericRuntime> Dispatchable<R> for OuterCall {
	fn dispatch(&self, runtime: &R, origin: AccountId) -> DispatchResult {
		match self {
			OuterCall::Balances(inner_call) => {
				<balances::Call as Dispatchable<R>>::dispatch(inner_call, &runtime, origin)
			}
		}
	}
}

/// Interface of the runtime that will be available to each module.
pub trait GenericRuntime {
	/// The thread id of the runtime.
	fn thread_id(&self) -> ThreadId;

	/// Read from storage.
	fn read(&self, key: &Key) -> Result<Value, ThreadId>;

	/// Write to storage.
	fn write(&self, key: &Key, value: Value) -> Result<(), ThreadId>;

	/// Mutate storage
	fn mutate(&self, key: &Key, update: impl Fn(&mut Value) -> ()) -> Result<(), ThreadId>;
}

/// A runtime.
///
/// The main functionality of the runtime is that it orchestrates the execution of transactions.
pub struct Runtime {
	/// The state pointer.
	state: Arc<RuntimeState>,
	/// Id of the thread.
	id: ThreadId,
}

impl Runtime {
	/// Create a new runtime.
	pub fn new(state: Arc<RuntimeState>, id: ThreadId) -> Self {
		Self { state, id }
	}

	/// Dispatch a call.
	pub fn dispatch(&self, call: OuterCall, origin: AccountId) -> DispatchResult {
		<OuterCall as Dispatchable<Self>>::dispatch(&call, self, origin)
	}

	/// Validate a call.
	pub fn validate(&self, call: OuterCall, origin: AccountId) -> ValidationResult {
		<OuterCall as Dispatchable<Self>>::validate(&call, self, origin)
	}
}

impl GenericRuntime for Runtime {
	fn thread_id(&self) -> ThreadId {
		self.id
	}

	fn read(&self, key: &Key) -> Result<Value, ThreadId> {
		self.state.read(key, self.id)
	}

	fn write(&self, key: &Key, value: Value) -> Result<(), ThreadId> {
		self.state.write(key, value, self.id)
	}

	fn mutate(&self, key: &Key, update: impl Fn(&mut Value) -> ()) -> Result<(), ThreadId> {
		self.state.mutate(key, update, self.id)
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use primitives::AccountId;
	use state::TaintState;
	use std::sync::Arc;

	#[test]
	fn can_access_storage() {
		let state = RuntimeState::new().as_arc();

		let state_ptr = Arc::clone(&state);
		std::thread::spawn(move || {
			let runtime = Runtime::new(state_ptr, 1);
			let tx = OuterCall::Balances(balances::Call::Transfer(AccountId::random(), 1000));
			assert!(runtime.dispatch(tx.clone(), AccountId::random()).is_ok());
			assert!(runtime.validate(tx, AccountId::random()).is_ok());
		});

		let state_ptr = Arc::clone(&state);
		std::thread::spawn(move || {
			let runtime = Runtime::new(state_ptr, 2);
			let tx = OuterCall::Balances(balances::Call::Transfer(AccountId::random(), 1000));
			assert!(runtime.dispatch(tx.clone(), AccountId::random()).is_ok());
			assert!(runtime.validate(tx, AccountId::random()).is_ok());
		});
	}
}
