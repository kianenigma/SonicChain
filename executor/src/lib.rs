#![feature(thread_id_value)]
#![feature(debug_non_exhaustive)]

pub mod concurrent;
pub mod logging;
pub mod pool;
mod test;
pub mod types;

use pool::VecPool;
use runtime::StateMap;
use state::StateEq;
use types::Block;
use types::Transaction;

// FIXME: some means of easily annotating the transaction.
// FIXME: Start thinking about test scenarios.
// FIXME: add algorithm to do something based on the static annotations.
// FIXME: new module to generate random transactions.
// TODO: A bank example with orphans?

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

	/// Author and validate a block.
	///
	/// Most often used for testing, otherwise you'd probably want to do one and then time the
	/// execution separately.
	fn author_and_validate(&mut self, initial_transactions: Vec<Transaction>) -> bool {
		let (authoring_state, block) = self.author_block(initial_transactions);
		self.clean();
		let validation_state = self.validate_block(block);
		validation_state.state_eq(authoring_state)
	}
}
