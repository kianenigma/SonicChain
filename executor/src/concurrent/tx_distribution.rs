use crate::{
	pool::TransactionPool,
	types::{Transaction, TransactionStatus},
};
use primitives::*;
use runtime::SequentialRuntime;
use std::matches;

pub use node::ConnectedComponentsDistributer;

const LOG_TARGET: &'static str = "tx-dist";

/// Something that can distribute transactions into buckets, each assigned to a thread.
pub trait Distributer {
	/// Distribute the given transactions among the given thread ids in some arbitrary way.
	///
	/// This will receive a mutable reference to the pool and will also update the `status` of each
	/// transaction to `Done(_)`.
	fn distribute<P: TransactionPool<Transaction>>(
		runtime: &SequentialRuntime,
		worker_ids: &[ThreadId],
		txs: &mut P,
	);
}

pub mod node {
	#![allow(dead_code)]
	use super::*;
	use std::{
		cell::RefCell,
		rc::{Rc, Weak},
	};

	type NodeCell = RefCell<Node>;

	#[derive(Eq, PartialEq, Clone, Debug)]
	enum NodeType {
		Tx(TransactionId),
		Key(StateKey),
	}

	#[derive(Clone, Debug)]
	struct Node {
		node_type: NodeType,
		/// Points to key if node type is tx.
		///
		/// This is an Rc.
		keys: Option<Vec<Rc<NodeCell>>>,
		/// Points to tx if node type is key.
		///
		/// This is a Weak.
		txs: Option<Vec<Weak<NodeCell>>>,
		/// Id of the component to which this node belongs to.
		component: Option<u32>,
	}

	impl Node {
		fn new(node_type: NodeType) -> Self {
			Self {
				node_type,
				keys: Default::default(),
				txs: Default::default(),
				component: None,
			}
		}

		fn new_tx(tx: TransactionId, keys: Vec<Rc<NodeCell>>) -> Self {
			Self {
				node_type: NodeType::Tx(tx),
				keys: Some(keys),
				txs: None,
				component: None,
			}
		}

		fn new_key(key: StateKey, txs: Vec<Weak<NodeCell>>) -> Self {
			Self {
				node_type: NodeType::Key(key),
				txs: Some(txs),
				keys: None,
				component: None,
			}
		}

		fn is_key_node(&self) -> bool {
			matches!(self.node_type, NodeType::Key(_))
		}

		fn is_tx_node(&self) -> bool {
			matches!(self.node_type, NodeType::Tx(_))
		}

		fn add_key(&mut self, key: Rc<NodeCell>) {
			debug_assert!(self.is_tx_node());
			self.keys.as_mut().unwrap().push(key);
		}

		fn add_tx(&mut self, tx: Weak<NodeCell>) {
			debug_assert!(self.is_key_node());
			self.txs.as_mut().unwrap().push(tx)
		}

		fn as_rc(self) -> Rc<NodeCell> {
			Rc::from(RefCell::new(self))
		}

		fn visit(&mut self, id: u32) -> bool {
			match self.component {
				None => {
					self.component = Some(id);
					true
				}
				_ => false,
			}
		}

		fn traverse(start: Rc<NodeCell>, component_id: u32) -> Vec<Rc<NodeCell>> {
			// debug_assert!(matches!(start.borrow().node_type, NodeType::Tx(_)));

			let mut queue = vec![start];
			let mut component = vec![];

			while !queue.is_empty() {
				let current = queue.remove(0);

				// mark visit the current node.
				{
					let mut current_borrow = current.borrow_mut();
					if current_borrow.visit(component_id) && current_borrow.is_tx_node() {
						component.push(Rc::clone(&current));
					}
				}

				let current = current.borrow();
				match current.node_type {
					NodeType::Key(_) => {
						debug_assert!(current.keys.is_none());
						current.txs.as_ref().unwrap().iter().for_each(|tx_weak| {
							let tx_rc = Rc::clone(&tx_weak.upgrade().unwrap());
							if tx_rc.borrow().component.is_none() {
								queue.push(tx_rc);
							}
						})
					}
					NodeType::Tx(_) => {
						debug_assert!(current.txs.is_none());
						current.keys.as_ref().unwrap().iter().for_each(|key_rc| {
							if key_rc.borrow().component.is_none() {
								queue.push(Rc::clone(key_rc))
							}
						})
					}
				}
			}

			component
		}
	}

	#[cfg(test)]
	mod test_node {
		use super::*;

		#[test]
		fn traverse_works() {
			//
			//  t1         t2
			//  |           |
			// 	|--- k1 ----|
			//  |--- k2 ----|
			let k1 = Node::new_key(vec![1, 1, 1].into(), vec![]).as_rc();
			let k2 = Node::new_key(vec![2, 2, 2].into(), vec![]).as_rc();
			let mut t1 = Node::new_tx(1, vec![]);
			let mut t2 = Node::new_tx(2, vec![]);

			t1.add_key(Rc::clone(&k1));
			t1.add_key(Rc::clone(&k2));

			t2.add_key(Rc::clone(&k1));
			t2.add_key(Rc::clone(&k2));

			let t1 = t1.as_rc();
			let t2 = t2.as_rc();

			k1.borrow_mut().add_tx(Rc::downgrade(&t1));
			k1.borrow_mut().add_tx(Rc::downgrade(&t2));

			// dbg!(k1.borrow().txs.as_ref().unwrap().first().unwrap().upgrade());

			k2.borrow_mut().add_tx(Rc::downgrade(&t1));
			k2.borrow_mut().add_tx(Rc::downgrade(&t2));

			let members = Node::traverse(k1.clone(), 1);

			assert_eq!(k1.borrow().component, Some(1));
			assert_eq!(k2.borrow().component, Some(1));

			assert_eq!(t2.borrow().component, Some(1));
			assert_eq!(t2.borrow().component, Some(1));

			assert_eq!(members.len(), 2);
			assert_eq!(
				members
					.iter()
					.map(|c| c.borrow().node_type.clone())
					.collect::<Vec<_>>(),
				vec![NodeType::Tx(1), NodeType::Tx(2)],
			)
		}
	}

	pub struct ConnectedComponentsDistributer;

	impl Distributer for ConnectedComponentsDistributer {
		fn distribute<P: TransactionPool<Transaction>>(
			runtime: &SequentialRuntime,
			worker_ids: &[ThreadId],
			txs: &mut P,
		) {
			use std::collections::BTreeMap;
			let mut keys_map: BTreeMap<StateKey, Rc<NodeCell>> = BTreeMap::new();

			let transaction_nodes = txs
				.iter()
				.map(|tx| {
					let keys = runtime.validate(&tx.function, tx.signature.0).unwrap();
					let tx_node = Node::new_tx(tx.id, Default::default()).as_rc();

					let keys = keys
						.into_iter()
						.map(|key| {
							let key_node = keys_map
								.entry(key.clone())
								.or_insert(Node::new_key(key, Default::default()).as_rc());
							// add this transaction to the key.
							key_node.borrow_mut().add_tx(Rc::downgrade(&tx_node));

							Rc::clone(key_node)
						})
						.collect::<Vec<_>>();

					for key in keys {
						tx_node.borrow_mut().add_key(key);
					}

					tx_node
				})
				.collect::<Vec<_>>();

			let mut components = transaction_nodes
				.iter()
				.enumerate()
				.map(|(id, tx_node)| Node::traverse(Rc::clone(tx_node), id as u32))
				.filter(|c| !c.is_empty())
				.collect::<Vec<_>>();

			debug_assert_eq!(
				components.iter().map(|x| x.len()).sum::<usize>(),
				txs.len(),
				"All transactions must be assigned to a component.",
			);

			let mut component_workers: BTreeMap<ThreadId, Vec<Rc<NodeCell>>> = BTreeMap::new();
			worker_ids.iter().for_each(|i| {
				component_workers.insert(*i, vec![]);
			});

			loop {
				// Get the largest component.
				let (index, largest_component) = components
					.iter()
					.enumerate()
					.max_by_key(|(_, x)| x.len())
					.map(|(i, x)| (i, x.clone()))
					.unwrap();
				// smallest thread group.
				let (k, v) = component_workers
					.iter_mut()
					.min_by_key(|(_, v)| v.len())
					.unwrap();

				logging::log!(
					debug,
					"assigning component with {} transactions to thread {}",
					largest_component.len(),
					k,
				);
				v.extend(largest_component);

				components.remove(index);
				if components.len() == 0 {
					break;
				}
			}

			debug_assert!(components.is_empty(), "All components must be drained.");
			debug_assert_eq!(
				component_workers
					.iter()
					.map(|(_, v)| v.len())
					.sum::<usize>(),
				txs.len(),
				"all transactions within the components must be assigned to a worker."
			);

			component_workers.iter().for_each(|(k, v)| {
				logging::log!(info, "Assigning {} transactions to thread {}", v.len(), k);
			});

			component_workers
				.into_iter()
				.for_each(|(worker, tx_nodes)| {
					tx_nodes
						.into_iter()
						.map(|tx| match tx.borrow().node_type {
							NodeType::Tx(id) => id,
							NodeType::Key(_) => panic!("Unexpected node type"),
						})
						.for_each(|tid| {
							let (_, tx) = txs.get_mut(|t| t.id == tid).unwrap();
							tx.status = TransactionStatus::Done(worker);
						})
				});

			debug_assert!(
				txs.iter()
					.all(|tx| matches!(tx.status, TransactionStatus::Done(_))),
				"All transactions inside the pool must have a Done(_) status.",
			)
		}
	}

	#[cfg(test)]
	mod test_distributer {
		use super::*;
		use crate::{types, Pool, State};

		#[test]
		fn it_works() {
			logging::init_logger();
			let (load, _) = types::transaction_generator::bank(50, 10);
			let mut pool = Pool::from(load);
			let state = State::default().as_arc();
			let rt = SequentialRuntime::new(state, 1);
			let workers = vec![1, 2, 3];

			// all the assertions are in the code.
			ConnectedComponentsDistributer::distribute(&rt, &workers, &mut pool);
		}
	}
}

/// A dumb, naive, round robin transaction distributer.
pub struct RoundRobin;

impl Distributer for RoundRobin {
	fn distribute<P: TransactionPool<Transaction>>(
		_: &SequentialRuntime,
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

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{
		pool::VecPool,
		types::{Transaction, TransactionStatus},
	};
	use primitives::testing;

	#[test]
	fn round_robin_works() {
		let mut pool = <VecPool<Transaction>>::new();

		pool.push_back(Transaction::new_transfer(testing::alice(), 1));
		pool.push_back(Transaction::new_transfer(testing::bob(), 2));
		pool.push_back(Transaction::new_transfer(testing::dave(), 3));

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
