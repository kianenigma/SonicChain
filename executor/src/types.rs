use parity_scale_codec::Encode;
use primitives::{ThreadId, TransactionId};
use runtime::OuterCall;
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::sync::mpsc::Sender;

#[derive(Debug, Eq, PartialEq)]
/// A block of transaction.
pub struct Block {
	/// Transactions within the block.
	pub transactions: Vec<Transaction>,
}

impl From<Vec<Transaction>> for Block {
	fn from(transactions: Vec<Transaction>) -> Self {
		Self { transactions }
	}
}

/// Status of a transaction.
///
/// This is used to annotate the final status of a transaction.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum TransactionStatus {
	/// Done by the given thread.
	Done(ThreadId),
	/// Ended up being an orphan.
	Orphan,
	/// Not yet executed.
	NotExecuted,
}

impl Default for TransactionStatus {
	fn default() -> Self {
		TransactionStatus::NotExecuted
	}
}

/// Execution status of a transaction.
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
#[derive(Clone, Eq, PartialEq)]
pub struct Transaction {
	/// The identifier of the transaction.
	///
	/// This must be strictly unique.
	pub id: TransactionId,
	/// Status of the transaction.
	///
	/// This should be set at the every end, once the transaction is executed.
	pub status: TransactionStatus,
	/// Execution status.
	pub exec_status: ExecutionStatus,
	/// The function of the transaction. This should be executed by a runtime.
	pub function: OuterCall,
	/// The signature of the transaction
	pub signature: (primitives::AccountId, primitives::Signature),
}

impl Transaction {
	pub fn set_done(&mut self, by_whom: ThreadId) {
		self.status = TransactionStatus::Done(by_whom);
	}

	pub fn set_orphan(&mut self) {
		self.status = TransactionStatus::Orphan;
	}
}

/// An opaque transaction trait that can be verified.
pub trait VerifiableTransaction {
	/// Verify that this transaction is sane.
	///
	/// This should typically just check the signature.
	fn verify(&self) -> bool;
}

impl VerifiableTransaction for Transaction {
	fn verify(&self) -> bool {
		let (origin, signature) = self.signature;
		let payload = self.function.encode();
		origin.verify(payload.as_ref(), &signature)
	}
}

impl Debug for Transaction {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.debug_struct("Transaction")
			.field("id", &self.id)
			.field("status", &self.status)
			.field("exec_status", &self.exec_status)
			.finish_non_exhaustive()
	}
}

impl Transaction {
	pub fn new(
		id: TransactionId,
		call: OuterCall,
		origin: primitives::AccountId,
		signed_call: primitives::Signature,
	) -> Self {
		Self {
			id,
			function: call,
			status: TransactionStatus::NotExecuted,
			exec_status: ExecutionStatus::Initial,
			signature: (origin, signed_call),
		}
	}

	/// A test transfer from the given keypair to bob with the value of 999 and tx id of 99.
	#[cfg(test)]
	pub fn new_transfer(origin: primitives::Pair, id: TransactionId) -> Self {
		use primitives::testing;

		let call = runtime::OuterCall::Balances(runtime::balances::Call::Transfer(
			testing::bob().public(),
			999,
		));
		let signed_call = call.using_encoded(|payload| origin.sign(payload));
		Self::new(id, call, origin.public(), signed_call)
	}

	/// A test transfer from the given keypair to bob with the value of 999 and tx id of 99.
	#[cfg(test)]
	pub fn new_transfer_to(origin: primitives::Pair, dest: primitives::AccountId) -> Self {
		const ID: u32 = 99;

		let call = runtime::OuterCall::Balances(runtime::balances::Call::Transfer(dest, 999));
		let signed_call = call.using_encoded(|payload| origin.sign(payload));
		Self::new(ID, call, origin.public(), signed_call)
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

impl From<MessagePayload> for Message {
	fn from(p: MessagePayload) -> Self {
		Self::new_from_thread(p)
	}
}

/// The type of the tasks that the master can order the worker to do.
#[derive(Debug, Clone)]
pub enum TaskType {
	/// Authoring a new block.
	Authoring,
	/// Validating a block.
	Validating,
}

#[derive(Debug, Clone)]
pub enum MessagePayload {
	/// Data needed to finalize the setup of the worker.
	FinalizeSetup(BTreeMap<ThreadId, Sender<Message>>),
	/// Payload to indicate the type of task.
	Task(TaskType),
	/// Execute this transaction.
	///
	/// This message has no response; If the thread never respond back, then the master can assume
	/// that it has been executed by the thread.
	Transaction(Transaction),
	/// Initial transactions that the master distributed to the worker are done.
	///
	/// This can be used in both authoring and validation phase. It only implies that the worker
	/// should not wait for any other `Transaction` message types henceforth.
	TransactionDistributionDone,
	/// The outcome report of the initial phase of the authoring phase.
	///
	/// First inner values are the _executed_, _forwarded_ and _orphaned_ count respectively.
	AuthoringReport(usize, usize),
	/// Same as `AuthoringReport`, but for validation phase.
	///
	/// There is no need to report back anything, just tell the master that you are done.
	ValidationReport,
	/// Report the execution of a transaction by a worker back to master.
	///
	/// This should only be used if the thread executing a transaction is not the original owner of
	/// the transaction.
	WorkerExecuted(TransactionId),
	/// Report an orphan transaction back to the master.
	WorkerOrphan(TransactionId),
	/// Master is signaling the end of the task.
	TaskDone,
	/// Master is signaling the termination of the thread.
	///
	/// This should be followed by the worker thread exiting and the master Joining.
	Terminate,
	/// An arbitrary payload of bytes for testing.
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

pub mod transaction_generator {
	use super::*;
	use primitives::testing::*;
	use primitives::*;
	use runtime::balances::BalanceOf;

	/// Build a transfer from `origin` to `to`.
	///
	/// Note that the id must be unique from the call site.
	pub fn build_transfer(id: TransactionId, origin: Pair, to: Public) -> Transaction {
		use runtime::balances::Call;
		let call = OuterCall::Balances(Call::Transfer(to, 10));
		let signature = call.using_encoded(|payload| origin.sign(payload));
		Transaction::new(id, call, origin.public(), signature)
	}

	/// Give `who` a large amount of balance.
	pub fn endow_account<R: runtime::ModuleRuntime>(who: Public, rt: &R, amount: u128) {
		<BalanceOf<R>>::write(&rt, who, amount.into()).unwrap()
	}

	/// build `count` random transfers.
	pub fn random_transfers(count: usize) -> Vec<Transaction> {
		(0..count)
			.map(|c| {
				let sender = testing::random();
				let recipient = testing::random().public();
				build_transfer(c as TransactionId, sender, recipient)
			})
			.collect::<Vec<_>>()
	}

	pub fn simple_alice_bob_dave() -> Vec<Transaction> {
		let tx1 = build_transfer(1, alice(), bob().public());
		let tx2 = build_transfer(2, alice(), dave().public());

		vec![tx1, tx2]
	}

	/// Build a bank test example with `member` accounts sending `transfers` transfers between them.
	///
	/// This is pretty useful to demonstrate a common use case with orphans as well.
	///
	/// Accounts will be endowed.
	pub fn bank(members: usize, transfers: usize) -> (Vec<Transaction>, Vec<AccountId>) {
		use rand::seq::SliceRandom;

		let accounts = (0..members).map(|_| testing::random()).collect::<Vec<_>>();

		let txs = (0..transfers)
			.map(|i| {
				let from = accounts.choose(&mut rand::thread_rng()).unwrap();
				let to = accounts.choose(&mut rand::thread_rng()).unwrap();

				let call = runtime::OuterCall::Balances(runtime::balances::Call::Transfer(
					to.public(),
					100,
				));

				let signed_call = call.using_encoded(|payload| from.sign(payload));
				Transaction::new(i as TransactionId, call, from.public(), signed_call)
			})
			.collect::<Vec<Transaction>>();

		let accounts = accounts
			.into_iter()
			.map(|acc| acc.public())
			.collect::<Vec<_>>();
		(txs, accounts)
	}
}
