use parity_scale_codec::{Decode, Encode};
use primitives::*;
use state::{GenericState, TaintState};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

// re-exports for decl_storage macros.
pub use primitives;

/// The state type of the runtime.
pub type RuntimeState = TaintState<Key, Value, ThreadId>;

pub mod balances;
pub mod staking;
pub mod storage_macros;

const LOG_TARGET: &'static str = "runtime";

macro_rules! log {
	($level:tt, $patter:expr $(, $values:expr)* $(,)?) => {
		log::$level!(
			target: LOG_TARGET,
			$patter $(, $values)*
		)
	};
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum DispatchError {
	/// The dispatch attempted at modifying a state key which has been tainted.
	///
	/// The second element will be true if this failure should cause the entire execution to stop,
	/// because the state is now corrupt.
	Tainted(ThreadId, bool),
	/// The transaction had a logical error.
	LogicError(&'static str),
}

/// The result of a dispatch.
pub type DispatchResult = Result<(), DispatchError>;

/// The result of the validation of a dispatchable.
pub type ValidationResult = Result<Vec<primitives::Key>, ()>;

/// Anything that can be dispatched.
///
/// Both the inner call and the outer call will be of type Dispatchable.
pub trait Dispatchable<R: GenericRuntime> {
	/// Dispatch this dispatchable.
	fn dispatch(&self, runtime: &R, origin: AccountId) -> DispatchResult;

	/// Validate this dispatchable.
	///
	/// This should be cheap and return potentially some useful metadata about the dispatchable.
	fn validate(&self, _: &R, _: AccountId) -> ValidationResult;
}

/// The outer call of the runtime.
///
/// This is an encoding of all the transactions that can be executed.
#[derive(Debug, Clone, Eq, PartialEq, Encode, Decode)]
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

	fn validate(&self, runtime: &R, origin: AccountId) -> ValidationResult {
		match self {
			OuterCall::Balances(inner_call) => {
				<balances::Call as Dispatchable<R>>::validate(inner_call, runtime, origin)
			}
		}
	}
}

/// Interface of the runtime that will be available to each module.
pub trait GenericRuntime {
	/// If this runtime is restricted or not. If true, the storage access will be subject to
	/// tainting. Else, all access' are guaranteed to go well.
	const LIMITED: bool;

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
pub struct WorkerRuntime {
	/// The state pointer.
	state: Arc<RuntimeState>,
	/// Thread local state cache.
	cache: RefCell<HashMap<Key, Value>>,
	/// Id of the thread.
	id: ThreadId,
}

impl WorkerRuntime {
	/// Create a new runtime.
	pub fn new(state: Arc<RuntimeState>, id: ThreadId) -> Self {
		Self {
			state,
			id,
			cache: HashMap::new().into(),
		}
	}

	/// Dispatch a call.
	// TODO: force worker to call this from this runtime, not via the trait.
	pub fn dispatch(&self, call: &OuterCall, origin: AccountId) -> DispatchResult {
		// the cache must always be empty at the beginning of a dispatch.
		debug_assert_eq!(self.cache.borrow().keys().len(), 0);

		// execute
		let dispatch_result = <OuterCall as Dispatchable<Self>>::dispatch(call, self, origin);

		// only commit if result is ok, or if the error is logic error.
		match dispatch_result {
			Ok(_) | Err(DispatchError::LogicError(_)) => self.commit_cache(),
			_ => (),
		};

		log!(
			trace,
			"worker runtime executed {:?}. result is {:?}. cached writes {}",
			call,
			dispatch_result,
			self.cache.borrow().len()
		);

		// clear the cache anyhow.
		self.cache.borrow_mut().clear();
		dispatch_result
	}

	/// commit the cache to the persistent state.
	pub fn commit_cache(&self) {
		// TODO: we can use unsafe insert here; we know that we own this shit
		self.cache.borrow().iter().for_each(|(k, v)| {
			self.state
				.write(k, v.clone(), self.id)
				.expect("We own the data")
		});
	}

	/// Validate a call.
	pub fn validate(&self, call: &OuterCall, origin: AccountId) -> ValidationResult {
		<OuterCall as Dispatchable<Self>>::validate(call, self, origin)
	}
}

impl GenericRuntime for WorkerRuntime {
	const LIMITED: bool = true;

	fn thread_id(&self) -> ThreadId {
		self.id
	}

	fn read(&self, key: &Key) -> Result<Value, ThreadId> {
		// if this value is in the cache, then it belongs to us and return the cached value.
		if let Some(value) = self.cache.borrow().get(key) {
			Ok(value.clone())
		} else {
			self.state.read(key, self.id)
		}
	}

	fn write(&self, key: &Key, value: Value) -> Result<(), ThreadId> {
		match self.read(key) {
			Ok(_) => {
				self.cache.borrow_mut().insert(key.clone(), value);
				Ok(())
			}
			Err(owner) => Err(owner),
		}
	}

	fn mutate(&self, key: &Key, update: impl Fn(&mut Value) -> ()) -> Result<(), ThreadId> {
		match self.read(key) {
			Ok(mut old) => {
				update(&mut old);
				self.cache.borrow_mut().insert(key.clone(), old);
				Ok(())
			}
			Err(owner) => Err(owner),
		}
	}
}

#[derive(Debug, Default)]
pub struct MasterRuntime {
	state: Arc<RuntimeState>,
	id: ThreadId,
}

impl MasterRuntime {
	/// Create new master runtime.
	pub fn new(state: Arc<RuntimeState>, id: ThreadId) -> Self {
		Self { state, id }
	}

	/// Dispatch a call.
	pub fn dispatch(&self, call: &OuterCall, origin: AccountId) -> DispatchResult {
		<OuterCall as Dispatchable<Self>>::dispatch(call, self, origin)
	}

	/// Validate a call.
	pub fn validate(&self, call: &OuterCall, origin: AccountId) -> ValidationResult {
		<OuterCall as Dispatchable<Self>>::validate(call, self, origin)
	}
}

impl GenericRuntime for MasterRuntime {
	const LIMITED: bool = false;

	fn thread_id(&self) -> ThreadId {
		self.id
	}

	fn read(&self, key: &Key) -> Result<Value, ThreadId> {
		Ok(self.state.unsafe_read_value(key).unwrap_or_default())
	}

	fn write(&self, key: &Key, value: Value) -> Result<(), ThreadId> {
		self.state.unsafe_insert_genesis_value(key, value);
		Ok(())
	}

	fn mutate(&self, key: &Key, update: impl Fn(&mut Value) -> ()) -> Result<(), ThreadId> {
		let mut old = self.read(key).expect("Self::read cannot fail");
		update(&mut old);
		self.write(key, old).expect("Self::write cannot fail");
		Ok(())
	}
}

#[cfg(test)]
mod worker_runtime_test {
	use super::*;
	use primitives::*;
	use std::sync::Arc;

	#[test]
	fn basic_worker_runtime_works() {
		let state = RuntimeState::new().as_arc();
		let k1: StateKey = vec![1u8].into();
		let rt = WorkerRuntime::new(state, 1);

		let r: Vec<u8> = rt.read(&k1).unwrap().0;
		assert_eq!(r, vec![]);

		assert!(rt.write(&k1, vec![1, 2, 3].into()).is_ok());
		assert_eq!(rt.read(&k1).unwrap().0, vec![1u8, 2, 3]);

		assert!(rt.mutate(&k1, |val| val.0.push(99)).is_ok());
		assert_eq!(rt.read(&k1).unwrap().0, vec![1u8, 2, 3, 99]);
	}

	#[test]
	fn worker_runtime_fails_if_tainted() {
		let state = RuntimeState::new().as_arc();
		let k1: StateKey = vec![1u8].into();
		let rt = WorkerRuntime::new(Arc::clone(&state), 1);

		// current runtime is 1, taint with 2.
		state.unsafe_insert(&k1, state::StateEntry::new_taint(2));

		assert!(rt.read(&k1).is_err());
		assert!(rt.write(&k1, vec![1, 2, 3].into()).is_err());
		assert!(rt.mutate(&k1, |val| val.0.push(99)).is_err());
	}

	#[test]
	fn can_share_runtime_state_between_threads() {
		let state = RuntimeState::new().as_arc();

		let state_ptr = Arc::clone(&state);
		std::thread::spawn(move || {
			let runtime = WorkerRuntime::new(state_ptr, 1);
			let tx =
				OuterCall::Balances(balances::Call::Transfer(testing::random().public(), 1000));
			assert!(runtime.dispatch(&tx, testing::random().public()).is_ok());
			assert!(runtime.validate(&tx, testing::random().public()).is_ok());
		});

		let state_ptr = Arc::clone(&state);
		std::thread::spawn(move || {
			let runtime = WorkerRuntime::new(state_ptr, 2);
			let tx =
				OuterCall::Balances(balances::Call::Transfer(testing::random().public(), 1000));
			assert!(runtime.dispatch(&tx, testing::random().public()).is_ok());
			assert!(runtime.validate(&tx, testing::random().public()).is_ok());
		});
	}

	#[test]
	#[ignore]
	fn worker_runtime_caching_works() {
		todo!()
	}
}

#[cfg(test)]
mod master_runtime_test {
	use super::*;

	#[test]
	fn basic_master_runtime_works() {
		let state = RuntimeState::new().as_arc();
		let k1: StateKey = vec![1u8].into();
		let rt = MasterRuntime::new(state, 1);

		let r: Vec<u8> = rt.read(&k1).unwrap().0;
		assert_eq!(r, vec![]);

		assert!(rt.write(&k1, vec![1, 2, 3].into()).is_ok());
		assert_eq!(rt.read(&k1).unwrap().0, vec![1u8, 2, 3]);

		assert!(rt.mutate(&k1, |val| val.0.push(99)).is_ok());
		assert_eq!(rt.read(&k1).unwrap().0, vec![1u8, 2, 3, 99]);
	}

	#[test]
	fn master_runtime_works_regardless_of_taint() {
		let state = RuntimeState::new().as_arc();
		let k1: StateKey = vec![1u8].into();
		let rt = MasterRuntime::new(Arc::clone(&state), 1);

		// current runtime is 1, taint with 2.
		state.unsafe_insert(&k1, state::StateEntry::new_taint(2));

		assert!(rt.read(&k1).is_ok());
		assert!(rt.write(&k1, vec![1, 2, 3].into()).is_ok());
		assert!(rt.mutate(&k1, |val| val.0.push(99)).is_ok());
	}
}
