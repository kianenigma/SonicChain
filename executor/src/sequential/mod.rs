use crate::State;
use crate::*;
use runtime::*;
use types::*;

const LOG_TARGET: &'static str = "seq-exec";

/// A sequential executor.
///
/// This is orders of magnitude simpler thant the concurrent counter part. There is no notion of
/// master or worker. All state operations are done in an unsafe manner, assuming that there will be
/// no other concurrent thread.
pub struct SequentialExecutor {
	pub runtime: MasterRuntime,
}

impl SequentialExecutor {
	pub fn new() -> Self {
		let id = std::thread::current().id().as_u64().into();
		let state = State::new().as_arc();
		let runtime = MasterRuntime::new(state, id);
		Self { runtime }
	}

	fn apply_transaction(&self, transactions: Vec<Transaction>) {
		for tx in transactions {
			log!(debug, "applying transaction {:?}", tx);
			let call = tx.function;
			let origin = tx.signature.0;
			let ok = self.runtime.dispatch(call, origin)
				.expect("Sequential execution cannot fail on execute. This will at most be Ok(LogicError(..))");
			log!(debug, "Result of last apply: {:?}", ok);
		}
	}
}

impl Executor for SequentialExecutor {
	fn author_block(&mut self, initial_transactions: Vec<Transaction>) -> (StateMap, Block) {
		log!(
			info,
			"Authoring block with {} transactions.",
			initial_transactions.len(),
		);
		// simply apply the transactions, ony by fucking one.
		self.apply_transaction(initial_transactions.clone());
		(self.runtime.state.dump(), initial_transactions.into())
	}

	fn validate_block(&mut self, block: Block) -> StateMap {
		log!(
			info,
			"Validating block with {} transactions. ",
			block.transactions.len(),
		);
		self.apply_transaction(block.transactions);
		self.runtime.state.dump()
	}

	fn clean(&mut self) {
		self.runtime.state.unsafe_clean();
	}

	fn apply_state(&mut self, state: StateMap) {
		for (k, v) in state.into_iter() {
			self.runtime.state.unsafe_insert(&k, v);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use parity_scale_codec::Encode;
	use primitives::*;
	use runtime::balances::*;

	#[test]
	fn can_author_block() {
		let mut executor = SequentialExecutor::new();
		let transactions = transaction_generator::simple_alice_bob_dave();

		transaction_generator::endow_account(testing::alice().public(), &executor.runtime, 100);

		let (state, block) = executor.author_block(transactions);
		assert_eq!(block.transactions.len(), 2);
		assert_eq!(
			state
				.get(&<BalanceOf<MasterRuntime>>::key_for(
					testing::alice().public()
				))
				.unwrap()
				.data(),
			AccountBalance::from(80).encode().into(),
		);
		assert_eq!(
			state
				.get(&<BalanceOf<MasterRuntime>>::key_for(
					testing::bob().public()
				))
				.unwrap()
				.data(),
			AccountBalance::from(10).encode().into(),
		);
		assert_eq!(
			state
				.get(&<BalanceOf<MasterRuntime>>::key_for(
					testing::dave().public()
				))
				.unwrap()
				.data(),
			AccountBalance::from(10).encode().into(),
		);
	}

	#[test]
	fn can_validate_block() {
		let mut executor = SequentialExecutor::new();
		let transactions = transaction_generator::simple_alice_bob_dave();
		// TODO: maybe introduce initial_state to all of the functions in executor.. that would be
		// more uniform but honestly more hassle to setup.
		transaction_generator::endow_account(testing::alice().public(), &executor.runtime, 100);

		let (_, block) = executor.author_block(transactions);
		executor.clean();

		transaction_generator::endow_account(testing::alice().public(), &executor.runtime, 100);
		let validation_state = executor.validate_block(block);
		assert_eq!(
			validation_state
				.get(&<BalanceOf<MasterRuntime>>::key_for(
					testing::alice().public()
				))
				.unwrap()
				.data(),
			AccountBalance::from(80).encode().into(),
		);
		assert_eq!(
			validation_state
				.get(&<BalanceOf<MasterRuntime>>::key_for(
					testing::bob().public()
				))
				.unwrap()
				.data(),
			AccountBalance::from(10).encode().into(),
		);
		assert_eq!(
			validation_state
				.get(&<BalanceOf<MasterRuntime>>::key_for(
					testing::dave().public()
				))
				.unwrap()
				.data(),
			AccountBalance::from(10).encode().into(),
		);
	}

	#[test]
	fn can_author_and_validate_block() {
		logging::init_logger();
		let mut executor = SequentialExecutor::new();
		let transactions = transaction_generator::simple_alice_bob_dave();

		// TODO: this is simply too much hassle to setup. We need a bloody macro or something for
		// this to easily setup mock states.
		let initial_state = State::new();
		let initial_state_rt = MasterRuntime::new(initial_state.as_arc(), 999);
		transaction_generator::endow_account(testing::alice().public(), &initial_state_rt, 100);

		assert!(executor.author_and_validate(transactions, Some(initial_state_rt.state.dump())));
	}
}
