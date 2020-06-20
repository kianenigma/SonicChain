use crate::{
	message::{Message, MessagePayload, Transaction, TransactionStatus},
	State,
};
use primitives::ThreadId;
use runtime::{OuterCall, Runtime};
use std::collections::BTreeMap;
use std::{
	matches,
	sync::{
		mpsc::{Receiver, Sender},
		Arc,
	},
	thread,
};

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
		loop {
			let Message { payload, from } = self.from_master.recv().unwrap();
			debug_assert_eq!(from, self.master_id);

			match payload {
				MessagePayload::Transaction(tx) => self.execute_or_forward(tx),
				MessagePayload::InitialPhaseDone => break,
				_ => panic!("Unexpected message payload."),
			}
			.unwrap();
		}

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
				}
				.unwrap()
			}
		}
	}

	/// Tries to execute transaction.
	///
	/// If execution went okay, returns `Ok(())`, else, it returns the thread id of the owner of the
	/// transaction.
	pub(crate) fn execute_transaction(&self, tx: Transaction) -> Result<(), ThreadId> {
		Ok(())
	}

	/// Tries to execute the transaction, else forward it to either another worker who owns it, or
	/// the master of the transaction has already been forwarded.
	///
	/// This can only fail if the communication fails.
	pub(crate) fn execute_or_forward(&self, tx: Transaction) -> Result<(), ()> {
		Ok(())
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
	use primitives::*;
	use std::sync::mpsc::channel;

	fn test_worker() -> Worker {
		let state = State::new().as_arc();
		let (master_tx, master_rx) = channel();
		let (_, others_rx) = channel();
		let worker = Worker::new_from_thread(99, state, master_tx, master_rx, others_rx);
		worker
	}

	fn test_tx() -> Transaction {
		let call = OuterCall::Balances(runtime::balances::Call::Transfer(AccountId::random(), 999));
		Transaction::new(call)
	}

	#[test]
	fn can_execute_tx() {
		let worker = test_worker();
		let tx = test_tx();
		worker.execute_or_forward(tx).unwrap();
	}

	#[test]
	fn will_forward_tx_if_tainted() {}

	#[test]
	fn will_forward_to_master_if_already_forwarded() {}
}
