pub mod master;
pub mod tx_distribution;
pub mod worker;

use std::{
	collections::BTreeMap,
	sync::{
		mpsc::{channel, Sender},
		Arc,
	},
};

use crate::{pool::*, types::*, Executor, State, StateMap};
use master::*;
use primitives::*;
use std::thread;
use tx_distribution::*;
use worker::*;

/// A concurrent executor.
#[derive(Debug)]
pub struct ConcurrentExecutor<P: TransactionPool<Transaction>, D: Distributer> {
	pub master: Master<P, D>,
}

impl<P: TransactionPool<Transaction>, D: Distributer> ConcurrentExecutor<P, D> {
	/// Sets up a new concurrent executor.
	///
	/// This is basically a wrapper around a master struct with some utility function.
	///
	/// OIt is worth noting that this function defines the initial code of each worker thread. The
	/// initial code is as such: The worker thread will be created, it will wait for a mandatory
	/// initial message from the master, and then call either `run` or `test_run`. The main `run`
	/// method of the worker will just sleep. See `Worker` for more info.
	pub fn new(threads: usize, test_run: bool, initial_state: Option<State>) -> Self {
		// One queue for all workers to send to master.
		let (workers_to_master_tx, workers_to_master_rx) = channel();

		let mut master = Master::new_from_thread(workers_to_master_rx, initial_state);

		let mut to_workers: BTreeMap<ThreadId, Sender<Message>> = Default::default();

		for i in 0..threads {
			// clone the state and master id.
			let state_ptr = Arc::clone(&master.state);
			let master_id = master.id;

			// one channel for the master to send to this worker.
			let (master_to_worker_tx, master_to_worker_rx) = channel();
			let worker_to_master_tx = Clone::clone(&workers_to_master_tx);

			// one channel for other workers to send to this worker.
			let (from_others_tx, from_others_rx) = channel();

			let worker_handle = thread::Builder::new()
				.name(format!("Worker#{}", i))
				.spawn(move || {
					// note that we are creating this inside a new thread.
					let mut worker = Worker::new_from_thread(
						master_id,
						state_ptr,
						worker_to_master_tx,
						master_to_worker_rx,
						from_others_rx,
					);

					// wait for the master to send you the btree-map of the send queue to all other
					// threads.
					worker.wait_finalize_setup();

					// run
					if test_run {
						worker.test_run();
					} else {
						worker.run();
					}
				})
				.expect("Failed to spawn a new worker thread.");

			let worker_id = worker_handle.thread().id().as_u64().into();
			to_workers.insert(worker_id, from_others_tx);
			let handle = WorkerHandle::new(master_to_worker_tx, worker_handle);
			master.workers.insert(worker_id, handle);
		}

		assert_eq!(to_workers.len(), threads);
		log::info!("created {} worker threads.", threads);

		master
			.broadcast(MessagePayload::FinalizeSetup(to_workers.clone()).into())
			.expect("Broadcast must works");

		Self { master }
	}
}

impl<P: TransactionPool<Transaction>, D: Distributer> Executor for ConcurrentExecutor<P, D> {
	fn author_block(&mut self, initial_transactions: Vec<Transaction>) -> (StateMap, Block) {
		// Validate and all of the transactions to the pool.
		self.master
			.tx_pool
			.push_batch(initial_transactions.as_ref());

		// run.
		self.master.unpark_all();
		self.master.run_author();

		// TODO: later on, we probably want to do this shit elsewhere so we can clearly time ONLY
		// the execution, not this side-process.

		// dump the state
		let state = self.master.state.dump();
		let mut block = Block::from(self.master.tx_pool.all());
		block
			.transactions
			.extend(self.master.orphan_pool.iter().cloned());

		(state, block)
	}

	fn clean(&mut self) {
		self.master.tx_pool.clear();
		self.master.state.unsafe_clean();
		self.master.orphan_pool.clear();
	}

	fn validate_block(&mut self, block: Block) -> StateMap {
		self.master.validate_block(block)
	}

	fn apply_state(&mut self, state: StateMap) {
		for (k, v) in state.into_iter() {
			self.master.state.unsafe_insert(&k, v);
		}
	}
}

#[cfg(test)]
mod concurrent_executor {
	use super::*;
	use crate::{pool::TransactionPool, *};
	use logging::init_logger;
	use primitives::testing::*;
	use runtime::InitialStateGenerate;
	use types::transaction_generator;

	#[test]
	fn concurrent_executor_new_works() {
		// TODO: this test is sometimes flaky:
		// thread 'main' panicked at 'assertion failed: master.join_all().is_ok()', executor/src/concurrent/mod.rs:148:9
		let executor = ConcurrentExecutor::<Pool, RoundRobin>::new(4, true, None);
		let master = executor.master;
		std::thread::sleep(std::time::Duration::from_millis(500));
		assert_eq!(master.workers.len(), 4);

		master.run_test();

		assert!(master.join_all().is_ok())
	}

	#[test]
	fn empty_setup_works_authoring() {
		init_logger();

		let mut executor = ConcurrentExecutor::<Pool, RoundRobin>::new(3, false, None);
		std::thread::sleep(std::time::Duration::from_millis(200));
		assert_eq!(executor.master.workers.len(), 3);

		executor.author_block(vec![]);
		executor.master.run_terminate();
		assert!(executor.master.join_all().is_ok());
	}

	#[test]
	fn empty_setup_validation_authoring() {
		init_logger();

		let mut executor = ConcurrentExecutor::<Pool, RoundRobin>::new(3, false, None);
		std::thread::sleep(std::time::Duration::from_millis(200));
		assert_eq!(executor.master.workers.len(), 3);

		assert!(executor.author_and_validate(vec![], None));

		executor.master.run_terminate();
		assert!(executor.master.join_all().is_ok());
	}

	#[test]
	fn validation_authoring_works_bank() {
		logging::init_logger();
		let mut executor = ConcurrentExecutor::<Pool, RoundRobin>::new(3, false, None);
		let (transactions, accounts) = transaction_generator::bank(5, 20);

		let initial_state = InitialStateGenerate::new()
			.with_runtime(|rt| {
				accounts
					.into_iter()
					.for_each(|acc| transaction_generator::endow_account(acc, rt, 100_000))
			})
			.build();

		assert!(executor.author_and_validate(transactions, Some(initial_state)));
	}

	#[test]
	fn multiple_tasks_works() {
		init_logger();

		let mut executor = ConcurrentExecutor::<Pool, RoundRobin>::new(3, false, None);

		let (txs, accounts) = transaction_generator::bank(5, 20);
		accounts.iter().for_each(|acc| {
			transaction_generator::endow_account(*acc, &executor.master.runtime, 1000_000_000_000)
		});
		std::thread::sleep(std::time::Duration::from_millis(200));
		assert_eq!(executor.master.workers.len(), 3);

		let (state1, block1) = executor.author_block(txs.clone());
		executor.clean();

		// master queue must be empty.
		assert_eq!(executor.master.tx_pool.len(), 0);
		assert_eq!(executor.master.state.unsafe_len(), 0);

		// start another job
		// re-endow the accounts.
		accounts.into_iter().for_each(|acc| {
			types::transaction_generator::endow_account(
				acc,
				&executor.master.runtime,
				1000_000_000_000,
			)
		});
		let (state2, block2) = executor.author_block(txs);
		executor.clean();

		assert_eq!(executor.master.tx_pool.len(), 0);
		assert_eq!(executor.master.state.unsafe_len(), 0);

		assert!(state1.state_eq(state2));
		assert_eq!(block1.transactions.len(), 20);
		assert_eq!(block2.transactions.len(), 20);
	}

	#[test]
	fn orphan_example() {
		init_logger();
		use runtime::balances::*;

		// alice -> bob, will assigned to first worker.
		let tx1 = transaction_generator::build_transfer(100, alice(), bob().public());
		// eve -> dave, will be assigned to second worker.
		let tx2 = transaction_generator::build_transfer(101, eve(), dave().public());
		// bob -> dave, will be assigned to third worker.
		let tx3 = transaction_generator::build_transfer(102, bob(), dave().public());

		let executor = ConcurrentExecutor::<Pool, RoundRobin>::new(3, false, None);
		let mut master = executor.master;
		std::thread::sleep(std::time::Duration::from_millis(200));
		assert_eq!(master.workers.len(), 3);

		// give them some balance.
		transaction_generator::endow_account(alice().public(), &master.runtime, 1000);
		transaction_generator::endow_account(bob().public(), &master.runtime, 1000);
		transaction_generator::endow_account(dave().public(), &master.runtime, 1000);
		transaction_generator::endow_account(eve().public(), &master.runtime, 1000);

		master.tx_pool.push_back(tx1);
		master.tx_pool.push_back(tx2);
		master.tx_pool.push_back(tx3);

		master.run_author();
		master.run_terminate();

		let alice_balance = BalanceOf::read(&master.runtime, alice().public())
			.unwrap()
			.free();
		let bob_balance = BalanceOf::read(&master.runtime, bob().public())
			.unwrap()
			.free();
		let dave_balance = BalanceOf::read(&master.runtime, dave().public())
			.unwrap()
			.free();
		let eve_balance = BalanceOf::read(&master.runtime, eve().public())
			.unwrap()
			.free();

		assert_eq!(alice_balance, 990);
		assert_eq!(eve_balance, 990);
		assert_eq!(bob_balance, 1000);
		assert_eq!(dave_balance, 1020);

		assert!(master.join_all().is_ok());
	}

	macro_rules! bank_test_with_distribution {
		($( $distribution:ty, $name:ident ,)*) => {
			$(
				#[test]
				fn $name() {
					init_logger();
					// NOTE: sanity check would be nice here, but we can't quite do this. We can't
					// compare the output of this to a sequential execution, because it correctly
					// may not be the same. This is indeed re-ordering the transactions. Best that
					// we can do for now is to call author and validate so that we can be more or
					// less sure that at least a concurrent validation will lead to the same result.
					const NUM_ACCOUNTS: usize = 10;
					const NUM_TXS: usize = 1_000;

					let mut executor = ConcurrentExecutor::<Pool, $distribution>::new(4, false, None);
					std::thread::sleep(std::time::Duration::from_millis(500));
					assert_eq!(executor.master.workers.len(), 4);

					let (transfers, accounts) = transaction_generator::bank(NUM_ACCOUNTS, NUM_TXS);

					let initial_state = InitialStateGenerate::new().with_runtime(|rt|
					accounts.iter().for_each(|acc| {
						transaction_generator::endow_account(
							*acc,
							rt,
							1000_000_000_000,
						)
					})).build();


					executor.author_and_validate(transfers, Some(initial_state));

					executor.master.run_terminate();
					assert!(executor.master.join_all().is_ok());
				}
			)*
		}
	}

	bank_test_with_distribution!(RoundRobin, bank_round_robin,);
	bank_test_with_distribution!(ConnectedComponentsDistributer, bank_connected_components,);
}
