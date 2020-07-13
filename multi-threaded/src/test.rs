#![cfg(test)]

use crate::types::Transaction;
use crate::*;
use logging::init_logger;
use parity_scale_codec::Encode;
use primitives::testing::*;

#[test]
fn concurrent_executor_new_works() {
	let executor = ConcurrentExecutor::<Pool, RoundRobin>::new(4, true, None);
	let master = executor.master;
	std::thread::sleep(std::time::Duration::from_millis(500));
	assert_eq!(master.workers.len(), 4);

	master.run_test();

	assert!(master.join_all().is_ok())
}

#[test]
fn orphan_example() {
	init_logger();
	use runtime::balances::*;
	use runtime::*;

	let build_transfer = |id: TransactionId, origin: Pair, to: Public| -> Transaction {
		let call = OuterCall::Balances(Call::Transfer(to, 10));
		let signature = call.using_encoded(|payload| origin.sign(payload));
		Transaction::new(id, call, origin.public(), signature)
	};

	// alice -> bob, will assigned to first worker.
	let tx1 = build_transfer(100, alice(), bob().public());
	// eve -> dave, will be assigned to second worker.
	let tx2 = build_transfer(101, eve(), dave().public());
	// bob -> dave, will be assigned to third worker.
	let tx3 = build_transfer(102, bob(), dave().public());

	let executor = ConcurrentExecutor::<Pool, RoundRobin>::new(3, false, None);
	let mut master = executor.master;
	std::thread::sleep(std::time::Duration::from_millis(200));
	assert_eq!(master.workers.len(), 3);

	// give them some balance.
	BalanceOf::write(&master.runtime, alice().public(), 1000.into()).unwrap();
	BalanceOf::write(&master.runtime, bob().public(), 1000.into()).unwrap();
	BalanceOf::write(&master.runtime, dave().public(), 1000.into()).unwrap();
	BalanceOf::write(&master.runtime, eve().public(), 1000.into()).unwrap();

	master.tx_pool.push_back(tx1);
	master.tx_pool.push_back(tx2);
	master.tx_pool.push_back(tx3);

	master.run_author();

	let alice_balance = BalanceOf::read(&master.runtime, alice().public())
		.unwrap()
		.free();
	let bob_balance = BalanceOf::read(&master.runtime, bob().public())
		.unwrap()
		.free();
	let dave_balance = BalanceOf::read(&master.runtime, dave().public())
		.unwrap()
		.free();
	let eve_balance = BalanceOf::read(&master.runtime, eve().public())
		.unwrap()
		.free();

	assert_eq!(alice_balance, 990);
	assert_eq!(eve_balance, 990); // 980
	assert_eq!(bob_balance, 1000); // 1100
	assert_eq!(dave_balance, 1020);

	assert!(master.join_all().is_ok());
}

macro_rules! bank_test_with_distribution {
	($( $distribution:ty, $name:ident ,)*) => {
		$(
			#[test]
			fn $name() {
				init_logger();
				use rand::seq::SliceRandom;
				use runtime::balances::BalanceOf;
				use runtime::MasterRuntime;

				const NUM_ACCOUNTS: usize = 10;
				const NUM_TXS: usize = 1_000;

				let accounts = (0..NUM_ACCOUNTS).map(|_| random()).collect::<Vec<_>>();
				let transfers = (0..NUM_TXS)
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

				let executor = ConcurrentExecutor::<Pool, $distribution>::new(4, false, None);
				let mut master = executor.master;
				std::thread::sleep(std::time::Duration::from_millis(500));
				assert_eq!(master.workers.len(), 4);

				// fund all the accounts with quite some money.
				accounts.iter().for_each(|acc| {
					<BalanceOf<MasterRuntime>>::write(
						&master.runtime,
						acc.public(),
						1_000_000_000_000.into(),
					)
					.unwrap()
				});

				// push all txs to master
				transfers
					.iter()
					.for_each(|t| master.tx_pool.push_back(t.clone()));

				// start the master as well.
				master.run_author();

				// assert to join all okay
				assert!(master.join_all().is_ok())
			}
		)*
	}
}

bank_test_with_distribution!(RoundRobin, bank_round_robin,);
