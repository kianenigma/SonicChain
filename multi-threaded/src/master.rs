use crate::{message::Message, State};
use primitives::ThreadId;
use std::collections::BTreeMap;
use std::sync::{
	mpsc::{Receiver, Sender},
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
}

impl Master {
	/// Create a new instance of the master queue.
	pub fn new(id: ThreadId, from_workers: Receiver<Message>) -> Self {
		Self {
			id: id,
			from_workers,
			workers: Default::default(),
			state: Default::default(),
		}
	}

	/// Call [`Self::new`] with the current thread id.
	pub fn new_from_thread(from_workers: Receiver<Message>) -> Self {
		let id = thread::current().id().as_u64().into();
		Self::new(id, from_workers)
	}

	/// The main logic of the master thread.
	pub fn run() {
		unimplemented!();
	}

	/// Join on all the workers.
	///
	/// The master terminates upon calling this.
	pub fn join_all(self) {
		self.workers
			.into_iter()
			.for_each(|(_, handle)| handle.handle.join().unwrap())
	}
}
