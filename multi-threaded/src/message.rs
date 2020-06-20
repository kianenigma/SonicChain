use primitives::ThreadId;
use runtime::OuterCall;
use std::collections::BTreeMap;
use std::sync::mpsc::Sender;

/// Status of a transaction.
///
/// This is used to annotate the final status of a transaction.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TransactionStatus {
	/// Done by the given thread.
	Done(primitives::ThreadId),
	/// Ended up being an orphan.
	Orphan,
}

/// Execution status of a transaction
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ExecutionStatus {
	/// It has just been created.
	Initial,
	/// Has already been forwarded by one thread.
	Forwarded,
}

impl Default for ExecutionStatus {
	fn default() -> Self {
		Self::Initial
	}
}

/// Opaque transaction type.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Transaction {
	/// Status of the transaction.
	///
	/// This should be set at the every end, once the transaction is executed.
	pub status: Option<TransactionStatus>,
	/// Execution status.
	pub exec_status: ExecutionStatus,
	/// The function of the transaction. This should be executed by a runtime.
	pub function: OuterCall,
	/// The signature of the transaction
	pub signature: Option<()>,
}

impl Transaction {
	pub fn new(call: OuterCall) -> Self {
		Self {
			function: call,
			status: None,
			exec_status: ExecutionStatus::Initial,
			signature: None,
		}
	}
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Message {
	pub payload: MessagePayload,
	pub from: ThreadId,
}

impl Message {
	pub fn new_from_thread(payload: MessagePayload) -> Self {
		let id = std::thread::current().id().as_u64().into();
		Self { payload, from: id }
	}
}

#[derive(Debug, Clone)]
pub enum MessagePayload {
	/// Data needed to finalize the setup of the worker.
	FinalizeSetup(BTreeMap<ThreadId, Sender<Message>>),
	/// Execute this transaction.
	///
	/// This message has no response; If the thread never respond back, then the master can assume
	/// that it has been executed by the thread.
	Transaction(Transaction),
	/// Initial transactions that the master distributed to the worker are done.
	InitialPhaseDone,
	/// Master is signaling the end.
	Terminate,
	/// A test payload.
	#[cfg(test)]
	Test(Vec<u8>),
}

impl PartialEq for MessagePayload {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Transaction(x), Self::Transaction(y)) => x == y,
			#[cfg(test)]
			(Self::Test(x), Self::Test(y)) => x == y,
			(Self::FinalizeSetup(x), Self::FinalizeSetup(y)) => {
				x.keys().into_iter().all(|key| y.contains_key(key))
			}
			_ => false,
		}
	}
}

impl Eq for MessagePayload {}
