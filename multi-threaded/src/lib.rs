#![feature(thread_id_value)]
#![feature(debug_non_exhaustive)]

use primitives::*;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;

pub mod concurrent;
pub mod logging;
pub mod pool;
mod test;
pub mod types;

use concurrent::{master, tx_distribution, worker};
use master::*;
use pool::*;
use state::StateEq;
use tx_distribution::*;
use types::*;
use worker::*;

/// The final state type of the application.
pub type State = runtime::RuntimeState;
/// The final pool type of the application.
pub type Pool = VecPool<Transaction>;
/// The inner hash map used in state.
pub type StateMap = state::MapType<Key, Value, ThreadId>;

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
	fn author_and_validate(&mut self, initial_transactions: Vec<Transaction>) -> bool {
		let (authoring_state, block) = self.author_block(initial_transactions);
		self.clean();
		let validation_state = self.validate_block(block);
		validation_state.state_eq(authoring_state)
	}
}

#[derive(Debug)]
pub struct ConcurrentExecutor<P: TransactionPool<Transaction>, D: Distributer> {
	master: Master<P, D>,
}

impl<P: TransactionPool<Transaction>, D: Distributer> ConcurrentExecutor<P, D> {
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
					match worker.from_master.recv().unwrap().payload {
						MessagePayload::FinalizeSetup(data) => worker.to_others = data,
						_ => panic!("Received unexpected message"),
					};

					log::info!("Worker initialized. Parking self.");

					// master will initially park the thread in initialization, and unpark it once done.
					thread::park();

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

		master.workers.iter().for_each(|(_, handle)| {
			let message = MessagePayload::FinalizeSetup(to_workers.clone()).into();
			handle
				.send
				.send(message)
				.expect("Can always send FinalizeSetup; qed");
		});

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

		// dump the state
		let state = self.master.state.dump();
		let block = Block::from(self.master.tx_pool.all());

		(state, block)
	}

	fn clean(&mut self) {
		self.master.state.unsafe_clean();
	}

	fn validate_block(&mut self, _block: Block) -> StateMap {
		let _buckets: BTreeMap<ThreadId, Vec<Transaction>> = Default::default();
		unimplemented!()
	}
}

// FIXME: some means of easily annotating the transaction.
// FIXME: Start thinking about test scenarios.
// FIXME: add algorithm to do something based on the static annotations.
// FIXME: new module to generate random transactions.
// TODO: A bank example with orphans?
