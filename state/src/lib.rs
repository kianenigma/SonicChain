use logging::log;
use std::{
	cell::RefCell,
	collections::hash_map::HashMap,
	fmt::Debug,
	sync::{Arc, RwLock},
};

const LOG_TARGET: &'static str = "state";

// TODO: for now I will sprinkle some manual sleeps here, but later on consider having some fancy
// type like SlowMap that is a hashmap that sleeps upon both read and write.

#[cfg(not(test))]
const READ_DELAY: std::time::Duration = std::time::Duration::from_millis(1);
#[cfg(not(test))]
const WRITE_DELAY: std::time::Duration = std::time::Duration::from_millis(2);

macro_rules! sleep_read {
	() => {
		#[cfg(not(test))]
		std::thread::sleep(READ_DELAY);
	};
}

macro_rules! write_sleep {
	() => {
		#[cfg(not(test))]
		std::thread::sleep(WRITE_DELAY);
	};
}

/// Extension trait to check the equality of two state dumps.
///
/// This can be done by means of state root in case of merklized state, or others if a simpler state
/// implementation is being used.
pub trait StateEq {
	fn state_eq(&self, other: Self) -> bool;
}

impl<K: KeyT, V: ValueT, T: TaintT> StateEq for MapType<K, V, T> {
	fn state_eq(&self, other: Self) -> bool {
		self.iter()
			.all(|(k, v)| other.get(k).map(|vv| v.data == vv.data).unwrap_or(false))
			&& self.keys().len() == other.keys().len()
	}
}

/// Public interface of a state database. It could in principle use any backend.
pub trait GenericState<K, V, T> {
	/// Read the state entry at `key`.
	///
	/// - If the key does not exist, it will try and taint it, and return `Ok(Default)`.
	/// 	- This will first create a read lock, then a write lock.
	/// - If the key exists, and the taint is equal to `current`, then Ok(value) is returned.
	/// 	- This will only require read locks.
	/// - If the key exists, and the taint is **not** equal to `current`, then `Err(owner)` is
	///   returned.
	/// 	- This will only require read locks.
	fn read(&self, key: &K, current: T) -> Result<V, T>;

	/// Write to the state entry at `key`.
	///
	/// - If the key does not exist, it will try and taint it, then write `value` into it and return
	///   `Ok(())`.
	/// 	- This will first create a read lock, then a write lock.
	/// - If the key exists, and the taint is equal to `current`, then the `value` is written and
	///   Ok(()) is returned.
	/// 	- This will only require read locks.
	/// - If the key exists, and the taint is **not** equal to `current`, then `Err(owner)` is
	///   returned.
	/// 	- This will only require read locks.
	fn write(&self, key: &K, value: V, current: T) -> Result<(), T>;

	/// A combination of read and write, in place. Just a syntactic sugar, not really optimized.
	fn mutate(&self, key: &K, update: impl Fn(&mut V) -> (), current: T) -> Result<(), T>
	where
		K: KeyT,
		V: ValueT,
		T: TaintT,
	{
		self.read(key, current).and_then(|mut val| {
			(update)(&mut val);
			self.write(key, val, current)
		})
	}
}

/// The inner HashMap type.
pub type MapType<K, V, T> = HashMap<K, StateEntry<V, T>>;

pub trait KeyT: Clone + Debug + std::hash::Hash + Eq + PartialEq {}
impl<T: Clone + Debug + std::hash::Hash + Eq + PartialEq> KeyT for T {}

pub trait ValueT: Clone + Debug + Default + Eq + PartialEq {}
impl<T: Clone + Debug + Default + Eq + PartialEq> ValueT for T {}

pub trait TaintT: Clone + Copy + Debug + Eq + PartialEq {}
impl<T: Clone + Copy + Debug + Eq + PartialEq> TaintT for T {}

#[derive(Default, Debug, Clone)]
pub struct StateEntry<V, T> {
	data: RefCell<V>,
	taint: Option<T>,
}

impl<V: ValueT, T> PartialEq for StateEntry<V, T> {
	fn eq(&self, other: &Self) -> bool {
		self.data.eq(&other.data)
	}
}

impl<V: ValueT, T> Eq for StateEntry<V, T> {}

// or just use atomic RefCell.
unsafe impl<V, T> Sync for StateEntry<V, T> {}

impl<V: ValueT, T: TaintT> StateEntry<V, T> {
	pub fn data(&self) -> V {
		self.data.borrow().clone()
	}

	pub fn new(value: V, taint: T) -> Self {
		Self {
			data: value.into(),
			taint: Some(taint),
		}
	}

	pub fn new_taint(taint: T) -> Self {
		Self {
			taint: Some(taint),
			data: Default::default(),
		}
	}

	pub fn new_data(value: V) -> Self {
		Self {
			data: value.into(),
			taint: None,
		}
	}
}

/// A struct that implements `GenericState`.
///
/// This implements the taintable struct. Each access will try and taint that state key. Any further
/// access from other threads will not be allowed.
///
/// This is a highly concurrent implementation. Locking is scarce.
#[derive(Debug, Default)]
pub struct TaintState<K: KeyT, V: ValueT, T: TaintT> {
	backend: RwLock<MapType<K, V, T>>,
}

impl<K: KeyT, V: ValueT, T: TaintT> TaintState<K, V, T> {
	/// Create a new `TaintState`.
	pub fn new() -> Self {
		Self {
			backend: RwLock::new(MapType::default()),
		}
	}

	/// Consume self and return it wrapped in an `Arc`.
	pub fn as_arc(self) -> Arc<Self> {
		std::sync::Arc::new(self)
	}

	/// Create self with given capacity.
	pub fn with_capacity(capacity: usize) -> Self {
		Self {
			backend: RwLock::new(MapType::with_capacity(capacity)),
		}
	}

	/// Unsafe implementation of insert. This will not respect the tainting of the key.
	pub fn unsafe_insert(&self, at: &K, value: StateEntry<V, T>) {
		write_sleep!();
		self.backend.write().unwrap().insert(at.clone(), value);
	}

	/// Unsafe insert of a value, wiping away the taint value.
	pub fn unsafe_insert_genesis_value(&self, at: &K, value: V) {
		write_sleep!();
		logging::log!(trace, "inserting genesis value at {:?} => {:?}", at, value);
		self.backend
			.write()
			.unwrap()
			.insert(at.clone(), StateEntry::new_data(value));
	}

	/// Return a dump of the state at this point in time as a HashMap.
	///
	/// Note that this copied all the data to na new hashmap and hence is an expensive operation.
	pub fn dump(&self) -> MapType<K, V, T> {
		self.backend
			.read()
			.map(|g| g.clone())
			.expect("dumping state should work")
	}

	/// insert all the keys of the given state into self.
	///
	/// Infallible.
	#[deprecated = "This is not used as far as I can see"]
	pub fn unsafe_duplicate(&self, initial: Self) {
		initial
			.dump()
			.into_iter()
			.for_each(|(k, v)| self.unsafe_insert(&k, v))
	}

	/// Clear the inner map.
	pub fn unsafe_clean(&self) {
		write_sleep!();
		self.backend.write().unwrap().clear();
	}

	/// Count all the keys in the state
	pub fn unsafe_len(&self) -> usize {
		self.backend.read().unwrap().len()
	}

	/// Print the entire state's info.

	/// Unsafe implementation of read. This will not respect the tainting of the key.
	pub fn unsafe_read_value(&self, key: &K) -> Option<V> {
		self.unsafe_read(key).map(|e| e.data.clone().into_inner())
	}

	/// Unsafe implementation of read. This will not respect the tainting of the key.
	pub fn unsafe_read_taint(&self, key: &K) -> Option<T> {
		self.unsafe_read(key).and_then(|e| e.taint)
	}

	/// Unsafe implementation of read. This will not respect the tainting of the key.
	fn unsafe_read(&self, key: &K) -> Option<StateEntry<V, T>> {
		sleep_read!();
		self.backend.read().unwrap().get(key).cloned()
	}
}

impl<K: KeyT, V: ValueT, T: TaintT> GenericState<K, V, T> for TaintState<K, V, T> {
	fn read(&self, key: &K, current: T) -> Result<V, T> {
		sleep_read!();
		let read_guard = self.backend.read().unwrap();

		let outcome = if let Some(entry) = read_guard.get(key) {
			let maybe_owner = entry.taint;

			if let Some(owner) = maybe_owner {
				// 1. if entry exists and it has a taint.
				if owner == current {
					Ok(entry.data.borrow().clone())
				} else {
					Err(owner)
				}
			} else {
				// 2. the entry exists but it has no taint.
				drop(read_guard);
				let mut write_guard = self.backend.write().unwrap();
				let entry = write_guard
					.get_mut(key)
					.expect("Entry has been already checked to exist.");
				if let Some(owner) = entry.taint {
					// rare case: someone tainted in the meantime. that bastard someone must be some
					// other thread.
					if owner == current {
						panic!("Current thread cannot be the owner.");
					} else {
						Err(owner)
					}
				} else {
					entry.taint = Some(current);
					Ok(entry.data.borrow().clone())
				}
			}
		} else {
			// 3. the entry does not exists.
			drop(read_guard);
			let mut write_guard = self.backend.write().unwrap();
			if let Some(entry) = write_guard.get(key) {
				// rare case: someone tainted/created in the meantime. that bastard someone must be
				// some other thread.
				let owner = entry
					.taint
					.expect("Newly created entry at runtime MUST have a taint.");
				if owner == current {
					panic!("Current thread cannot be the owner.");
				} else {
					Err(owner)
				}
			} else {
				// we have the write lock and the entry does not have taint. Taint and read.
				let new_entry = <StateEntry<V, T>>::new_taint(current);
				write_guard.insert(key.clone(), new_entry);
				Ok(Default::default())
			}
		};

		log!(trace, "reading {:?} => {:?}", key, outcome);
		outcome
	}

	fn write(&self, key: &K, value: V, current: T) -> Result<(), T> {
		write_sleep!();
		let read_guard = self.backend.read().unwrap();
		let outcome = if let Some(entry) = read_guard.get(key) {
			if let Some(owner) = entry.taint {
				// 1. if entry exists and it has a taint.
				if owner == current {
					*entry.data.borrow_mut() = value;
					Ok(())
				} else {
					Err(owner)
				}
			} else {
				// 2. the entry exists but it has no taint.
				drop(read_guard);
				let mut write_guard = self.backend.write().unwrap();
				let entry = write_guard
					.get_mut(key)
					.expect("Entry has been already checked to exist.");
				if let Some(owner) = entry.taint {
					// rare case: someone tainted in the meantime. that bastard someone must be some
					// other thread.
					if owner == current {
						panic!("Current thread cannot be the owner.");
					} else {
						Err(owner)
					}
				} else {
					// we have the write lock and the entry does not have taint. Taint and write.
					*entry.data.borrow_mut() = value;
					entry.taint = Some(current);
					Ok(())
				}
			}
		} else {
			// 3. the entry does not exists.
			drop(read_guard);
			let mut write_guard = self.backend.write().unwrap();
			if let Some(entry) = write_guard.get(key) {
				let owner = entry
					.taint
					.expect("Newly created entry at runtime MUST have a taint.");
				if owner == current {
					panic!("Current thread cannot be the owner.");
				} else {
					Err(owner)
				}
			} else {
				let new_entry = <StateEntry<V, T>>::new(value, current);
				write_guard.insert(key.clone(), new_entry);
				Ok(())
			}
		};

		log!(trace, "writing {:?} => {:?}", key, outcome);
		outcome
	}
}

#[cfg(test)]
mod test_state {
	use super::*;
	use std::{sync::Arc, thread};

	type Key = u32;
	type Value = u32;
	type ThreadId = u8;

	type TestState = TaintState<Key, Value, ThreadId>;

	#[test]
	fn basic_state_works() {
		let state = TaintState::new();
		state.unsafe_insert(&33, StateEntry::new("Foo", "Thread1"));
		assert_eq!(state.unsafe_read_value(&33).unwrap(), "Foo");
		assert_eq!(state.unsafe_read_taint(&33).unwrap(), "Thread1");
	}

	#[test]
	fn basic_read_write_ops() {
		let state = TestState::new();
		assert_eq!(state.read(&10, 1).unwrap(), 0);
		assert!(state.write(&10, 5, 1).is_ok());
		assert_eq!(state.read(&10, 1).unwrap(), 5);
	}

	#[test]
	fn reading_taints() {
		let state = TaintState::<u32, u32, u8>::new();
		assert!(state.read(&10u32, 1u8).is_ok());
		assert_eq!(state.unsafe_read_taint(&10).unwrap(), 1);
	}

	#[test]
	fn writing_taints() {
		let state = TaintState::new();
		assert!(state.write(&10u32, 5u32, 1u8).is_ok());
		assert_eq!(state.unsafe_read_taint(&10).unwrap(), 1);
	}

	#[test]
	fn cannot_read_from_tainted() {
		let state = TaintState::new();
		assert!(state.write(&10u32, 5u32, 1u8).is_ok());
		// thread 2 cannot read from 10 anymore.
		assert!(state.read(&10, 2).is_err());
	}

	#[test]
	fn can_share_state_between_threads() {
		let state = TestState::new().as_arc();

		let h1 = {
			let state = Arc::clone(&state);
			thread::spawn(move || state.write(&10, 10, 1))
		};

		let h2 = {
			let state = Arc::clone(&state);
			thread::spawn(move || state.write(&11, 11, 2))
		};

		assert!(h1.join().is_ok());
		assert!(h2.join().is_ok());

		assert_eq!(state.unsafe_read_value(&10).unwrap(), 10);
		assert_eq!(state.unsafe_read_value(&11).unwrap(), 11);
	}

	#[test]
	fn mutate_works() {
		let state = TestState::new().as_arc();

		assert!(state
			.mutate(
				&10,
				|old| {
					assert_eq!(*old, Value::default());
					*old = 5;
				},
				1
			)
			.is_ok());

		assert!(state
			.mutate(
				&10,
				|old| {
					assert_eq!(*old, 5);
					*old = 6;
				},
				1
			)
			.is_ok());

		state.unsafe_insert(&11, StateEntry::new(11, 2));

		assert!(state
			.mutate(
				&11,
				|_| {
					// closure will never be executed.
					assert!(false);
				},
				1
			)
			.is_err());
	}

	#[test]
	fn only_one_thread_can_taint_read() {
		let state = TestState::new().as_arc();
		let num_threads = 12;

		let handles: Vec<std::thread::JoinHandle<Result<Value, ThreadId>>> = (1..=num_threads)
			.map(|id| {
				let state = Arc::clone(&state);
				thread::spawn(move || state.read(&999, id))
			})
			.collect();

		let results: Vec<Result<Value, ThreadId>> =
			handles.into_iter().map(|h| h.join().unwrap()).collect();
		assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
		assert_eq!(
			results.iter().filter(|r| r.is_err()).count(),
			(num_threads - 1) as usize
		);
	}

	#[test]
	fn can_have_genesis_values() {
		let state = TestState::new();
		state.unsafe_insert(&10u32, StateEntry::new_data(10u32));

		assert_eq!(state.unsafe_read_taint(&10u32), None);
		assert_eq!(state.unsafe_read_value(&10u32), Some(10u32));

		assert_eq!(state.unsafe_read_taint(&11u32), None);
		assert_eq!(state.unsafe_read_value(&11u32), None);
	}

	#[test]
	fn taints_upon_first_read_of_genesis_values() {
		let state = TestState::new();
		state.unsafe_insert(&10u32, StateEntry::new_data(10u32));

		assert_eq!(state.unsafe_read_taint(&10u32), None);
		assert_eq!(state.unsafe_read_value(&10u32), Some(10u32));

		state.read(&10u32, 1u8).unwrap();

		assert_eq!(state.unsafe_read_taint(&10u32), Some(1));
		assert_eq!(state.unsafe_read_value(&10u32), Some(10u32));
	}

	#[test]
	fn taints_upon_first_write_of_genesis_values() {
		let state = TestState::new();
		state.unsafe_insert(&10u32, StateEntry::new_data(10u32));

		assert_eq!(state.unsafe_read_taint(&10u32), None);
		assert_eq!(state.unsafe_read_value(&10u32), Some(10u32));

		state.write(&10u32, 99u32, 1u8).unwrap();

		assert_eq!(state.unsafe_read_taint(&10u32), Some(1));
		assert_eq!(state.unsafe_read_value(&10u32), Some(99u32));
	}

	#[test]
	fn duplicate_works() {
		let initial = TestState::new();
		initial.unsafe_insert(&10u32, StateEntry::new_data(10u32));
		initial.unsafe_insert(&11u32, StateEntry::new(11, 1));

		let state = TestState::new();
		state.unsafe_duplicate(initial);

		let dump = state.dump();
		assert_eq!(
			*dump.get(&10).unwrap(),
			StateEntry {
				data: 10u32.into(),
				taint: None,
			}
		);
		assert_eq!(
			*dump.get(&11).unwrap(),
			StateEntry {
				data: 11u32.into(),
				taint: Some(1),
			}
		);
	}

	#[test]
	fn dump_works() {
		let state = TestState::new();
		state.unsafe_insert(&10u32, StateEntry::new_data(10u32));
		state.unsafe_insert(&11u32, StateEntry::new(11, 1));

		let dump = state.dump();
		assert_eq!(
			*dump.get(&10).unwrap(),
			StateEntry {
				data: 10u32.into(),
				taint: None,
			}
		);
		assert_eq!(
			*dump.get(&11).unwrap(),
			StateEntry {
				data: 11u32.into(),
				taint: Some(1),
			}
		);
	}

	#[test]
	fn clean_works() {
		let state = TestState::new();
		state.unsafe_insert(&10u32, StateEntry::new_data(10u32));
		state.unsafe_insert(&11u32, StateEntry::new(11, 1));

		assert_eq!(
			state.unsafe_read(&10).unwrap(),
			StateEntry {
				data: 10u32.into(),
				taint: None,
			}
		);
		assert_eq!(
			state.unsafe_read(&11).unwrap(),
			StateEntry {
				data: 11u32.into(),
				taint: Some(1),
			}
		);

		state.unsafe_clean();
		assert!(state.unsafe_read(&10).is_none());
		assert!(state.unsafe_read(&11).is_none());
	}

	#[test]
	fn state_eq_works() {
		let state1 = TestState::new();
		state1.unsafe_insert(&10u32, StateEntry::new_data(10u32));
		state1.unsafe_insert(&11u32, StateEntry::new(11, 1));
		let dump1 = state1.dump();

		let state2 = TestState::new();
		state2.unsafe_insert(&10u32, StateEntry::new_data(10u32));
		state2.unsafe_insert(&11u32, StateEntry::new(11, 1));
		let dump2 = state2.dump();

		assert!(dump1.state_eq(dump2));

		let state3 = TestState::new();
		state3.unsafe_insert(&10u32, StateEntry::new_data(11u32));
		state3.unsafe_insert(&11u32, StateEntry::new(11, 1));
		let dump3 = state3.dump();

		assert!(!dump1.state_eq(dump3));
	}

	#[test]
	fn state_eq_ignores_taint() {
		let state1 = TestState::new();
		state1.unsafe_insert(&10u32, StateEntry::new_data(10u32));
		state1.unsafe_insert(&11u32, StateEntry::new(11, 1));
		let dump1 = state1.dump();

		let state2 = TestState::new();
		state2.unsafe_insert(&10u32, StateEntry::new_data(10u32));
		state2.unsafe_insert(&11u32, StateEntry::new(11, 2));
		let dump2 = state2.dump();

		assert!(dump1.state_eq(dump2));
	}
}
