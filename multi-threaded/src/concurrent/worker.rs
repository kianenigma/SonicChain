use crate::{
	types::{ExecutionStatus, Message, MessagePayload, Transaction},
	State,
};
use primitives::{ThreadId, TransactionId};
use runtime::{DispatchError, DispatchResult, WorkerRuntime};
use std::collections::BTreeMap;
use std::{
	sync::{
		mpsc::{Receiver, Sender},
		Arc,
	},
	thread,
};

const LOG_TARGET: &'static str = "worker";

macro_rules! log {
	($level:tt, $patter:expr $(, $values:expr)* $(,)?) => {
		log::$level!(
			target: LOG_TARGET,
			$patter $(, $values)*
		)
	};
}

/// The execution outcome of a transaction.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ExecutionOutcome {
	/// This transaction was executed successfully with the contained dispatch result.
	Executed(DispatchResult),
	/// This transaction was forwarded to another worker thread with the given thread id.
	Forwarded(ThreadId),
	/// This transaction was forwarded to master as Orphan.
	ForwardedToMaster,
}

/// A worker thread.
pub struct Worker {
	/// The id of the worker.
	pub id: ThreadId,
	/// The id of the master thread.
	pub master_id: ThreadId,
	/// Shared state.
	///
	/// Note that the worker should typically only ever access the state through the runtime.
	pub state: Arc<State>,
	/// The runtime
	pub runtime: WorkerRuntime,
	/// Channel to send messages to master.
	pub to_master: Sender<Message>,
	/// Channel to receive data from the master.
	pub from_master: Receiver<Message>,
	/// Map of channels to send messages to other workers.
	pub to_others: BTreeMap<ThreadId, Sender<Message>>,
	/// Channel to receive messages from other workers.
	pub from_others: Receiver<Message>,
}

impl Worker {
	/// Create a new worker thread.
	pub fn new(
		id: ThreadId,
		master_id: ThreadId,
		state: Arc<State>,
		to_master: Sender<Message>,
		from_master: Receiver<Message>,
		from_others: Receiver<Message>,
	) -> Self {
		let runtime = WorkerRuntime::new(state.clone(), id);
		Self {
			id,
			master_id,
			state,
			runtime,
			to_master,
			from_master,
			to_others: Default::default(),
			from_others,
		}
	}

	/// Call [`Self::new`] with the current thread id.
	pub fn new_from_thread(
		master_id: ThreadId,
		state: Arc<State>,
		to_master: Sender<Message>,
		from_master: Receiver<Message>,
		from_others: Receiver<Message>,
	) -> Self {
		let id = thread::current().id().as_u64().into();
		Self::new(id, master_id, state, to_master, from_master, from_others)
	}

	/// Run the main worker thread logic. This will called after the initial unparking of the
	/// master.
	///
	/// It will loop and try and receive stuff from master until it is done, then it will read from
	/// worker incoming queue until termination.
	pub fn run(self) {
		// execute everything from master.
		log!(info, "Starting phase `deplete_master_queue()`.");
		self.deplete_master_queue();

		// try and read termination signal from master, else execute anything from other workers.
		loop {
			if let Ok(Message { payload, from: _ }) = self.from_master.try_recv() {
				match payload {
					MessagePayload::Terminate => break,
					_ => panic!("Unexpected message payload."),
				}
			}
			if let Ok(Message { payload, from: _ }) = self.from_others.try_recv() {
				match payload {
					MessagePayload::Transaction(mut tx) => {
						tx.exec_status = ExecutionStatus::Forwarded;
						self.execute_or_forward(tx)
					}
					_ => panic!("Unexpected message payload"),
				};
			}
		}
	}

	/// Execute all the transactions in the queue from master until it is empty.
	///
	/// This should be called only once at the beginning of the execution.
	fn deplete_master_queue(&self) {
		let mut executed = 0;
		let mut forwarded = 0;
		loop {
			let Message { payload, from } = self.from_master.recv().unwrap();
			debug_assert_eq!(from, self.master_id);

			match payload {
				MessagePayload::Transaction(tx) => {
					let outcome = self.execute_or_forward(tx.clone());
					match outcome {
						ExecutionOutcome::Executed(_) => executed += 1,
						ExecutionOutcome::Forwarded(_) | ExecutionOutcome::ForwardedToMaster => {
							forwarded += 1
						}
					}
				}
				MessagePayload::InitialPhaseDone => break,
				_ => panic!("Unexpected message payload."),
			};
		}

		let message = MessagePayload::InitialPhaseReport(executed, forwarded).into();
		log!(info, "Sending report {:?}", message);
		self.to_master
			.send(message)
			.expect("Sending to master cannot fail; qed");
	}

	/// Tries to execute transaction.
	///
	/// If execution went okay, returns `Ok(())`, else, it returns the thread id of the owner of the
	/// transaction.
	pub(crate) fn execute_transaction(&self, tx: Transaction) -> runtime::DispatchResult {
		let call = tx.function;
		let origin = tx.signature.0;
		self.runtime.dispatch(call, origin)
	}

	/// Tries to execute the transaction, else forward it to either another worker who owns it, or
	/// the master of the transaction has already been forwarded.
	///
	/// This also reports to master if a transaction has been forwarded to us and we successfully
	/// executed it.
	///
	/// NOTE: in case this forwards a transaction to another thread, it does not update the
	/// exec_status field. The receiver should do so.
	///
	/// This can this can never fail. Only errors will be communication, in which case it panics.
	pub(crate) fn execute_or_forward(&self, tx: Transaction) -> ExecutionOutcome {
		// keep tx id because we don't need the rest, move the tx object to the function.
		let tid = tx.id;
		let exec_status = tx.exec_status;
		let execution_outcome = self.execute_transaction(tx.clone());

		let forward_to_master = |tid: TransactionId| -> ExecutionOutcome {
			let msg = MessagePayload::WorkerOrphan(tid).into();
			self.to_master
				.send(msg)
				.expect("Send to master should work; qed.");
			ExecutionOutcome::ForwardedToMaster
		};

		let forward_to_worker = |tx: Transaction, wid: ThreadId| -> ExecutionOutcome {
			let msg = MessagePayload::Transaction(tx).into();
			self.to_others
				.get(&wid)
				.expect("Must have queue to all other workers; qed.")
				.send(msg)
				.expect("Send to others should work; qed.");
			ExecutionOutcome::Forwarded(wid)
		};

		let report_execution = |tid: TransactionId| {
			let msg = MessagePayload::WorkerExecuted(tid).into();
			self.to_master
				.send(msg)
				.expect("Send to master should work; qed.");
		};

		let final_outcome = match execution_outcome {
			Err(err) => {
				match (err, exec_status) {
					(DispatchError::Tainted(owner, corrupt), ExecutionStatus::Initial) => {
						if corrupt {
							forward_to_master(tid)
						} else {
							forward_to_worker(tx.clone(), owner)
						}
					}
					(DispatchError::Tainted(_, _), ExecutionStatus::Forwarded) => {
						forward_to_master(tx.id)
					}
					(DispatchError::LogicError(why), ExecutionStatus::Initial) => {
						// logical error and initial, all good, do nothing.
						ExecutionOutcome::Executed(Err(DispatchError::LogicError(why)))
					}
					(DispatchError::LogicError(why), ExecutionStatus::Forwarded) => {
						// it was an error and it was forwarded. Report to master.
						report_execution(tid);
						ExecutionOutcome::Executed(Err(DispatchError::LogicError(why)))
					}
				}
			}
			Ok(_) => {
				if exec_status == ExecutionStatus::Forwarded {
					report_execution(tid)
				}
				ExecutionOutcome::Executed(Ok(()))
			}
		};

		log!(trace, "execute or forward {:?} => {:?}", tx, final_outcome);
		final_outcome
	}

	/// A run method only used for testing.
	pub fn test_run(self) {
		// send this to master.
		self.to_master
			.send(Message::new_from_thread(MessagePayload::Test(
				b"FromWorker".to_vec(),
			)))
			.unwrap();

		// expect this from master.
		let data = self.from_master.recv().unwrap().payload;
		assert!(matches!(data, MessagePayload::Test(x) if x == b"FromMaster".to_vec()));

		// send this to next worker.
		self.to_others.iter().for_each(|(_, sender)| {
			sender
				.send(Message::new_from_thread(
					crate::types::MessagePayload::Test(vec![self.id as u8]),
				))
				.unwrap();
		});

		let mut num_received = 0;
		let num_workers = self.to_others.len();
		while num_received != (num_workers - 1) {
			let message = self.from_others.recv().unwrap();
			let Message { payload, from } = message;
			log!(debug, "Received {:?} from {:?}", payload, from);
			assert!(matches!(payload, MessagePayload::Test(x) if x == vec![from as u8]));
			num_received += 1;
		}
	}
}

#[cfg(test)]
mod worker_test {
	use super::*;
	use primitives::*;
	use runtime::balances::*;
	use std::matches;
	use std::sync::mpsc::channel;

	const OTHER_WORKER: ThreadId = 69;
	const MASTER_ID: ThreadId = 99;

	fn test_worker() -> (Worker, Receiver<Message>, Receiver<Message>) {
		let state = State::new().as_arc();
		let (_, from_master_rx) = channel();
		let (to_master_tx, to_master_rx) = channel();
		let (_, others_rx) = channel();
		let (other_worker_tx, other_worker_rx) = channel();
		let mut worker =
			Worker::new_from_thread(MASTER_ID, state, to_master_tx, from_master_rx, others_rx);
		worker.to_others.insert(OTHER_WORKER, other_worker_tx);
		(worker, other_worker_rx, to_master_rx)
	}

	fn test_tx(origin: Pair) -> (Transaction, AccountId) {
		(Transaction::new_transfer(origin), testing::alice().public())
	}

	#[test]
	fn can_execute_tx() {
		let (worker, _, master_rx) = test_worker();
		let alice = testing::alice();
		let (tx, alice) = test_tx(alice);
		let master_runtime = runtime::MasterRuntime::new(Arc::clone(&worker.state), 0);

		// because alice has no funds yet.
		assert!(matches!(
			worker.execute_or_forward(tx.clone()),
			ExecutionOutcome::Executed(Err(e)) if e == DispatchError::LogicError("Does not have enough funds.")
		));

		// give alice some funds.
		BalanceOf::write(&master_runtime, alice, 999.into()).unwrap();

		// now alice has some funds.
		assert!(matches!(
			worker.execute_or_forward(tx),
			ExecutionOutcome::Executed(Ok(_))
		));

		// Nothing has been sent to master.
		assert!(master_rx
			.recv_timeout(std::time::Duration::from_secs(1))
			.is_err());

		// verify that this thread, whatever it is, is now the owner of both alice and bob keys.
		assert_eq!(
			worker
				.state
				.unsafe_read_taint(&<BalanceOf<WorkerRuntime>>::key_for(
					testing::alice().public(),
				))
				.unwrap(),
			worker.id
		);

		assert_eq!(
			worker
				.state
				.unsafe_read_taint(&<BalanceOf<WorkerRuntime>>::key_for(
					testing::bob().public(),
				))
				.unwrap(),
			worker.id
		);
	}

	#[test]
	fn will_forward_tx_if_tainted() {
		let (worker, other_rx, _) = test_worker();
		let alice = testing::alice();
		let (tx, alice) = test_tx(alice);

		// manually taint the storage item of bob to some other thread.
		let alice_key = <BalanceOf<WorkerRuntime>>::key_for(alice);
		worker
			.state
			.unsafe_insert(&alice_key, state::StateEntry::new_taint(OTHER_WORKER));

		assert!(matches!(
			worker.execute_or_forward(tx.clone()),
			ExecutionOutcome::Forwarded(x) if x == OTHER_WORKER
		));

		let incoming = other_rx.recv().unwrap();
		assert_eq!(incoming.from, worker.id);
		assert!(matches!(incoming.payload, MessagePayload::Transaction(itx) if itx.id == tx.id))
	}

	#[test]
	fn will_forward_to_master_if_already_forwarded() {
		let (worker, _, master_rx) = test_worker();
		let alice = testing::alice();
		let (mut tx, alice) = test_tx(alice);
		tx.exec_status = ExecutionStatus::Forwarded;

		// manually taint the storage item of alice to some other thread.
		let alice_key = <BalanceOf<WorkerRuntime>>::key_for(alice);
		worker
			.state
			.unsafe_insert(&alice_key, state::StateEntry::new_taint(OTHER_WORKER));

		assert!(matches!(
			worker.execute_or_forward(tx),
			ExecutionOutcome::ForwardedToMaster
		));

		let incoming = master_rx.recv().unwrap();
		assert_eq!(incoming.from, worker.id);
		assert!(matches!(incoming.payload, MessagePayload::WorkerOrphan(_)))
	}

	#[test]
	fn will_report_to_master_if_forwarded_and_executed() {
		let (worker, _, master_rx) = test_worker();
		let alice = testing::alice();
		let (mut tx, _) = test_tx(alice);

		// make if forwarded for whatever reason.
		tx.exec_status = ExecutionStatus::Forwarded;

		// because alice has no funds yet.
		assert!(matches!(
			worker.execute_or_forward(tx.clone()),
			ExecutionOutcome::Executed(Err(e)) if e == DispatchError::LogicError("Does not have enough funds.")
		));

		// master should have received a notification now.
		let msg = master_rx.recv().unwrap();
		assert!(matches!(msg.payload, MessagePayload::WorkerExecuted(_)));
	}
}
