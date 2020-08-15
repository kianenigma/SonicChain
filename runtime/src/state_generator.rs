use crate::{SequentialRuntime, StateMap};
use primitives::{StateKey, StateValue};

/// A simple builder struct for state.
///
/// This uses an internal sequential runtime to insert some values into a state, and return it upon
/// build.
pub struct InitialStateGenerate {
	runtime: SequentialRuntime,
}

impl InitialStateGenerate {
	pub fn new() -> Self {
		Self {
			runtime: SequentialRuntime::new(Default::default(), 999),
		}
	}

	pub fn with_runtime(self, update: impl FnOnce(&SequentialRuntime) -> ()) -> Self {
		update(&self.runtime);
		self
	}

	pub fn insert(self, key: StateKey, value: StateValue) -> Self {
		self.runtime.state.unsafe_insert_genesis_value(&key, value);
		self
	}

	pub fn build(self) -> StateMap {
		self.runtime.state.dump()
	}
}
