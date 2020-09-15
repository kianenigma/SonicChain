#![feature(thread_id_value)]
#![feature(debug_non_exhaustive)]

pub mod concurrent;
pub mod pool;
pub mod sequential;
pub mod types;

use logging::log;
use pool::VecPool;
use runtime::StateMap;
use state::StateEq;
use types::{Block, Transaction};

const LOG_TARGET: &'static str = "exec";

/// The final state type of the application.
pub type State = runtime::RuntimeState;
/// The final pool type of the application.
pub type Pool = VecPool<Transaction>;

/// Something that can execute transaction, blocks etc.
pub trait Executor {
	/// Execute the given block.
	///
	/// The output is the final state after the execution.
	fn author_block(&mut self, initial_transactions: Vec<Transaction>) -> (StateMap, Block);

	/// Re-validate a block as it will be done by the validator.
	fn validate_block(&mut self, block: Block) -> StateMap;

	/// Clean the internal state of the executor, whatever it may be.
	fn clean(&mut self);

	fn apply_state(&mut self, state: StateMap);

	/// Author and validate a block.
	///
	/// Most often used for testing, otherwise you'd probably want to do one and then time the
	/// execution separately.
	fn author_and_validate(
		&mut self,
		initial_transactions: Vec<Transaction>,
		initial_state: Option<StateMap>,
	) -> bool {
		if let Some(state) = initial_state.clone() {
			log!(
				debug,
				"Applying an initial state with {} keys for authoring.",
				state.len()
			);
			self.apply_state(state)
		}
		let start = std::time::Instant::now();
		let (authoring_state, block) = self.author_block(initial_transactions);
		log!(warn, "authoring took {:?}", start.elapsed());
		self.clean();

		// apply the initial state again.
		if let Some(state) = initial_state {
			log!(
				debug,
				"Applying an initial state with {} keys for validation.",
				state.len()
			);
			self.apply_state(state)
		}
		let validation_state = self.validate_block(block);
		validation_state.state_eq(authoring_state)
	}
}
