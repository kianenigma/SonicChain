use primitives::ThreadId;
use runtime::OuterCall;
use std::collections::BTreeMap;
use std::sync::mpsc::Sender;

/// Status of a transaction.
///
/// This is used to annotate the final status of a transaction.
#[derive(Debug, Copy, Clone)]
pub enum TransactionStatus {
	/// Done by the given thread.
	Done(primitives::ThreadId),
	/// Ended up being an orphan.
	Orphan,
}

/// Execution status of a transaction
#[derive(Debug, Copy, Clone)]
pub enum ExecutionStatus {
	/// It has just been created.
	Initial,
	/// Has already been forwarded by one thread.
	Forwarded,
}

/// Opaque transaction type.
#[derive(Debug)]
pub struct Transaction {
	/// Status of the transaction.
	pub status: TransactionStatus,
	/// Execution status.
	///
	/// This should be set at the every end, once the transaction is executed.
	pub exec_status: ExecutionStatus,
	/// The function of the transaction. This should be executed by a runtime.
	pub function: OuterCall,
}

#[derive(Debug)]
pub struct Message {
	pub payload: MessagePayload,
	pub from: ThreadId,
}

#[derive(Debug)]
pub enum MessagePayload {
	/// Execute this transaction.
	///
	/// This message has no response; If the thread never respond back, then the master can assume
	/// that it has been executed by the thread.
	Transaction(Transaction),
	/// Data needed to finalize the setup of the worker.
	FinalizeSetup(BTreeMap<ThreadId, Sender<Message>>),
}
