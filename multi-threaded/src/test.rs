#![cfg(test)]

use crate::*;
use parity_scale_codec::{Decode, Encode};
use primitives::testing::*;

#[test]
fn spawn_threads_works() {
	let master = spawn_workers::<Pool, RoundRobin>(4, true);
	std::thread::sleep(std::time::Duration::from_millis(500));
	assert_eq!(master.workers.len(), 4);

	// unpark all workers. Then they will run their test_run automatically.
	master.unpark_all();

	// start the master as well.
	master.run_test();

	// assert to join all okay
	assert!(master.join_all().is_ok())
}

#[test]
fn orphan_example() {
	init_logger();
	use runtime::balances::*;
	use runtime::*;

	// alice -> bob, will assigned to first worker.
	let call = OuterCall::Balances(Call::Transfer(bob().public(), 10));
	let origin = alice();
	let signature = call.using_encoded(|payload| origin.sign(payload));
	let tx1 = crate::types::Transaction::new(100, call, origin.public(), signature);

	// eve -> dave, will be assigned to second worker.
	let call = OuterCall::Balances(Call::Transfer(dave().public(), 10));
	let origin = eve();
	let signature = call.using_encoded(|payload| origin.sign(payload));
	let tx2 = crate::types::Transaction::new(101, call, origin.public(), signature);

	// bob -> dave, will be assigned to third worker.
	let call = OuterCall::Balances(Call::Transfer(dave().public(), 10));
	let origin = bob();
	let signature = call.using_encoded(|payload| origin.sign(payload));
	let tx3 = crate::types::Transaction::new(102, call, origin.public(), signature);

	let mut master = spawn_workers::<Pool, RoundRobin>(3, false);
	std::thread::sleep(std::time::Duration::from_millis(500));
	assert_eq!(master.workers.len(), 3);

	// give them some balance.
	let alice_key = runtime::balances::BalanceOf::<WorkerRuntime>::key_for(alice().public());
	let eve_key = runtime::balances::BalanceOf::<WorkerRuntime>::key_for(eve().public());
	let bob_key = runtime::balances::BalanceOf::<WorkerRuntime>::key_for(bob().public());
	let dave_key = runtime::balances::BalanceOf::<WorkerRuntime>::key_for(dave().public());
	master
		.state
		.unsafe_insert_genesis_value(&alice_key, (1000 as Balance).encode().into());
	master
		.state
		.unsafe_insert_genesis_value(&eve_key, (1000 as Balance).encode().into());
	master
		.state
		.unsafe_insert_genesis_value(&bob_key, (1000 as Balance).encode().into());
	master
		.state
		.unsafe_insert_genesis_value(&dave_key, (1000 as Balance).encode().into());

	master.tx_pool.push_back(tx1);
	master.tx_pool.push_back(tx2);
	master.tx_pool.push_back(tx3);

	master.unpark_all();
	master.run();

	let alice_balance =
		<Balance as Decode>::decode(&mut &*master.state.unsafe_read_value(&alice_key).unwrap().0)
			.unwrap();
	let bob_balance =
		<Balance as Decode>::decode(&mut &*master.state.unsafe_read_value(&bob_key).unwrap().0)
			.unwrap();
	let dave_balance =
		<Balance as Decode>::decode(&mut &*master.state.unsafe_read_value(&dave_key).unwrap().0)
			.unwrap();
	let eve_balance =
		<Balance as Decode>::decode(&mut &*master.state.unsafe_read_value(&eve_key).unwrap().0)
			.unwrap();

	assert_eq!(alice_balance, 990);
	assert_eq!(eve_balance, 990);
	assert_eq!(bob_balance, 1000);
	assert_eq!(dave_balance, 1020);

	assert!(master.join_all().is_ok());
}

#[test]
fn bank() {
	init_logger();
	use rand::seq::SliceRandom;

	const NUM_ACCOUNTS: usize = 200;
	const NUM_TXS: usize = 1_000;

	let accounts = (0..NUM_ACCOUNTS).map(|_| random()).collect::<Vec<_>>();
	let transfers = (0..NUM_TXS)
		.map(|i| {
			let from = accounts.choose(&mut rand::thread_rng()).unwrap();
			let to = accounts.choose(&mut rand::thread_rng()).unwrap();

			let call =
				runtime::OuterCall::Balances(runtime::balances::Call::Transfer(to.public(), 777));
			let signed_call = call.using_encoded(|payload| from.sign(payload));
			Transaction::new(i as TransactionId, call, from.public(), signed_call)
		})
		.collect::<Vec<Transaction>>();

	let mut master = spawn_workers::<Pool, RoundRobin>(4, false);
	std::thread::sleep(std::time::Duration::from_millis(500));
	assert_eq!(master.workers.len(), 4);

	// push all txs to master
	transfers
		.iter()
		.for_each(|t| master.tx_pool.push_back(t.clone()));

	// unpark all workers.
	master.unpark_all();

	// start the master as well.
	master.run();

	// assert to join all okay
	assert!(master.join_all().is_ok())
}
