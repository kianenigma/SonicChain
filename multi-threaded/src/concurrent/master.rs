use crate::types::MessagePayload;
use crate::{pool::*, types::Message, Distributer, State, Transaction, TransactionStatus};
use primitives::*;
use std::collections::BTreeMap;
use std::sync::{
	mpsc::{Receiver, SendError, Sender},
	Arc,
};
use std::thread::{self, JoinHandle};

const LOG_TARGET: &'static str = "master";

macro_rules! log {
	($level:tt, $patter:expr $(, $values:expr)* $(,)?) => {
		log::$level!(
			target: LOG_TARGET,
			$patter $(, $values)*
		)
	};
}

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
pub struct Master<P: TransactionPool<Transaction>, D> {
	/// The id of the thread.
	pub id: ThreadId,
	/// A map to all the workers and a [`WorkerHandle`] per each of them.
	pub workers: BTreeMap<ThreadId, WorkerHandle>,
	/// A channel to receive messages from the workers.
	pub from_workers: Receiver<Message>,
	/// The state. This will be shared will all the workers.
	pub state: Arc<State>,
	/// The transaction pool.
	pub tx_pool: P,
	/// The orphan pool.
	pub orphan_pool: Vec<Transaction>,
	/// A master runtime used for orphan phase and validation.
	pub runtime: runtime::MasterRuntime,
	// Marker.
	_marker: std::marker::PhantomData<D>,
}

impl<P: TransactionPool<Transaction>, D: Distributer> Master<P, D> {
	/// Create a new instance of the master queue.
	pub fn new(
		id: ThreadId,
		from_workers: Receiver<Message>,
		initial_state: Option<State>,
	) -> Self {
		let state: Arc<State> = initial_state.unwrap_or_default().into();
		let runtime = runtime::MasterRuntime::new(Arc::clone(&state), id);
		Self {
			id: id,
			from_workers,
			workers: Default::default(),
			state,
			tx_pool: P::new(),
			orphan_pool: Default::default(),
			runtime,
			_marker: std::marker::PhantomData::<D>,
		}
	}

	/// Call [`Self::new`] with the current thread id.
	pub fn new_from_thread(from_workers: Receiver<Message>, initial_state: Option<State>) -> Self {
		let id = thread::current().id().as_u64().into();
		Self::new(id, from_workers, initial_state)
	}

	/// Get the number of workers.
	pub fn num_workers(&self) -> usize {
		self.workers.len()
	}

	/// Send a particular message to all workers.
	pub fn broadcast(&self, message: Message) -> Result<(), SendError<Message>> {
		log!(info, "Broadcasting {:?}", message);
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
	pub fn run_author(&mut self) {
		// unpark all workers.
		self.unpark_all();

		// distribute transactions, mark all transactions by their _designated_ executor.
		self.initial_phase();

		// collect any `Orphan` or `Executed` events. This will update some of the transactions'
		// `ExecutionStatus` to `Orphan` or `Done(_)` of some other thread than the designated one.
		self.collection_phase();

		// Send terminate to all workers.
		self.broadcast(MessagePayload::Terminate.into())
			.expect("broadcast should work; qed.");

		// Execute all the collected orphans.
		self.orphan_phase();
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

		loop {
			if let Ok(Message {
				payload,
				from: worker,
			}) = self.from_workers.try_recv()
			{
				log!(
					debug,
					"message in collection phase form {:?} => {:?}",
					worker,
					payload
				);
				match payload {
					MessagePayload::InitialPhaseReport(e, f) => {
						executed_workers += e;
						forwarded += f;
						reported += 1;
					}
					MessagePayload::WorkerOrphan(tid) => {
						let mut orphan = self
							.tx_pool
							.remove(|t| t.id == tid)
							.expect("Transaction must exist in the pool");
						orphan.status = TransactionStatus::Orphan;
						self.orphan_pool.push(orphan);
					}
					MessagePayload::WorkerExecuted(tid) => {
						let (_, t) = self
							.tx_pool
							.get_mut(|t| t.id == tid)
							.expect("Transaction must exist in the pool");

						log!(debug, "Updating owner of {:?} to {:?}", t, worker);
						// initially, the transaction must have been marked with Done(_) of some
						// other thread, and now we update it.
						match t.status {
							TransactionStatus::Done(initial_worker) => {
								assert_ne!(worker, initial_worker);
								t.status = TransactionStatus::Done(worker);
								executed_local += 1;
							}
							_ => panic!("Unexpected initial worker."),
						};
					}
					_ => panic!("Unexpected message type at master."),
				}
			}

			// we all workers have said that we're done, and we've received enough `Executed`
			// messages. At this point all transactions must be either reported as orphan, or
			// executed.
			if reported == workers_len && forwarded == (executed_local + self.orphan_pool.len()) {
				debug_assert_eq!(
					total,
					executed_local + executed_workers + self.orphan_pool.len()
				);
				log!(
					info,
					"Finishing Collection phase with [{} executed][{} forwarded][{} orphaned]",
					executed_workers,
					executed_local,
					self.orphan_pool.len()
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
	/// At the end of this phase, all transactions in the `tx_pool` must have been marked by
	/// `Executed(id)` where the id is their _designated worker_.
	pub(crate) fn initial_phase(&mut self) {
		self.distribute_transactions();

		let threads_and_txs = self
			.tx_pool
			.all()
			.iter()
			.map(|tx| match tx.status {
				TransactionStatus::Done(w) => (w, tx.clone()),
				_ => panic!(
					"A transaction has not been assigned. This is a bug in the distribution code"
				),
			})
			.collect::<Vec<_>>();

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

	/// Execute all the transactions in the orphan queue on top of the previous state.
	///
	/// At this point, we are sure that no other thread is alive.
	pub(crate) fn orphan_phase(&mut self) {
		log!(
			info,
			"Starting orphan phase with {} transactions.",
			self.orphan_pool.len()
		);
		for tx in self.orphan_pool.iter_mut() {
			debug_assert_eq!(tx.status, TransactionStatus::Orphan);
			let origin = tx.signature.0;
			log!(trace, "Executing orphan tx: {:?}", tx);
			let _outcome = self
				.runtime
				.dispatch(tx.function.clone(), origin)
				.expect("Executing transaction in the master runtime should never fail; qed");
			log!(trace, "outcome = {:?}", _outcome,);
		}
	}

	/// For now, round robin distribution.
	///
	/// This marks each transaction with the Done(_) of the assigned thread id.`
	pub(crate) fn distribute_transactions(&mut self) {
		let worker_ids = self
			.workers
			.keys()
			.into_iter()
			.cloned()
			.collect::<Vec<ThreadId>>();

		D::distribute(&self.runtime, worker_ids.as_ref(), &mut self.tx_pool);
	}

	/// Join on all the workers.
	///
	/// The master terminates upon calling this.
	pub fn join_all(self) -> std::thread::Result<()> {
		log!(warn, "Joining all threads.");
		self.workers
			.into_iter()
			.map(|(_, handle)| handle.handle.join())
			.collect::<Result<_, _>>()
	}

	/// A run method only for testing.
	#[cfg(test)]
	pub fn run_test(&self) {
		self.unpark_all();

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
}
#[cfg(test)]
mod master_tests_single_worker {
	use super::*;
	use crate::tx_distribution::RoundRobin;
	use crate::types::*;
	use primitives::testing::*;
	use std::sync::mpsc::*;

	const MASTER_ID: ThreadId = 1;
	const WORKER_ID: ThreadId = 9;
	const NUM_TX: usize = 3;

	type Pool = VecPool<Transaction>;

	fn test_master() -> (Master<Pool, RoundRobin>, Receiver<Message>, Sender<Message>) {
		let (from_workers_tx, from_workers_rx) = channel();
		let mut master = Master::<Pool, RoundRobin>::new(MASTER_ID, from_workers_rx, None);

		let (to_worker_tx, to_worker_rx) = channel();
		let handle = std::thread::spawn(move || {
			thread::park();
		});
		master
			.workers
			.insert(WORKER_ID, WorkerHandle::new(to_worker_tx, handle));

		let origins = vec![alice(), dave(), eve()];
		assert_eq!(origins.len(), NUM_TX);
		for o in origins.into_iter() {
			master.tx_pool.push_back(Transaction::new_transfer(o))
		}

		(master, to_worker_rx, from_workers_tx)
	}

	#[test]
	fn initial_phase_works() {
		let (mut master, worker_rx, _) = test_master();

		master.initial_phase();

		// 4 tx must arrive.
		for _ in 0..NUM_TX {
			assert!(matches!(
				worker_rx.recv().unwrap().payload,
				MessagePayload::Transaction(_)
			));
		}

		// then this.
		assert!(matches!(
			worker_rx.recv().unwrap().payload,
			MessagePayload::InitialPhaseDone
		));
	}

	#[test]
	fn collection_phase_works() {
		let (mut master, _, from_worker_tx) = test_master();

		// in a single worker setup it makes not much sense to have any sort of forwarding or
		// orphans.
		from_worker_tx
			.send(MessagePayload::InitialPhaseReport(NUM_TX, 0).into())
			.unwrap();

		// this must terminate eventually with the messages sent above.
		master.collection_phase();
	}
}

#[cfg(test)]
mod master_tests_multi_worker {
	use super::*;
	use crate::tx_distribution::RoundRobin;
	use crate::types::*;
	use primitives::testing::*;
	use std::sync::mpsc::*;

	const MASTER_ID: ThreadId = 1;
	const WORKER_IDS: [ThreadId; 3] = [10, 11, 12];
	const NUM_TX: usize = 45;

	type Pool = VecPool<Transaction>;

	fn test_master() -> (
		Master<Pool, RoundRobin>,
		Vec<Receiver<Message>>,
		Sender<Message>,
	) {
		let (from_workers_tx, from_workers_rx) = channel();
		let mut master = Master::<Pool, RoundRobin>::new(MASTER_ID, from_workers_rx, None);

		let mut worker_receivers = vec![];

		(0..WORKER_IDS.len()).for_each(|i| {
			let (to_worker_tx, to_worker_rx) = channel();
			let handle = std::thread::spawn(move || {
				thread::park();
			});
			master
				.workers
				.insert(WORKER_IDS[i], WorkerHandle::new(to_worker_tx, handle));
			worker_receivers.push(to_worker_rx);
		});

		for i in 0..NUM_TX {
			let mut tx = Transaction::new_transfer(random());
			tx.id = i as TransactionId;
			master.tx_pool.push_back(tx);
		}

		// needed for everything to work well.
		assert!(NUM_TX % WORKER_IDS.len() == 0);

		(master, worker_receivers, from_workers_tx)
	}

	#[test]
	fn initial_phase_works() {
		let (mut master, worker_receivers, _) = test_master();

		master.initial_phase();

		// each thread must receive NUM_TX / WORKER_IDS.len() txs and one `InitialPhaseDone`.
		for rx in worker_receivers {
			// NOTE: this might break once we have something other than basic round robin.
			for _ in 0..NUM_TX / WORKER_IDS.len() {
				assert!(matches!(
					rx.recv().unwrap().payload,
					MessagePayload::Transaction(_)
				));
			}

			assert!(matches!(
				rx.recv().unwrap().payload,
				MessagePayload::InitialPhaseDone
			));
		}
	}

	#[test]
	fn collection_phase_works_basic() {
		// IMPORTANT NOTE: we must bring _receivers into scope to prevent them from being `Drop`ed,
		// and the channel getting closed.
		let (mut master, _receivers, from_worker_tx) = test_master();

		// in this case this makes not difference thou'.
		master.initial_phase();

		// each worker reports back that they've done NUM_TX/Len.
		for _ in 0..WORKER_IDS.len() {
			from_worker_tx
				.send(MessagePayload::InitialPhaseReport(NUM_TX / WORKER_IDS.len(), 0).into())
				.unwrap();
		}

		// this must terminate eventually with the messages sent above.
		master.collection_phase();
	}

	#[test]
	fn collection_phase_works_with_forwarded() {
		let (mut master, _receivers, from_worker_tx) = test_master();

		master.initial_phase();

		// each worker reports back that they've done all except one.
		for _ in 0..WORKER_IDS.len() {
			from_worker_tx
				.send(MessagePayload::InitialPhaseReport(NUM_TX / WORKER_IDS.len() - 1, 1).into())
				.unwrap();
		}

		// each thread will report one `Executed(_)`. The ID is kinda arbitrary at this stage.
		for i in 0..WORKER_IDS.len() {
			from_worker_tx
				.send(MessagePayload::WorkerExecuted(i as TransactionId).into())
				.unwrap();
		}

		// this must terminate eventually with the messages sent above.
		master.collection_phase();
	}
}
