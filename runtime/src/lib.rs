use primitives::*;
use state::GenericState;

pub mod balances;
pub mod staking;

/// The result of a dispatch.
pub type DispatchResult = Result<(), ThreadId>;

/// The result of the validation of a dispatchable.
pub type ValidationResult = Result<(), ()>;

/// Anything that can be dispatched.
///
/// Both the inner call and the outer call will be of type Dispatchable.
pub trait Dispatchable<S: GenericState<Key, Value, ThreadId>> {
	/// Dispatch this dispatchable.
	fn dispatch(&self, state: &S, origin: AccountId) -> DispatchResult;

	/// Validate this dispatchable.
	///
	/// This should be cheap and return potentially some useful metadata about the dispatchable.
	fn validate(&self, _: &S, _: AccountId) -> ValidationResult {
		Ok(())
	}
}

/// The outer call of the runtime.
///
/// This is an encoding of all the transactions that can be executed.
#[derive(Debug, Clone)]
pub enum OuterCall {
	Balances(balances::Call),
}

impl<S: GenericState<Key, Value, ThreadId>> Dispatchable<S> for OuterCall {
	fn dispatch(&self, state: &S, origin: AccountId) -> DispatchResult {
		match self {
			OuterCall::Balances(inner) => inner.dispatch(state, origin),
		}
	}
}

/// A runtime.
///
/// The main functionality of the runtime is that it orchestrates the execution of transactions.
pub struct Runtime<'a, S: GenericState<Key, Value, ThreadId>> {
	/// A reference to a state.
	pub state: &'a S,
	/// The id of the current runtime.
	pub id: ThreadId,
}

impl<'a, S> Runtime<'a, S>
where
	S: GenericState<Key, Value, ThreadId>,
{
	/// Create a new runtime.
	pub fn new(state: &'a S, id: ThreadId) -> Self {
		Self { state, id }
	}

	/// Dispatch a call.
	pub fn dispatch(&self, call: OuterCall, origin: AccountId) -> DispatchResult {
		call.dispatch(self.state, origin)
	}

	/// Validate a call.
	pub fn validate(&self, call: OuterCall, origin: AccountId) -> ValidationResult {
		call.validate(self.state, origin)
	}

	/// Read from storage.
	pub fn read(&self, key: &Key) -> Result<Value, ThreadId> {
		self.state.read(key, self.id)
	}

	/// Write to storage.
	pub fn write(&self, key: &Key, value: Value) -> Result<(), ThreadId> {
		self.state.write(key, value, self.id)
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use state::TaintState;
	use std::sync::Arc;

	// TODO: I can see the value in making the runtime also generic on this stuff.
	type TestState = TaintState<Key, Value, ThreadId>;

	#[test]
	fn can_access_storage() {
		let state = TestState::new();
		let state = Arc::from(state);

		let state_ptr = Arc::clone(&state);
		std::thread::spawn(move || {
			let runtime = Runtime::new(state_ptr.as_ref(), 1);
			let transaction = OuterCall::Balances(balances::Call::Transfer(100, 1000));
			assert!(runtime.dispatch(transaction.clone(), 2).is_ok());
			assert!(runtime.validate(transaction, 2).is_ok());
		});

		let state_ptr = Arc::clone(&state);
		std::thread::spawn(move || {
			let runtime = Runtime::new(state_ptr.as_ref(), 2);
			let transaction = OuterCall::Balances(balances::Call::Transfer(20, 1000));
			assert!(runtime.dispatch(transaction.clone(), 1).is_ok());
			assert!(runtime.validate(transaction, 1).is_ok());
		});
	}
}
