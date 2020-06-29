use crate::message::MessagePayload;
use crate::{message::Message, State, Transaction, TransactionStatus};
use primitives::*;
use std::collections::BTreeMap;
use std::sync::{
	mpsc::{Receiver, SendError, Sender},
	Arc,
};
use std::thread::{self, JoinHandle};

/// A handle created for each worker thread.
#[derive(Debug)]
pub struct WorkerHandle {
	/// A channel to send a message to this thread.
	pub send: Sender<Message>,
	/// The thread handle for this thread. Can be used to join all the threads.
	pub handle: JoinHandle<()>,
}

impl WorkerHandle {
	/// Create a new [`WorkerHandle`].
	pub fn new(send: Sender<Message>, handle: JoinHandle<()>) -> Self {
		Self { send, handle }
	}
}

/// The master thread.
#[derive(Debug)]
pub struct Master {
	/// The id of the thread.
	pub id: ThreadId,
	/// A map to all the workers and a [`WorkerHandle`] per each of them.
	pub workers: BTreeMap<ThreadId, WorkerHandle>,
	/// A channel to receive messages from the workers.
	pub from_workers: Receiver<Message>,
	/// The state. This will be shared will all the workers.
	pub state: Arc<State>,
	/// The transaction pool.
	pub tx_pool: Vec<Transaction>,
	/// The orphan pool.
	pub orphan_pool: Vec<Transaction>,
}

impl Master {
	/// Create a new instance of the master queue.
	pub fn new(id: ThreadId, from_workers: Receiver<Message>) -> Self {
		Self {
			id: id,
			from_workers,
			workers: Default::default(),
			state: Default::default(),
			tx_pool: Default::default(),
			orphan_pool: Default::default(),
		}
	}

	/// Call [`Self::new`] with the current thread id.
	pub fn new_from_thread(from_workers: Receiver<Message>) -> Self {
		let id = thread::current().id().as_u64().into();
		Self::new(id, from_workers)
	}

	/// Get the number of workers.
	pub fn num_workers(&self) -> usize {
		self.workers.len()
	}

	/// Send a particular message to all workers.
	pub fn broadcast(&self, message: Message) -> Result<(), SendError<Message>> {
		self.workers
			.iter()
			.map(|(_, h)| h.send.send(message.clone()))
			.collect::<Result<_, _>>()
	}

	/// unpark all workers.
	pub fn unpark_all(&self) {
		self.workers
			.iter()
			.for_each(|(_, h)| h.handle.thread().unpark())
	}

	/// The main logic of the master thread.
	pub fn run(&mut self) {
		// distribute transactions, mark all transactions by their _designated_ executor.
		self.initial_phase();

		// collect any `Orphan` or `Executed` events. This will update some of the transactions'
		// `ExecutionStatus` to `Orphan` or `Done(_)` of some other thread than the designated one.
		self.collection_phase();
	}

	/// Logic of the collection phase of the execution.
	///
	/// Things that happen here:
	/// 1. collect `InitialPhaseReport` from all threads.
	/// 2. collect any `Orphan` transactions.
	/// 3. collect any `Executed` transactions.
	///
	/// This process ends when we have received all `InitialPhaseReport`. Then, we know exactly how
	/// many `Executed` events we must wait for. Only then, we can terminate.
	fn collection_phase(&mut self) {
		let mut executed_workers = 0;
		let mut executed_local = 0;
		let mut forwarded = 0;
		let mut reported = 0;
		let total = self.tx_pool.len();
		let workers_len = self.workers.len();

		let all_workers_done = |r: usize| r == workers_len;

		loop {
			if let Ok(Message {
				payload,
				from: worker,
			}) = self.from_workers.try_recv()
			{
				match payload {
					MessagePayload::InitialPhaseReport(e, f) => {
						executed_workers += e;
						forwarded += f;
						reported += 1;
					}
					MessagePayload::Orphan(tx) => {
						// FIXME: this must be an id.
						self.orphan_pool.push(tx);
					}
					MessagePayload::Executed(tx) => {
						// FIXME: this must be an id.
						// FIXME: perhaps avoid linear search here? idk. Or keep txs sorted and
						// binary search in them.
						self.tx_pool
							.iter_mut()
							.find(|ref t| tx.signature == t.signature)
							.map(|t| t.status = TransactionStatus::Done(worker))
							.expect("Transaction must exist in the pool");
						executed_local += 1;
					}
					_ => panic!("Unexpected message type at master."),
				}
			}

			// we all workers have said that we're done, and we've received enough `Executed`
			// messages. At this point all transactions must be either reported as orphan, or
			// executed.
			if all_workers_done(reported) && forwarded == executed_local {
				assert_eq!(
					total,
					executed_local + executed_workers + self.orphan_pool.len()
				);
				break;
			}
		}
	}

	/// Logic of the initial phase of the execution.
	///
	/// First, we distribute all the transactions to the worker threads with some arbitrary
	/// algorithm. We will assume that this distribution will hold unless if any of the worker
	/// threads send a message indicating that.
	///
	/// At the end of this phase, all transactions in the `tx_pool` must have been marked by either
	/// `Executed(id)` or `Orphan`. Moreover, the Orphan pool must have been populated.
	// FIXME: well we don't do this quite just.
	pub(crate) fn initial_phase(&mut self) {
		let threads_and_txs = self.distribute_transactions();

		// distribute transactions to all workers.
		threads_and_txs.into_iter().for_each(|(tid, tx)| {
			self.workers
				.get(&tid)
				.expect("Worker thread must exist; qed.")
				.send
				.send(MessagePayload::Transaction(tx).into())
				.expect("Sending should not fail; qed.")
		});

		// tell all workers that the initial phase is done.
		self.broadcast(MessagePayload::InitialPhaseDone.into())
			.expect("Broadcast should work; qed.");
	}

	/// For now, round robin distribution.
	pub(crate) fn distribute_transactions(&self) -> Vec<(ThreadId, Transaction)> {
		let worker_ids = self
			.workers
			.keys()
			.into_iter()
			.cloned()
			.collect::<Vec<ThreadId>>();
		let num_workers = worker_ids.len();

		self.tx_pool
			.iter()
			.enumerate()
			.map(|(idx, tx)| (worker_ids[idx % num_workers], tx.clone())) // FIXME: clone?
			.collect()
	}

	/// A run method only for testing.
	#[cfg(test)]
	pub fn run_test(&self) {
		// receive from all workers.
		let mut num_received = 0;
		let num_workers = self.num_workers();
		while num_received != num_workers {
			let payload = self.from_workers.recv().unwrap().payload;
			assert!(matches!(payload, MessagePayload::Test(x) if x == b"FromWorker".to_vec()));
			num_received += 1;
		}

		// send to all workers.
		self.broadcast(Message::new_from_thread(MessagePayload::Test(
			b"FromMaster".to_vec(),
		)))
		.unwrap();
	}

	/// Join on all the workers.
	///
	/// The master terminates upon calling this.
	pub fn join_all(self) -> std::thread::Result<()> {
		self.workers
			.into_iter()
			.map(|(_, handle)| handle.handle.join())
			.collect::<Result<_, _>>()
	}
}
