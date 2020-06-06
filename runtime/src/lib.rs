pub use primitives::*;
use state::{GenericState, KeyT, TaintState, TaintT, ValueT};
use std::marker::PhantomData;

pub type State = dyn GenericState<Key, Value, ThreadId>;

pub type DispatchResult = Result<(), ()>;
pub type ValidationResult = Result<(), ()>;

pub trait Dispatchable<S: GenericState<Key, Value, ThreadId>> {
	fn dispatch(&self, state: &S, origin: Origin) -> DispatchResult;
	fn validate(&self, _: &S, _: Origin) -> ValidationResult {
		Ok(())
	}
}

pub struct Runtime<'a, S: GenericState<Key, Value, ThreadId>> {
	state: &'a S,
}

impl<'a, S> Runtime<'a, S>
where
	S: GenericState<Key, Value, ThreadId>,
{
	fn new(state: &'a S) -> Self {
		Self { state }
	}

	fn dispatch(&self, call: Call, origin: Origin) -> DispatchResult {
		call.dispatch(self.state, origin)
	}

	fn validate(&self, call: Call, origin: Origin) -> ValidationResult {
		call.validate(self.state, origin)
	}
}

#[derive(Debug, Clone)]
enum Call {
	Balances(balances::Call),
}

impl<S: GenericState<Key, Value, ThreadId>> Dispatchable<S> for Call {
	fn dispatch(&self, state: &S, origin: Origin) -> DispatchResult {
		match self {
			Call::Balances(inner) => inner.dispatch(state, origin),
		}
	}
}

mod balances {
	use super::*;

	#[derive(Debug, Clone)]
	pub enum Call {
		Transfer(u64, u64),
	}

	impl<S: GenericState<Key, Value, ThreadId>> Dispatchable<S> for Call {
		fn dispatch(&self, state: &S, origin: Origin) -> DispatchResult {
			match *self {
				Self::Transfer(to, value) => transfer(state, origin, to, value),
			}
		}
	}

	fn transfer<S: GenericState<Key, Value, ThreadId>>(
		state: &S,
		origin: Origin,
		to: u64,
		value: u64,
	) -> DispatchResult {
		state.read(&10, 1);
		state.write(&10, vec![1, 2, 3], 1);
		Ok(())
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use std::sync::Arc;

	// TODO: I can see the value in making the runtime also generic on this stuff.
	type TestState = TaintState<Key, Value, ThreadId>;

	#[test]
	fn can_access_storage() {
		let state = TestState::new();
		let state = Arc::from(state);

		let state_ptr = Arc::clone(&state);
		std::thread::spawn(move || {
			let transaction = Call::Balances(balances::Call::Transfer(100, 1000));
			let runtime = Runtime::new(state_ptr.as_ref());
			assert!(runtime.dispatch(transaction.clone(), 2).is_ok());
			assert!(runtime.validate(transaction, 2).is_ok());
		});

		let state_ptr = Arc::clone(&state);
		std::thread::spawn(move || {
			let transaction = Call::Balances(balances::Call::Transfer(20, 1000));
			let runtime = Runtime::new(state_ptr.as_ref());
			assert!(runtime.dispatch(transaction.clone(), 1).is_ok());
			assert!(runtime.validate(transaction, 1).is_ok());
		});
	}
}
