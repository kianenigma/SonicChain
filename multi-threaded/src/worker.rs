use crate::{
	message::{ExecutionStatus, Message, MessagePayload, Transaction},
	State,
};
use primitives::ThreadId;
use runtime::{DispatchError, DispatchResult, Runtime};
use std::collections::BTreeMap;
use std::{
	sync::{
		mpsc::{Receiver, Sender},
		Arc,
	},
	thread,
};

/// The execution status of a transaction.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ExecutionOutcome {
	Executed(DispatchResult),
	Forwarded(ThreadId),
	ForwardedToMaster,
}

/// A worker thread.
pub struct Worker {
	/// The id of the worker.
	pub id: ThreadId,
	/// The id of the master thread.
	pub master_id: ThreadId,
	/// Shared state.
	pub state: Arc<State>,
	/// The runtime
	pub runtime: Runtime,
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
		// TODO: consider removing access to state from worker. Worker should only ever access state
		// from the runtime interface that it has.
		let runtime = Runtime::new(state.clone(), id);
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
					MessagePayload::Transaction(tx) => self.execute_or_forward(tx),
					_ => panic!("Unexpected message payload"),
				};
			}
		}
	}

	/// Execute all the transactions in the queue from master until it is empty.
	///
	/// This should be called only once at the beginning of the execution.
	fn deplete_master_queue(&self) {
		loop {
			let Message { payload, from } = self.from_master.recv().unwrap();
			debug_assert_eq!(from, self.master_id);

			match payload {
				MessagePayload::Transaction(tx) => {
					let outcome = self.execute_or_forward(tx.clone());
					log::info!("Execution of {:?} => {:?}", tx, outcome);
				}
				MessagePayload::InitialPhaseDone => break,
				_ => panic!("Unexpected message payload."),
			};
		}
	}

	/// Tries to execute transaction.
	///
	/// If execution went okay, returns `Ok(())`, else, it returns the thread id of the owner of the
	/// transaction.
	pub(crate) fn execute_transaction(&self, tx: Transaction) -> runtime::DispatchResult {
		let call = tx.function;
		let origin = tx.signature.0;
		runtime::Dispatchable::dispatch(&call, &self.runtime, origin)
	}

	/// Tries to execute the transaction, else forward it to either another worker who owns it, or
	/// the master of the transaction has already been forwarded.
	///
	/// This can this can never fail. Only errors will be communication, in which case it panics.
	pub(crate) fn execute_or_forward(&self, mut tx: Transaction) -> ExecutionOutcome {
		let execution_outcome = self.execute_transaction(tx.clone());
		match execution_outcome {
			Err(err) => {
				match (err, tx.exec_status) {
					(DispatchError::Tainted(owner), ExecutionStatus::Initial) => {
						// forward it to the owner.
						tx.exec_status = ExecutionStatus::Forwarded;
						let msg = MessagePayload::Transaction(tx).into();
						self.to_others
							.get(&owner)
							.expect("Must have queue to all other workers; qed.")
							.send(msg)
							.expect("Send to others should work; qed.");
						ExecutionOutcome::Forwarded(owner)
					}
					(DispatchError::Tainted(_), ExecutionStatus::Forwarded) => {
						// forward it to the master as an orphan.
						let msg = MessagePayload::Orphan(tx).into();
						self.to_master
							.send(msg)
							.expect("Send to master should work; qed.");
						ExecutionOutcome::ForwardedToMaster
					}
					(DispatchError::LogicError(why), _) => {
						// Fine. We executed this, and it was a logical error. if transaction
						// belonged to use initially, do nothing. Else, report to master that we've
						// executed it.
						if tx.exec_status == ExecutionStatus::Forwarded {
							let msg = MessagePayload::Executed(tx, self.id).into();
							self.to_master
								.send(msg)
								.expect("Send to master should work; qed.");
						}
						ExecutionOutcome::Executed(Err(DispatchError::LogicError(why)))
					}
				}
			}
			Ok(_) => {
				if tx.exec_status == ExecutionStatus::Forwarded {
					let msg = MessagePayload::Executed(tx, self.id).into();
					self.to_master
						.send(msg)
						.expect("Send to master should work; qed.");
				}
				ExecutionOutcome::Executed(Ok(()))
			}
		}
	}

	/// A run method only used for testing.
	#[cfg(test)]
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
					crate::message::MessagePayload::Test(vec![self.id as u8]),
				))
				.unwrap();
		});

		let mut num_received = 0;
		let num_workers = self.to_others.len();
		while num_received != (num_workers - 1) {
			let message = self.from_others.recv().unwrap();
			let Message { payload, from } = message;
			println!("Received {:?} from {:?}", payload, from);
			assert!(matches!(payload, MessagePayload::Test(x) if x == vec![from as u8]));
			num_received += 1;
		}
	}
}

#[cfg(test)]
mod worker_test {
	use super::*;
	use parity_scale_codec::Encode;
	use primitives::*;
	use runtime::balances::storage::BalanceOf;
	use runtime::OuterCall;
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
		let call = OuterCall::Balances(runtime::balances::Call::Transfer(
			testing::bob().public(),
			999,
		));
		let signed_call = call.using_encoded(|payload| origin.sign(payload));
		(
			Transaction::new(call, origin.public(), signed_call),
			origin.public(),
		)
	}

	#[test]
	fn can_execute_tx() {
		let (worker, _, master_rx) = test_worker();
		let alice = testing::alice();
		let (tx, alice) = test_tx(alice);

		// because alice has no funds yet.
		assert!(matches!(
			worker.execute_or_forward(tx.clone()),
			ExecutionOutcome::Executed(Err(e)) if e == DispatchError::LogicError("Does not have enough funds.")
		));

		// give alice some funds.
		BalanceOf::write(&worker.runtime, alice, 999).unwrap();

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
				.unsafe_read_taint(&<BalanceOf<Runtime>>::key_for(testing::alice().public(),))
				.unwrap(),
			worker.id
		);

		assert_eq!(
			worker
				.state
				.unsafe_read_taint(&<BalanceOf<Runtime>>::key_for(testing::bob().public(),))
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
		let alice_key = <BalanceOf<Runtime>>::key_for(alice);
		worker
			.state
			.unsafe_insert(&alice_key, state::StateEntry::new_taint(OTHER_WORKER));

		assert!(matches!(
			dbg!(worker.execute_or_forward(tx)),
			ExecutionOutcome::Forwarded(x) if x == OTHER_WORKER
		));

		let incoming = other_rx.recv().unwrap();
		assert_eq!(incoming.from, worker.id);
		assert!(
			matches!(incoming.payload, MessagePayload::Transaction(tx) if tx.exec_status == ExecutionStatus::Forwarded)
		)
	}

	#[test]
	fn will_forward_to_master_if_already_forwarded() {
		let (worker, _, master_rx) = test_worker();
		let alice = testing::alice();
		let (mut tx, alice) = test_tx(alice);
		tx.exec_status = ExecutionStatus::Forwarded;

		// manually taint the storage item of bob to some other thread.
		let alice_key = <BalanceOf<Runtime>>::key_for(alice);
		worker
			.state
			.unsafe_insert(&alice_key, state::StateEntry::new_taint(OTHER_WORKER));

		assert!(matches!(
			dbg!(worker.execute_or_forward(tx)),
			ExecutionOutcome::ForwardedToMaster
		));

		let incoming = master_rx.recv().unwrap();
		assert_eq!(incoming.from, worker.id);
		assert!(matches!(incoming.payload, MessagePayload::Orphan(_)))
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
		assert!(matches!(
			msg.payload,
			MessagePayload::Executed(_, id) if id == worker.id
		));
	}
}
