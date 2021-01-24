use crate::*;
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
		"🖨  Generating millionaires_playground({}, {})",
		members,
		transfers,
	);
	const AMOUNT: Balance = 100_000_000_000;

	let (transactions, accounts) = bank(members, transfers, 1000);
	accounts
		.into_iter()
		.for_each(|acc| endow_account(acc, rt, AMOUNT));
	transactions
}

pub fn middle_class_playground<R: ModuleRuntime>(
	rt: &R,
	members: usize,
	transfers: usize,
	transfer_amount: Balance,
	lucky_members: usize,
) -> Vec<Transaction> {
	assert!(lucky_members <= members);
	logging::log!(
		info,
		"🖨 Generating middle_class_playground({}, {}, {}, {})",
		members,
		transfers,
		transfer_amount,
		lucky_members,
	);

	let (transactions, accounts) = bank(members, transfers, transfer_amount);
	accounts.into_iter().enumerate().for_each(|(idx, acc)| {
		if idx < lucky_members {
			// will have a lot.
			endow_account(acc, rt, transfer_amount * 1000_000)
		} else {
			// will have half.
			endow_account(acc, rt, transfer_amount / 2)
		}
	});
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
