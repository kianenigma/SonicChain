use crate::log_target;
use crate::pool::TransactionPool;
use crate::types::{Transaction, TransactionStatus};
use primitives::*;
use runtime::MasterRuntime;
use std::collections::HashMap;

/// Something that can distribute transactions into buckets, each assigned to a thread.
pub trait Distributer {
	/// Distribute the given transactions among the given thread ids in some arbitrary way.
	///
	/// This will receive a mutable reference to the pool and will also update the `status` of each
	/// transaction to `Done(_)`.
	fn distribute<P: TransactionPool<Transaction>>(
		runtime: &MasterRuntime,
		worker_ids: &[ThreadId],
		txs: &mut P,
	);
}

/// A dumb, naive, round robin transaction distributer.
pub struct RoundRobin;

impl Distributer for RoundRobin {
	fn distribute<P: TransactionPool<Transaction>>(
		_: &MasterRuntime,
		worker_ids: &[ThreadId],
		pool: &mut P,
	) {
		let num_workers = worker_ids.len();

		pool.iter_mut().enumerate().for_each(|(idx, tx)| {
			let assigned_worker = worker_ids[idx % num_workers];
			tx.status = TransactionStatus::Done(assigned_worker);
		});
	}
}

/// A storage aware transaction distributer.
pub struct SimpleStorageAwareDistributer;

impl Distributer for SimpleStorageAwareDistributer {
	fn distribute<P: TransactionPool<Transaction>>(
		runtime: &MasterRuntime,
		worker_ids: &[ThreadId],
		txs: &mut P,
	) {
		if worker_ids.len() == 0 {
			panic!("Cannot assign to no worker threads.");
		}

		// FIXME: this is accurate for now, but a very bad piece of code..
		// 1. Give the maximum number of transactions to each thread, except for the last one.
		for next_worker in worker_ids.iter().take(worker_ids.len() - 1) {
			// Find the number of times each key is accessed in all leftover transactions.
			let mut key_read_count: HashMap<primitives::Key, (usize, Vec<TransactionId>)> =
				HashMap::new();
			txs.iter()
				.filter(|tx| tx.status == TransactionStatus::NotExecuted)
				.for_each(|tx| {
					let keys = runtime
						.validate(&tx.function, tx.signature.0)
						.expect("Validation must work; qed.");
					keys.into_iter().for_each(|k| {
						key_read_count
							.entry(k)
							.and_modify(|(c, l)| {
								*c += 1;
								l.push(tx.id);
							})
							.or_insert((1, vec![tx.id]));
					})
				});

			if key_read_count.len() == 0 {
				log_target!(
					"tx-distribution",
					warn,
					"Early exiting from the distribution phase. Some threads will probably have no work."
				);
				return;
			}

			// Key that has the maximum access.
			let (max_key, (_, tx_ids)) = key_read_count
				.iter()
				.max_by_key(|(_, c)| *c)
				.expect("Non-empty iterator will always have a max");

			log_target!(
				"tx-distribution",
				trace,
				"Assigning {:?} transactions to a thread based on the common key {:?}",
				tx_ids.len(),
				max_key,
			);

			// assign all transactions that access this key to the next available thread.
			tx_ids.into_iter().for_each(|id| {
				let (_, tx) = txs
					.get_mut(|t| t.id == *id)
					.expect("Transaction must exist in the pool");
				debug_assert_eq!(tx.status, TransactionStatus::NotExecuted);
				tx.status = TransactionStatus::Done(*next_worker);
			});
		}

		// 2. assign anything left to the last thread.
		let last_worker = worker_ids
			.last()
			.expect("worker ids is checked to have positive length");
		txs.iter_mut()
			.filter(|tx| tx.status == TransactionStatus::NotExecuted)
			.for_each(|t| t.status = TransactionStatus::Done(*last_worker));
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::pool::VecPool;
	use crate::types::{Transaction, TransactionStatus};
	use primitives::testing;

	#[test]
	fn round_robin_works() {
		let mut pool = <VecPool<Transaction>>::new();

		pool.push_back(Transaction::new_transfer(testing::alice()));
		pool.push_back(Transaction::new_transfer(testing::bob()));
		pool.push_back(Transaction::new_transfer(testing::dave()));

		let ids = vec![1, 2];

		RoundRobin::distribute(&Default::default(), ids.as_ref(), &mut pool);

		assert_eq!(
			pool.get(|t| t.signature.0 == testing::alice().public())
				.unwrap()
				.1
				.status,
			TransactionStatus::Done(1)
		);

		assert_eq!(
			pool.get(|t| t.signature.0 == testing::bob().public())
				.unwrap()
				.1
				.status,
			TransactionStatus::Done(2)
		);

		assert_eq!(
			pool.get(|t| t.signature.0 == testing::dave().public())
				.unwrap()
				.1
				.status,
			TransactionStatus::Done(1)
		);
	}
}
