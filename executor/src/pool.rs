use crate::types;
use std::iter::*;

/// An ordered. transaction pool.
pub trait TransactionPool<Tx>: From<Vec<Tx>> {
	/// Create a new transaction pool.
	fn new() -> Self;

	/// Create a new transaction pool with the given capacity.
	fn with_capacity(size: usize) -> Self;

	/// Insert a new transaction into the pool.
	///
	/// This will add the transaction to the end of the pool.
	fn push_back(&mut self, tx: Tx);

	/// Insert a batch of transactions
	///
	/// This will add all of the transactions to the end of the pool.
	fn push_batch(&mut self, txs: &[Tx]);

	/// Insert a new transaction at the given index of the pool.
	///
	/// Panics if index > len.
	fn insert(&mut self, index: usize, tx: Tx);

	/// Remove the first element that matches the criteria from the pool, returning the removed
	/// element.
	///
	/// Will return None if no element matches.
	fn remove(&mut self, search: impl Fn(&Tx) -> bool) -> Option<Tx> {
		// NOTE: this will not work with .map(). Interesting, neh? closure seemingly consumes self
		// or something of that sort.
		match self.get(search) {
			Some((index, _)) => Some(self.remove_at(index)),
			None => None,
		}
	}

	/// Remove an element at a specific index, returning the removed element.
	///
	/// Will panic if index is out of bound.
	fn remove_at(&mut self, at: usize) -> Tx;

	/// Remove all.
	fn clear(&mut self);

	/// Current count of transaction
	fn len(&self) -> usize;

	/// Get a clone of all of the transactions in the pool, in the correct order.
	fn all(&self) -> Vec<Tx>;

	/// Consume self and get a clone of all of the transactions in the pool, in the correct order.
	fn destruct(self) -> Vec<Tx>;

	/// Get a transaction with the given criteria.
	///
	/// It returns a reference to the given transaction, and its index.
	fn get(&self, search: impl Fn(&Tx) -> bool) -> Option<(usize, &Tx)>;

	/// Get a transaction with the given criteria.
	///
	/// It returns a mutable reference to the given transaction, and its index.
	fn get_mut(&mut self, search: impl Fn(&Tx) -> bool) -> Option<(usize, &mut Tx)>;

	/// Iterate without consuming.
	fn iter<'a>(&'a self) -> PoolIter<'a, Tx>;

	/// Mutable iterator without consuming.
	fn iter_mut<'a>(&'a mut self) -> PoolIterMut<'a, Tx>;
}

/// A transaction pool implemented by a vector.
#[derive(Default, Debug)]
pub struct VecPool<Tx> {
	inner: Vec<Tx>,
}

impl<Tx> From<Vec<Tx>> for VecPool<Tx> {
	fn from(v: Vec<Tx>) -> Self {
		Self { inner: v }
	}
}

impl<Tx> IntoIterator for VecPool<Tx> {
	type IntoIter = std::vec::IntoIter<Tx>;
	type Item = Tx;
	fn into_iter(self) -> Self::IntoIter {
		self.inner.into_iter()
	}
}

pub struct PoolIter<'a, Tx>(std::slice::Iter<'a, Tx>);

impl<'a, Tx> IntoIterator for &'a VecPool<Tx> {
	type IntoIter = PoolIter<'a, Tx>;
	type Item = &'a Tx;

	fn into_iter(self) -> Self::IntoIter {
		PoolIter(self.inner.iter())
	}
}

impl<'a, Tx> Iterator for PoolIter<'a, Tx> {
	type Item = &'a Tx;

	fn next(&mut self) -> Option<Self::Item> {
		self.0.next()
	}
}

pub struct PoolIterMut<'a, Tx>(std::slice::IterMut<'a, Tx>);

impl<'a, Tx> IntoIterator for &'a mut VecPool<Tx> {
	type IntoIter = PoolIterMut<'a, Tx>;
	type Item = &'a mut Tx;

	fn into_iter(self) -> Self::IntoIter {
		PoolIterMut(self.inner.iter_mut())
	}
}

impl<'a, Tx> Iterator for PoolIterMut<'a, Tx> {
	type Item = &'a mut Tx;

	fn next(&mut self) -> Option<Self::Item> {
		self.0.next()
	}
}

impl<Tx: Clone + types::VerifiableTransaction> TransactionPool<Tx> for VecPool<Tx> {
	fn new() -> Self {
		Self {
			inner: Default::default(),
		}
	}

	fn with_capacity(size: usize) -> Self {
		Self {
			inner: Vec::with_capacity(size),
		}
	}

	fn push_back(&mut self, tx: Tx) {
		if !tx.verify() {
			panic!("Attempted to insert invalid transaction to the pool.")
		}
		self.inner.push(tx);
	}

	fn push_batch(&mut self, tx: &[Tx]) {
		tx.into_iter().for_each(|t| self.push_back(t.clone()));
	}

	fn insert(&mut self, index: usize, tx: Tx) {
		if !tx.verify() {
			panic!("Attempted to insert invalid transaction to the pool.")
		}
		self.inner.insert(index, tx);
	}

	fn remove_at(&mut self, at: usize) -> Tx {
		self.inner.remove(at)
	}

	fn clear(&mut self) {
		self.inner.clear();
	}

	fn len(&self) -> usize {
		self.inner.len()
	}

	fn all(&self) -> Vec<Tx> {
		self.inner.clone()
	}

	fn destruct(self) -> Vec<Tx> {
		self.inner
	}

	fn get(&self, search: impl Fn(&Tx) -> bool) -> Option<(usize, &Tx)> {
		self.inner.iter().enumerate().find(|(_, tx)| search(tx))
	}

	fn get_mut(&mut self, search: impl Fn(&Tx) -> bool) -> Option<(usize, &mut Tx)> {
		self.inner.iter_mut().enumerate().find(|(_, tx)| search(tx))
	}

	fn iter<'a>(&'a self) -> PoolIter<'a, Tx> {
		self.into_iter()
	}

	fn iter_mut<'a>(&'a mut self) -> PoolIterMut<'a, Tx> {
		self.into_iter()
	}
}

#[cfg(test)]
mod vecpool_tests {
	use super::*;

	#[derive(Clone, Eq, PartialEq, Debug)]
	struct TestTransaction {
		id: u32,
		signature: u32,
	}

	impl TestTransaction {
		fn new(id: u32, signature: u32) -> Self {
			Self { id, signature }
		}
	}

	impl types::VerifiableTransaction for TestTransaction {
		fn verify(&self) -> bool {
			self.id % 2 == 0
		}
	}

	type Pool = VecPool<TestTransaction>;

	#[test]
	fn get_works() {
		let mut pool = Pool::new();

		for i in (0..10).step_by(2) {
			pool.push_back(TestTransaction::new(i, i));
		}

		assert_eq!(
			pool.get(|t| t.id == 8),
			Some((
				4,
				&TestTransaction {
					id: 8,
					signature: 8
				}
			)),
		)
	}

	#[test]
	fn get_mut_works() {
		let mut pool = Pool::new();

		for i in (0..10).step_by(2) {
			pool.push_back(TestTransaction::new(i, i));
		}

		let (_, mut tx_mut) = pool.get_mut(|t| t.id == 8).unwrap();
		tx_mut.signature = 999;

		let (_, tx) = pool.get(|t| t.id == 8).unwrap();
		assert_eq!(tx.signature, 999);
	}

	#[test]
	fn iter_works() {
		let mut pool = Pool::new();

		for i in (0..6).step_by(2) {
			pool.push_back(TestTransaction::new(i, i));
		}

		let mut iter = pool.iter();
		assert_eq!(iter.next().map(|t| t.id), Some(0));
		assert_eq!(iter.next().map(|t| t.id), Some(2));
		assert_eq!(iter.next().map(|t| t.id), Some(4));
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn into_iter_works() {
		let mut pool = Pool::new();

		for i in (0..6).step_by(2) {
			pool.push_back(TestTransaction::new(i, i));
		}

		let mut iter = pool.into_iter();
		assert_eq!(iter.next().map(|t| t.id), Some(0));
		assert_eq!(iter.next().map(|t| t.id), Some(2));
		assert_eq!(iter.next().map(|t| t.id), Some(4));
		assert_eq!(iter.next(), None);
	}

	#[test]
	fn iter_mut_works() {
		let mut pool = Pool::new();

		for i in (0..10).step_by(2) {
			pool.push_back(TestTransaction::new(i, i));
		}

		pool.iter_mut().for_each(|mut t| t.signature += 1);
		(0..10)
			.step_by(2)
			.for_each(|i| assert_eq!(pool.get(|t| t.id == i).unwrap().1.signature, i + 1))
	}

	#[test]
	#[should_panic(expected = "Attempted to insert invalid transaction to the pool.")]
	fn insert_invalid_will_fail() {
		let mut pool = Pool::new();
		pool.push_back(TestTransaction::new(1, 0));
	}

	#[test]
	#[should_panic(expected = "Attempted to insert invalid transaction to the pool.")]
	fn insert_invalid_will_fail_2() {
		let mut pool = Pool::new();
		pool.insert(0, TestTransaction::new(1, 0));
	}
}
