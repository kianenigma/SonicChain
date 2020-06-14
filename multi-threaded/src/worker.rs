use crate::{message::Message, State};
use primitives::ThreadId;
use std::collections::BTreeMap;
use std::{
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
		Self {
			id,
			master_id,
			state,
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
	pub fn run(self) {
		unimplemented!();
	}
}
