use logging::log;
use parity_scale_codec::{Decode, Encode};
use primitives::*;
use state::{GenericState, TaintState};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

pub mod balances;
mod macros;
pub mod staking;
pub mod state_generator;

// re-export paste for macros.
#[doc(hidden)]
pub use paste::paste;

// re-exports.
pub use primitives;

// re-exports.
pub use state_generator::InitialStateGenerate;

/// The state type of the runtime.
pub type RuntimeState = TaintState<Key, Value, ThreadId>;

/// The inner hash map used in state.
pub type StateMap = state::MapType<Key, Value, ThreadId>;

const LOG_TARGET: &'static str = "runtime";

/// The error types returned from a dispatch function.
///
/// This is only used internally in this crate.
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum DispatchError {
	/// The dispatch attempted at modifying a state key which has been tainted.
	///
	/// The second element will be true if this failure should cause the entire execution to stop,
	/// because the state is now corrupt.
	///
	/// A transaction logic should only set this true to enforce the transaction to be forwarded to
	/// master as an orphan afterwards. This generally means: If a transaction DOES access a key k1,
	/// either by reading or writing, and fails on a consecutive access to k2. In this case, there
	/// is no point in sending this to whoever owns k2, because they most certainly do NOT own k1.
	/// Hence, in such cases, the transaction logic should enforce a the transaction to be forwarded
	/// to master.
	Tainted(ThreadId, bool),
	/// The transaction had a logical error.
	LogicError(&'static str),
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum RuntimeDispatchSuccess {
	/// Execution went fine.
	Ok,
	/// Execution had a logical error.
	LogicError(&'static str),
}

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum RuntimeDispatchError {
	/// The dispatch attempted at modifying a state key which has been tainted.
	Tainted(ThreadId, bool),
}

/// The result of a dispatch.
///
/// This is used internally, this pub(crate).
pub(crate) type DispatchResult = Result<(), DispatchError>;

/// The final result of the execution of a dispatch.
///
/// This is similar to `DispatchResult`, except that a logical error is technically `Ok`.
/// Only error is due to tainting.
pub type RuntimeDispatchResult = Result<RuntimeDispatchSuccess, RuntimeDispatchError>;

/// Conversion trait between `DispatchResult` and `RuntimeDispatchResult`.
trait ToRuntimeDispatchResult {
	fn to_runtime_dispatch_result(self) -> RuntimeDispatchResult;
}

impl ToRuntimeDispatchResult for DispatchResult {
	fn to_runtime_dispatch_result(self) -> RuntimeDispatchResult {
		match self {
			Ok(_) => Ok(RuntimeDispatchSuccess::Ok),
			Err(DispatchError::LogicError(why)) => Ok(RuntimeDispatchSuccess::LogicError(why)),
			Err(DispatchError::Tainted(whom, orphan)) => {
				Err(RuntimeDispatchError::Tainted(whom, orphan))
			}
		}
	}
}

/// Extension trait to convert the result of storage operations to another result with
/// `DispatchError` in the `Err` variant.
trait UnwrapStorageOp<T> {
	/// Map to `Tainted(t, false)`.
	///
	/// This basically means that the transaction need to be forwarded to another thread. This only
	/// makes sense on the first storage operation of the transaction.
	fn or_forward(self) -> Result<T, DispatchError>;

	/// Map to `Tainted(t, true)`.
	///
	/// This basically means that the transaction need to be forwarded to master as orphan. This
	/// only makes sense on the first storage operation of the transaction.
	fn or_orphan(self) -> Result<T, DispatchError>;
}

impl<T> UnwrapStorageOp<T> for Result<T, primitives::ThreadId> {
	fn or_forward(self) -> Result<T, DispatchError> {
		self.map_err(|t| DispatchError::Tainted(t, false))
	}

	fn or_orphan(self) -> Result<T, DispatchError> {
		self.map_err(|t| DispatchError::Tainted(t, true))
	}
}

/// The result of the validation of a dispatchable.
pub type ValidationResult = Vec<primitives::Key>;

/// Anything that can be dispatched.
///
/// Both the inner call and the outer call will be of type Dispatchable.
pub trait Dispatchable<R: ModuleRuntime> {
	/// Dispatch this dispatchable.
	///
	/// This consumes the call.
	fn dispatch<T: DispatchPermission>(self, runtime: &R, origin: AccountId) -> DispatchResult;

	/// Validate this dispatchable.
	///
	/// This should be cheap and return potentially some useful metadata about the dispatchable.
	fn validate(&self, _: &R, _: AccountId) -> ValidationResult;
}

/// Marker trait for those who have permission to dispatch.
pub trait DispatchPermission {}

decl_outer_call!(
	pub enum OuterCall {
		Balances(balances::Call),
		Staking(staking::Call),
	}
);

/// Interface of the runtime that will be available to each module.
pub trait ModuleRuntime {
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

/// A runtime that assumes multiple concurrent instances of itself are existing within threads. All
/// storage operations are checked to be correct. Only storage keys that belong to `self.id` can be
/// accessed. Upon successful access, the key is tainted.
///
/// This should be used within the worker threads.
///
/// This will cache writes and only applies them to storage at the very end.
pub struct ConcurrentRuntime {
	/// The state pointer.
	state: Arc<RuntimeState>,
	/// Thread local state cache.
	cache: RefCell<HashMap<Key, Value>>,
	/// Id of the thread.
	id: ThreadId,
}

impl DispatchPermission for ConcurrentRuntime {}

impl ConcurrentRuntime {
	/// Create a new runtime.
	pub fn new(state: Arc<RuntimeState>, id: ThreadId) -> Self {
		Self {
			state,
			id,
			cache: HashMap::new().into(),
		}
	}

	/// Dispatch a call.
	///
	/// Note that this will use a fresh new cache for the dispatch, and then
	pub fn dispatch(&self, call: OuterCall, origin: AccountId) -> RuntimeDispatchResult {
		// the cache must always be empty at the beginning of a dispatch.
		debug_assert_eq!(self.cache.borrow().keys().len(), 0);

		log!(trace, "worker runtime executing {:?}. ", call);
		// execute
		let dispatch_result =
			<OuterCall as Dispatchable<Self>>::dispatch::<Self>(call, self, origin);

		log!(
			trace,
			"result is {:?}. Cached writes {}.",
			dispatch_result,
			self.cache.borrow().len()
		);

		// only commit if result is ok. Note that logic error will also ignore all writes.
		match dispatch_result {
			Ok(_) => self.commit_cache(),
			_ => (),
		};

		// clear the cache anyhow.
		self.cache.borrow_mut().clear();
		dispatch_result.to_runtime_dispatch_result()
	}

	/// commit the cache to the persistent state.
	pub fn commit_cache(&self) {
		self.cache.borrow().iter().for_each(|(k, v)| {
			debug_assert_eq!(self.state.unsafe_read_taint(k).unwrap(), self.id);
			self.state
				.unsafe_insert(k, state::StateEntry::new(v.to_owned(), self.id));
		});
	}

	/// Validate a call.
	pub fn validate(&self, call: &OuterCall, origin: AccountId) -> ValidationResult {
		<OuterCall as Dispatchable<Self>>::validate(call, self, origin)
	}
}

impl ModuleRuntime for ConcurrentRuntime {
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

/// A runtime that assumes it is being used in a sequential manner. All storage operations are done
/// in an unsafe manner, i.e. not taint values are checked.
///
/// This should be used within the master threads for orphan execution, or for a sequential
/// execution (and, of course for testing where you don't care about tainting).
#[derive(Debug, Default)]
pub struct SequentialRuntime {
	/// The state.
	///
	/// Note that this is still an Arc because the runtime might be shared with another concurrent
	/// runtime. This is perfectly fine as long as the owner of the two guarantees that the two
	/// states are used exclusively.
	pub state: Arc<RuntimeState>,
	/// The thread id.
	pub id: ThreadId,
}

impl DispatchPermission for SequentialRuntime {}

impl SequentialRuntime {
	/// Create new master runtime.
	pub fn new(state: Arc<RuntimeState>, id: ThreadId) -> Self {
		Self { state, id }
	}

	/// Dispatch a call.
	pub fn dispatch(&self, call: OuterCall, origin: AccountId) -> RuntimeDispatchResult {
		<OuterCall as Dispatchable<Self>>::dispatch::<Self>(call, self, origin)
			.to_runtime_dispatch_result()
	}

	/// Validate a call.
	pub fn validate(&self, call: &OuterCall, origin: AccountId) -> ValidationResult {
		<OuterCall as Dispatchable<Self>>::validate(call, self, origin)
	}
}

impl ModuleRuntime for SequentialRuntime {
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
mod concurrent_runtime_test {
	use super::*;
	use primitives::*;
	use std::sync::Arc;

	#[test]
	fn basic_concurrent_runtime_works() {
		let state = RuntimeState::new().as_arc();
		let k1: StateKey = vec![1u8].into();
		let rt = ConcurrentRuntime::new(state, 1);

		let r: Vec<u8> = rt.read(&k1).unwrap().0;
		assert_eq!(r, vec![]);

		assert!(rt.write(&k1, vec![1, 2, 3].into()).is_ok());
		assert_eq!(rt.read(&k1).unwrap().0, vec![1u8, 2, 3]);

		assert!(rt.mutate(&k1, |val| val.0.push(99)).is_ok());
		assert_eq!(rt.read(&k1).unwrap().0, vec![1u8, 2, 3, 99]);
	}

	#[test]
	fn concurrent_runtime_fails_if_tainted() {
		let state = RuntimeState::new().as_arc();
		let k1: StateKey = vec![1u8].into();
		let rt = ConcurrentRuntime::new(Arc::clone(&state), 1);

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
			let runtime = ConcurrentRuntime::new(state_ptr, 1);
			let tx =
				OuterCall::Balances(balances::Call::Transfer(testing::random().public(), 1000));
			assert!(runtime
				.dispatch(tx.clone(), testing::random().public())
				.is_ok());
			assert!(!runtime.validate(&tx, testing::random().public()).is_empty());
		});

		let state_ptr = Arc::clone(&state);
		std::thread::spawn(move || {
			let runtime = ConcurrentRuntime::new(state_ptr, 2);
			let tx =
				OuterCall::Balances(balances::Call::Transfer(testing::random().public(), 1000));
			assert!(runtime
				.dispatch(tx.clone(), testing::random().public())
				.is_ok());
			assert!(!runtime.validate(&tx, testing::random().public()).is_empty());
		});
	}

	#[test]
	fn concurrent_runtime_caching_works() {
		let state = RuntimeState::new().as_arc();
		let k1: StateKey = vec![1u8].into();
		let k2: StateKey = vec![2u8].into();
		let rt = ConcurrentRuntime::new(Arc::clone(&state), 1);

		assert!(rt.read(&k1).is_ok());
		assert!(rt.read(&k2).is_ok());

		// nothing is cached.
		assert_eq!(rt.cache.borrow().len(), 0);

		assert!(rt.write(&k1, vec![1].into()).is_ok());

		// something is cached.
		assert_eq!(rt.cache.borrow().len(), 1);

		// it is not written to state
		assert_eq!(state.read(&k1, 1).unwrap(), vec![].into());

		// but reading through runtime works fine.
		assert_eq!(rt.read(&k1).unwrap(), vec![1].into());

		// commit
		rt.commit_cache();

		// now it is also in state
		assert_eq!(state.read(&k1, 1).unwrap(), vec![1].into());
	}
}

#[cfg(test)]
mod sequential_runtime_test {
	use super::*;

	#[test]
	fn basic_sequential_runtime_works() {
		let state = RuntimeState::new().as_arc();
		let k1: StateKey = vec![1u8].into();
		let rt = SequentialRuntime::new(state, 1);

		let r: Vec<u8> = rt.read(&k1).unwrap().0;
		assert_eq!(r, vec![]);

		assert!(rt.write(&k1, vec![1, 2, 3].into()).is_ok());
		assert_eq!(rt.read(&k1).unwrap().0, vec![1u8, 2, 3]);

		assert!(rt.mutate(&k1, |val| val.0.push(99)).is_ok());
		assert_eq!(rt.read(&k1).unwrap().0, vec![1u8, 2, 3, 99]);
	}

	#[test]
	fn sequential_runtime_works_regardless_of_taint() {
		let state = RuntimeState::new().as_arc();
		let k1: StateKey = vec![1u8].into();
		let rt = SequentialRuntime::new(Arc::clone(&state), 1);

		// current runtime is 1, taint with 2.
		state.unsafe_insert(&k1, state::StateEntry::new_taint(2));

		assert!(rt.read(&k1).is_ok());
		assert!(rt.write(&k1, vec![1, 2, 3].into()).is_ok());
		assert!(rt.mutate(&k1, |val| val.0.push(99)).is_ok());
	}
}
