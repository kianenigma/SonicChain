use executor::{concurrent::*, types::*, *};
use primitives::*;
use rand::seq::SliceRandom;
use runtime::*;
use tx_distribution::ConnectedComponentsDistributer;

const LOG_TARGET: &'static str = "benchmarks";

// TODO: Timings are not super accurate.
// TODO: more complicated modules: multiSig is fun?
// FIXME: print a report of the outcomes?
// TODO: I wish I could impl Drop for Master and join() + terminate all workers..

pub mod datasets {
	use super::*;
	use parity_scale_codec::Encode;
	use runtime::staking;
	use types::transaction_generator::*;

	pub fn millionaires_playground<R: ModuleRuntime>(
		rt: &R,
		members: usize,
		transfers: usize,
	) -> Vec<Transaction> {
		logging::log!(
			info,
			"Generating millionaires_playground({}, {})",
			members,
			transfers,
		);
		const AMOUNT: Balance = 100_000_000_000;

		let (transactions, accounts) = bank(members, transfers);
		accounts
			.into_iter()
			.for_each(|acc| endow_account(acc, rt, AMOUNT));
		transactions
	}

	pub fn world_of_stakers<R: ModuleRuntime>(
		rt: &R,
		validators: usize,
		nominators: usize,
		random: usize,
	) -> Vec<Transaction> {
		logging::log!(
			info,
			"Generating world_of_stakers({}, {})",
			validators,
			nominators,
		);
		const AMOUNT: Balance = 100_000_000_000;
		const BOND: Balance = 1000_000;

		let validators = (0..validators)
			.map(|_| (testing::random(), testing::random()))
			.collect::<Vec<_>>();
		let nominators = (0..nominators)
			.map(|_| (testing::random(), testing::random()))
			.collect::<Vec<_>>();

		validators
			.iter()
			.chain(nominators.iter())
			.for_each(|(stash, ctrl)| {
				endow_account(stash.public(), rt, AMOUNT);
				endow_account(ctrl.public(), rt, AMOUNT);
			});

		let mut transactions = vec![];
		validators.iter().for_each(|(stash, ctrl)| {
			// sign and submit a bond
			let inner_call = staking::Call::TxBond(BOND, ctrl.public());
			let id = rand::random::<TransactionId>();
			let call = OuterCall::Staking(inner_call);
			let signed_call = call.using_encoded(|bytes| stash.sign(bytes));
			let tx = Transaction::new(id, call, stash.public(), signed_call);
			transactions.push(tx);

			// sign and submit a validate.
			let inner_call = staking::Call::TxValidate();
			let id = rand::random::<TransactionId>();
			let call = OuterCall::Staking(inner_call);
			let signed_call = call.using_encoded(|bytes| ctrl.sign(bytes));
			let tx = Transaction::new(id, call, ctrl.public(), signed_call);
			transactions.push(tx);
		});

		nominators.iter().for_each(|(stash, ctrl)| {
			// sign and submit a bond
			let inner_call = staking::Call::TxBond(BOND, ctrl.public());
			let id = rand::random::<TransactionId>();
			let call = OuterCall::Staking(inner_call);
			let signed_call = call.using_encoded(|bytes| stash.sign(bytes));
			let tx = Transaction::new(id, call, stash.public(), signed_call);
			transactions.push(tx);

			// sign and submit a nominate.
			let votes = validators
				.choose_multiple(&mut rand::thread_rng(), 4)
				.map(|(s, _)| s.public())
				.collect::<Vec<_>>();
			let inner_call = staking::Call::TxNominate(votes);
			let id = rand::random::<TransactionId>();
			let call = OuterCall::Staking(inner_call);
			let signed_call = call.using_encoded(|bytes| ctrl.sign(bytes));
			let tx = Transaction::new(id, call, ctrl.public(), signed_call);
			transactions.push(tx);
		});

		let all_accounts = validators
			.into_iter()
			.chain(nominators.into_iter())
			.collect::<Vec<_>>();
		(0..random).for_each(|_| {
			match rand::random::<u8>() % 4 {
				0 => {
					// a random account unbonds.
					let (stash, _) = all_accounts.choose(&mut rand::thread_rng()).unwrap();
					let inner_call = staking::Call::TxUnbond(BOND / 2);
					let id = rand::random::<TransactionId>();
					let call = OuterCall::Staking(inner_call);
					let signed_call = call.using_encoded(|bytes| stash.sign(bytes));
					let tx = Transaction::new(id, call, stash.public(), signed_call);
					transactions.push(tx);
				}
				1 => {
					// a random account chills.
					let (_, ctrl) = all_accounts.choose(&mut rand::thread_rng()).unwrap();
					let inner_call = staking::Call::TxChill();
					let id = rand::random::<TransactionId>();
					let call = OuterCall::Staking(inner_call);
					let signed_call = call.using_encoded(|bytes| ctrl.sign(bytes));
					let tx = Transaction::new(id, call, ctrl.public(), signed_call);
					transactions.push(tx);
				}
				2 => {
					// a random account bonds-extra.
					let (stash, _) = all_accounts.choose(&mut rand::thread_rng()).unwrap();
					let inner_call = staking::Call::TxBondExtra(BOND);
					let id = rand::random::<TransactionId>();
					let call = OuterCall::Staking(inner_call);
					let signed_call = call.using_encoded(|bytes| stash.sign(bytes));
					let tx = Transaction::new(id, call, stash.public(), signed_call);
					transactions.push(tx);
				}
				3 => {
					// a random account sets controller.
					let (stash, _) = all_accounts.choose(&mut rand::thread_rng()).unwrap();
					let inner_call = staking::Call::TxSetController(testing::random().public());
					let id = rand::random::<TransactionId>();
					let call = OuterCall::Staking(inner_call);
					let signed_call = call.using_encoded(|bytes| stash.sign(bytes));
					let tx = Transaction::new(id, call, stash.public(), signed_call);
					transactions.push(tx);
				}
				_ => unreachable!(),
			}
		});

		transactions
	}
}

fn sequential() {
	let mut executor = sequential::SequentialExecutor::new();
	let dataset = datasets::millionaires_playground(&executor.runtime, 10_000, 5_000);
	// let dataset = datasets::world_of_stakers(&executor.runtime, 200, 1_000, 5_000);

	let start = std::time::Instant::now();
	executor.author_block(dataset);
	println!("Seq authoring took {:?}", start.elapsed());
}

fn concurrent<D: tx_distribution::Distributer>() {
	let mut executor = concurrent::ConcurrentExecutor::<Pool, D>::new(4, false, None);
	let dataset = datasets::millionaires_playground(&executor.master.runtime, 10_000, 5_000);
	// let dataset = datasets::world_of_stakers(&executor.master.runtime, 200, 1_000, 5_000);

	let start = std::time::Instant::now();
	executor.author_block(dataset);
	println!("Concurrent authoring took {:?}", start.elapsed());
}

fn main() {
	logging::init_logger();

	concurrent::<ConnectedComponentsDistributer>();
	std::thread::sleep(std::time::Duration::from_secs(4));
	sequential();
}
