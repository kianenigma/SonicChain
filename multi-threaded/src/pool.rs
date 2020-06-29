/// An ordered. transaction pool.
pub trait TransactionPool<Tx> {
	/// Create a new transaction pool.
	fn new() -> Self;

	/// Create a new transaction pool with the given capacity.
	fn with_capacity(size: usize) -> Self;

	/// Insert a new transaction into the pool.
	///
	/// This will add the transaction to the end of the pool.
	fn push_back(&mut self, tx: Tx);

	/// Insert a new transaction at the given index of the pool.
	///
	/// Panics if index > len.
	fn insert(&mut self, index: usize, tx: Tx);

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
}

/// A transaction pool implemented by a vector.
#[derive(Default, Debug)]
pub struct VecPool<Tx> {
	inner: Vec<Tx>,
}

impl<Tx: Clone> TransactionPool<Tx> for VecPool<Tx> {
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
		self.inner.push(tx);
	}

	fn insert(&mut self, index: usize, tx: Tx) {
		self.inner.insert(index, tx);
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

	type Pool = VecPool<TestTransaction>;

	#[test]
	fn get_works() {
		let mut pool = Pool::new();

		for i in 0..10 {
			pool.push_back(TestTransaction::new(i, i));
		}

		assert_eq!(
			pool.get(|t| t.id == 7),
			Some((
				7,
				&TestTransaction {
					id: 7,
					signature: 7
				}
			)),
		)
	}

	#[test]
	fn get_mut_works() {
		let mut pool = Pool::new();

		for i in 0..10 {
			pool.push_back(TestTransaction::new(i, i));
		}

		let (_, mut tx_mut) = pool.get_mut(|t| t.id == 7).unwrap();
		tx_mut.signature = 999;

		let (_, tx) = pool.get(|t| t.id == 7).unwrap();
		assert_eq!(tx.signature, 999);
	}
}
