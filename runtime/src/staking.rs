use crate::{balances::BalanceOf, decl_storage_map, decl_tx, DispatchError, UnwrapStorageOp};
use parity_scale_codec::{Decode, Encode};
use primitives::*;

// TODO: if remaining is zero, then clean the dust.

const MODULE: &'static str = "staking";

/// A staker's role in the staking system.
#[derive(Encode, Decode, Debug, Clone, Eq, PartialEq)]
pub enum StakingRole {
	Validator,
	Nominator,
	Chilled,
}

impl Default for StakingRole {
	fn default() -> Self {
		StakingRole::Chilled
	}
}

/// The ledger of a staker.
#[derive(Encode, Decode, Default, Debug, Clone, Eq, PartialEq)]
pub struct StakingLedger {
	value: Balance,
	controller: AccountId,
	role: StakingRole,
}

// A mapping from stash accounts to ledgers.
decl_storage_map!(Ledger, "ledger_of", AccountId, StakingLedger);
// A mapping from controller accounts to stash.
decl_storage_map!(Bonded, "bonded", AccountId, AccountId);
// A mapping from stash to nominations.
decl_storage_map!(Nominations, "nominations", AccountId, Vec<AccountId>);

decl_tx! {
	fn tx_bond(rt, stash, amount: Balance, controller: AccountId) {
		// check already bonded.
		if Ledger::exists(rt, stash).or_forward()? {
			return Err(DispatchError::LogicError("Already bonded."));
		}

		// check enough balance and write balance.
		let mut stash_balance = BalanceOf::read(rt, stash).or_orphan()?;
		stash_balance.reserve(amount).map_err(|err| DispatchError::LogicError(err))?;
		BalanceOf::write(rt, stash, stash_balance).expect("Must be owned.");

		// write bonded
		Bonded::write(rt, controller.clone(), stash).or_orphan()?;

		// write ledger.
		let ledger = StakingLedger { controller, value: amount, ..Default::default() };
		Ledger::write(rt, stash, ledger).expect("Must be owned.");

		Ok(())
	}

	fn tx_bond_extra(rt, stash, amount: Balance) {
		let mut ledger = Ledger::read(rt, stash).or_forward()?;
		if ledger == StakingLedger::default() {
			return Err(DispatchError::LogicError("Not bonded."));
		}

		// note the second argument, first read can fail, second will cause orphan.
		let mut balance = BalanceOf::read(rt, stash).or_orphan()?;
		balance.reserve(amount).map_err(|err| DispatchError::LogicError(err))?;
		BalanceOf::write(rt, stash, balance).expect("Must be owned.");

		ledger.value += amount;
		Ledger::write(rt, stash, ledger).expect("Must be owned.");

		Ok(())
	}

	fn tx_unbond(rt, stash, amount: Balance) {
		let mut ledger = Ledger::read(rt, stash).or_forward()?;

		if ledger == StakingLedger::default() {
			return Err(DispatchError::LogicError("Not bonded."));
		}

		if ledger.value < amount {
			return Err(DispatchError::LogicError("Too much unbonding."));
		}

		ledger.value = ledger.value.checked_sub(amount).expect("Must have enough bonded.");

		// update the ledger.
		Ledger::write(rt, stash, ledger).expect("Must be owned.");

		// update the balance.
		BalanceOf::mutate(rt, stash, |old| old.unreserve(amount).expect("Must have enough reserved"))
			.or_orphan()?;

		Ok(())
	}

	fn tx_set_controller(rt, stash, ctrl: AccountId) {
		let mut ledger = Ledger::read(rt, stash).or_forward()?;

		if ledger == StakingLedger::default() {
			return Err(DispatchError::LogicError("Not bonded."));
		}

		let prev_ctrl = ledger.controller;
		if prev_ctrl == ctrl {
			return Ok(());
		} else {
			ledger.controller = ctrl;
		}

		// remove previous binding.
		Bonded::clear(rt, prev_ctrl).or_forward()?;
		// insert new bonding.
		Bonded::write(rt, ctrl, stash).or_orphan()?;
		// update ledger.
		Ledger::write(rt, stash, ledger).expect("Must be owned.");

		Ok(())

	}

	fn tx_validate(rt, ctrl) {
		let stash = Bonded::read(rt, ctrl).or_forward()?;
		if stash == Default::default() {
			return Err(DispatchError::LogicError("Not bonded."));
		}

		let mut ledger = Ledger::read(rt, stash).or_orphan()?;
		if ledger == StakingLedger::default() {
			panic!("Ledger must exist when Bonded exists.");
		}

		// update ledger.
		ledger.role = StakingRole::Validator;
		Ledger::write(rt, stash, ledger).expect("Must be owned.");

		Ok(())
	}

	fn tx_nominate(rt, ctrl, targets: Vec<AccountId>) {
		let stash = Bonded::read(rt, ctrl).or_forward()?;
		if stash == Default::default() {
			return Err(DispatchError::LogicError("Not bonded."));
		}

		let mut ledger = Ledger::read(rt, stash).or_orphan()?;
		if ledger == StakingLedger::default() {
			panic!("Ledger must exist when Bonded exists.");
		}

		// write targets.
		Nominations::write(rt, stash, targets).or_orphan()?;

		// update ledger.
		ledger.role = StakingRole::Nominator;
		Ledger::write(rt, stash, ledger).expect("Must be owned.");

		Ok(())
	}

	fn tx_chill(rt, ctrl) {
		let stash = Bonded::read(rt, ctrl).or_forward()?;
		if stash == Default::default() {
			return Err(DispatchError::LogicError("Not bonded."));
		}

		let mut ledger = Ledger::read(rt, stash).or_orphan()?;
		if ledger == StakingLedger::default() {
			panic!("Ledger must exist when Bonded exists.");
		}

		// clear potentially the nominations.
		if ledger.role == StakingRole::Nominator {
			Nominations::clear(rt, stash).or_orphan()?;
		}

		// update ledger.
		ledger.role = StakingRole::Chilled;
		Ledger::write(rt, stash, ledger).expect("Must be owned.");

		Ok(())
	}
}

macro_rules! test_with_rt {
	($rt:ty, $name:ident) => {
		#[cfg(test)]
		mod $name {
			type Runtime = $rt;
			use super::*;
			use crate::*;
			use primitives::testing::*;

			#[test]
			fn bonding_works() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = alice().public();

				// give alice some balance.
				balances::BalanceOf::write(&runtime, alice, 999.into()).unwrap();

				tx_bond(&runtime, alice, 333, bob().public()).unwrap();

				// must have reserved
				assert_eq!(
					balances::BalanceOf::read(&runtime, alice).unwrap().free(),
					666,
				);
				assert_eq!(
					balances::BalanceOf::read(&runtime, alice)
						.unwrap()
						.reserved(),
					333,
				);

				// must have Bonded and StakingLedger
				assert_eq!(Bonded::read(&runtime, bob().public()).unwrap(), alice);
				assert_eq!(Ledger::read(&runtime, alice).unwrap().value, 333);
				assert_eq!(
					Ledger::read(&runtime, alice).unwrap().role,
					StakingRole::Chilled
				);
				assert_eq!(
					Ledger::read(&runtime, alice).unwrap().controller,
					bob().public(),
				);
			}

			#[test]
			fn bonding_fails_if_already_bonded() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = alice().public();

				// give alice some balance.
				balances::BalanceOf::write(&runtime, alice, 999.into()).unwrap();

				tx_bond(&runtime, alice, 333, bob().public()).unwrap();
				assert_eq!(
					tx_bond(&runtime, alice, 333, bob().public()).unwrap_err(),
					DispatchError::LogicError("Already bonded.")
				);
			}

			#[test]
			fn bonding_fails_if_not_enough_balance() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = alice().public();

				// give alice some balance.
				balances::BalanceOf::write(&runtime, alice, 999.into()).unwrap();

				assert_eq!(
					tx_bond(&runtime, alice, 1333, bob().public()).unwrap_err(),
					DispatchError::LogicError("Not enough funds.")
				);
			}

			#[test]
			fn bond_extra_works() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = alice().public();

				// give alice some balance.
				balances::BalanceOf::write(&runtime, alice, 999.into()).unwrap();

				tx_bond(&runtime, alice, 333, bob().public()).unwrap();
				tx_bond_extra(&runtime, alice, 222).unwrap();

				// must have reserved
				assert_eq!(
					balances::BalanceOf::read(&runtime, alice).unwrap().free(),
					444,
				);
				assert_eq!(
					balances::BalanceOf::read(&runtime, alice)
						.unwrap()
						.reserved(),
					555,
				);

				// must have Bonded and StakingLedger
				assert_eq!(Ledger::read(&runtime, alice).unwrap().value, 555);
			}

			#[test]
			fn bond_fails_if_not_bonded() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = alice().public();

				// give alice some balance.
				balances::BalanceOf::write(&runtime, alice, 999.into()).unwrap();

				assert_eq!(
					tx_bond_extra(&runtime, alice, 222).unwrap_err(),
					DispatchError::LogicError("Not bonded.")
				);

				// must have not reserved
				assert_eq!(
					balances::BalanceOf::read(&runtime, alice).unwrap().free(),
					999,
				);
				assert_eq!(
					balances::BalanceOf::read(&runtime, alice)
						.unwrap()
						.reserved(),
					0,
				);
			}

			#[test]
			fn bond_extra_fails_if_not_enough_balance() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = alice().public();

				// give alice some balance.
				balances::BalanceOf::write(&runtime, alice, 666.into()).unwrap();

				tx_bond(&runtime, alice, 333, bob().public()).unwrap();
				assert_eq!(
					tx_bond_extra(&runtime, alice, 444).unwrap_err(),
					DispatchError::LogicError("Not enough funds.")
				);

				// must have not reserved anything extra.
				assert_eq!(
					balances::BalanceOf::read(&runtime, alice).unwrap().free(),
					333,
				);
				assert_eq!(
					balances::BalanceOf::read(&runtime, alice)
						.unwrap()
						.reserved(),
					333,
				);
			}

			#[test]
			fn unbond_works() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = alice().public();

				// give alice some balance.
				balances::BalanceOf::write(&runtime, alice, 999.into()).unwrap();

				tx_bond(&runtime, alice, 333, bob().public()).unwrap();

				// must have Bonded and StakingLedger
				assert_eq!(Bonded::read(&runtime, bob().public()).unwrap(), alice);
				assert_eq!(Ledger::read(&runtime, alice).unwrap().value, 333);

				tx_unbond(&runtime, alice, 111).unwrap();

				// must have Bonded and StakingLedger
				assert_eq!(Bonded::read(&runtime, bob().public()).unwrap(), alice);
				assert_eq!(Ledger::read(&runtime, alice).unwrap().value, 222);
			}

			#[test]
			fn unbond_fails_if_not_enough_bonded() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = alice().public();

				// give alice some balance.
				balances::BalanceOf::write(&runtime, alice, 999.into()).unwrap();

				tx_bond(&runtime, alice, 333, bob().public()).unwrap();

				// must have Bonded and StakingLedger
				assert_eq!(Bonded::read(&runtime, bob().public()).unwrap(), alice);
				assert_eq!(Ledger::read(&runtime, alice).unwrap().value, 333);

				assert_eq!(
					tx_unbond(&runtime, alice, 444).unwrap_err(),
					DispatchError::LogicError("Too much unbonding."),
				);
			}

			#[test]
			fn unbond_fails_if_not_bonded() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = alice().public();

				// give alice some balance.
				balances::BalanceOf::write(&runtime, alice, 999.into()).unwrap();

				assert_eq!(
					tx_unbond(&runtime, alice, 444).unwrap_err(),
					DispatchError::LogicError("Not bonded."),
				);
			}

			#[test]
			fn set_controller_works() {
				let state = RuntimeState::new().as_arc();
				let rt = &Runtime::new(state, 0);
				let alice = alice().public();
				let bob = bob().public();
				let dave = dave().public();

				// give alice some balance and bond.
				balances::BalanceOf::write(rt, alice, 999.into()).unwrap();
				tx_bond(rt, alice, 333, bob).unwrap();
				assert_eq!(Ledger::read(rt, alice).unwrap().controller, bob);

				tx_set_controller(rt, alice, dave).unwrap();
				assert_eq!(Ledger::read(rt, alice).unwrap().controller, dave);
			}

			#[test]
			fn set_controller_fails_if_not_bonded() {
				let state = RuntimeState::new().as_arc();
				let rt = &Runtime::new(state, 0);
				let alice = alice().public();

				assert_eq!(
					tx_unbond(rt, alice, 444).unwrap_err(),
					DispatchError::LogicError("Not bonded."),
				);
			}

			#[test]
			fn set_controller_is_noop_if_same_controller() {
				let state = RuntimeState::new().as_arc();
				let rt = &Runtime::new(state, 0);
				let alice = alice().public();
				let bob = bob().public();

				// give alice some balance and bond.
				balances::BalanceOf::write(rt, alice, 999.into()).unwrap();
				tx_bond(rt, alice, 333, bob).unwrap();
				assert_eq!(Ledger::read(rt, alice).unwrap().controller, bob);

				tx_set_controller(rt, alice, bob).unwrap();
				assert_eq!(Ledger::read(rt, alice).unwrap().controller, bob);
			}

			#[test]
			fn validate_works() {
				let state = RuntimeState::new().as_arc();
				let rt = &Runtime::new(state, 0);
				let alice = alice().public();
				let bob = bob().public();

				// give alice some balance and bond.
				balances::BalanceOf::write(rt, alice, 999.into()).unwrap();
				tx_bond(rt, alice, 333, bob).unwrap();
				assert_eq!(Ledger::read(rt, alice).unwrap().role, StakingRole::Chilled);

				tx_validate(rt, bob).unwrap();
				assert_eq!(
					Ledger::read(rt, alice).unwrap().role,
					StakingRole::Validator,
				);
			}

			#[test]
			fn validate_fails_if_not_bonded() {
				let state = RuntimeState::new().as_arc();
				let rt = &Runtime::new(state, 0);
				let alice = alice().public();

				assert_eq!(
					tx_validate(rt, alice).unwrap_err(),
					DispatchError::LogicError("Not bonded."),
				);
			}

			#[test]
			fn nominate_works() {
				let state = RuntimeState::new().as_arc();
				let rt = &Runtime::new(state, 0);
				let alice = alice().public();
				let bob = bob().public();

				// give alice some balance and bond.
				balances::BalanceOf::write(rt, alice, 999.into()).unwrap();
				tx_bond(rt, alice, 333, bob).unwrap();
				assert_eq!(Ledger::read(rt, alice).unwrap().role, StakingRole::Chilled);
				assert_eq!(Bonded::read(rt, bob).unwrap(), alice);

				tx_nominate(rt, bob, vec![dave().public()]).unwrap();
				assert_eq!(
					Ledger::read(rt, alice).unwrap().role,
					StakingRole::Nominator,
				);
				assert_eq!(Nominations::read(rt, alice).unwrap(), vec![dave().public()]);
			}

			#[test]
			fn nominate_fails_if_not_bonded() {
				let state = RuntimeState::new().as_arc();
				let rt = &Runtime::new(state, 0);
				let alice = alice().public();

				assert_eq!(
					tx_nominate(rt, alice, vec![]).unwrap_err(),
					DispatchError::LogicError("Not bonded."),
				);
			}

			#[test]
			fn chill_works_after_validate() {
				let state = RuntimeState::new().as_arc();
				let rt = &Runtime::new(state, 0);
				let alice = alice().public();
				let bob = bob().public();

				// give alice some balance and bond.
				balances::BalanceOf::write(rt, alice, 999.into()).unwrap();
				tx_bond(rt, alice, 333, bob).unwrap();
				assert_eq!(Ledger::read(rt, alice).unwrap().role, StakingRole::Chilled);

				tx_validate(rt, bob).unwrap();
				assert_eq!(
					Ledger::read(rt, alice).unwrap().role,
					StakingRole::Validator,
				);

				tx_chill(rt, bob).unwrap();
				assert_eq!(Ledger::read(rt, alice).unwrap().role, StakingRole::Chilled);
			}

			#[test]
			fn chill_works_after_nominate() {
				let state = RuntimeState::new().as_arc();
				let rt = &Runtime::new(state, 0);
				let alice = alice().public();
				let bob = bob().public();

				// give alice some balance and bond.
				balances::BalanceOf::write(rt, alice, 999.into()).unwrap();
				tx_bond(rt, alice, 333, bob).unwrap();
				tx_nominate(rt, bob, vec![dave().public()]).unwrap();
				assert_eq!(
					Ledger::read(rt, alice).unwrap().role,
					StakingRole::Nominator,
				);
				assert_eq!(Nominations::read(rt, alice).unwrap(), vec![dave().public()]);

				tx_chill(rt, bob).unwrap();
				assert_eq!(Ledger::read(rt, alice).unwrap().role, StakingRole::Chilled);
				assert_eq!(Nominations::read(rt, alice).unwrap(), vec![]);
			}
		}
	};
}

test_with_rt!(crate::SequentialRuntime, master_runtime);
test_with_rt!(crate::ConcurrentRuntime, concurrent_runtime);
