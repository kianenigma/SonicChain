use crate::{
	decl_storage_map, decl_tx, DispatchError, DispatchResult, Dispatchable, ModuleRuntime,
	UnwrapStorageOp, ValidationResult,
};
use parity_scale_codec::{Decode, Encode};
use primitives::*;

const MODULE: &'static str = "balances";

#[derive(Debug, Clone, Default, Eq, PartialEq, Encode, Decode)]
pub struct AccountBalance {
	free: Balance,
	reserved: Balance,
}

impl From<Balance> for AccountBalance {
	fn from(free: Balance) -> Self {
		Self { free, reserved: 0 }
	}
}

impl From<(Balance, Balance)> for AccountBalance {
	fn from((free, reserved): (Balance, Balance)) -> Self {
		Self { free, reserved }
	}
}

impl AccountBalance {
	pub fn new(free: Balance, reserved: Balance) -> Self {
		Self { free, reserved }
	}

	pub fn new_free(free: Balance) -> Self {
		Self { free, reserved: 0 }
	}

	/// Total balance of an account.
	pub fn total(&self) -> Balance {
		self.free + self.reserved
	}

	/// Free balance of the account.
	pub fn free(&self) -> Balance {
		self.free
	}

	/// Reserved balance of the account
	pub fn reserved(&self) -> Balance {
		self.reserved
	}

	pub fn reserve(&mut self, amount: Balance) -> Result<(), &'static str> {
		if self.can_spend(amount) {
			self.free -= amount;
			self.reserved += amount;
			Ok(())
		} else {
			Err("Not enough funds.")
		}
	}

	pub fn unreserve(&mut self, amount: Balance) -> Result<(), &'static str> {
		if self.reserved > amount {
			self.reserved -= amount;
			self.free += amount;
			Ok(())
		} else {
			Err("Not enough reserved.")
		}
	}

	/// True of account has `amount` to spend or bond.
	pub fn can_spend(&self, amount: Balance) -> bool {
		self.free >= amount
	}
}

decl_storage_map!(
	BalanceOf,
	"balance_of",
	primitives::AccountId,
	AccountBalance
);

/// The call of the balances module.
#[derive(Debug, Clone, Eq, PartialEq, Encode, Decode)]
pub enum Call {
	Transfer(AccountId, Balance),
}

impl<R: ModuleRuntime> Dispatchable<R> for Call {
	fn dispatch<T>(self, runtime: &R, origin: AccountId) -> DispatchResult {
		match self {
			Self::Transfer(to, value) => tx_transfer(runtime, origin, to, value),
		}
	}

	fn validate(&self, _: &R, origin: AccountId) -> ValidationResult {
		match *self {
			Self::Transfer(to, _) => Ok(vec![
				<BalanceOf<R>>::key_for(origin),
				<BalanceOf<R>>::key_for(to),
			]),
		}
	}
}

decl_tx! {
	fn tx_transfer(
		runtime,
		origin,
		dest: AccountId,
		value: Balance,
	) {
		std::thread::sleep(std::time::Duration::from_millis(10));
		// If we fail at this step, it is fine. We have not written anything yet.
		let mut old_balance =
			BalanceOf::read(runtime, origin).or_forward()?;

		if let Some(remaining) = old_balance.free.checked_sub(value) {
			// update origin. Failure is okay.
			old_balance.free = remaining;

			BalanceOf::write(runtime, origin, old_balance)
				.expect("Origin's balance key must be owned by the current thread.");

			// update dest.
			BalanceOf::mutate(runtime, dest, |old| old.free += value).or_orphan()?;

			Ok(())
		} else {
			Err(DispatchError::LogicError("Does not have enough funds."))
		}
	}
}

macro_rules! test_with_rt {
	($rt:ty, $name:ident) => {
		#[cfg(test)]
		mod $name {
			type Runtime = $rt;
			use super::*;
			use crate::*;
			use std::sync::Arc;

			#[test]
			fn transfer_works() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = primitives::testing::alice().public();
				let bob = primitives::testing::bob().public();

				// give alice some balance.
				state.unsafe_insert_genesis_value(
					&<BalanceOf<Runtime>>::key_for(alice),
					(AccountBalance::from(999)).encode().into(),
				);

				let transfer = OuterCall::Balances(Call::Transfer(bob.clone(), 666));

				runtime.dispatch(transfer, alice).unwrap();

				assert_eq!(BalanceOf::read(&runtime, bob).unwrap().free, 666);
				assert_eq!(BalanceOf::read(&runtime, alice).unwrap().free, 333);
			}

			#[test]
			fn transfer_fails_if_not_enough_balance() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = primitives::testing::alice().public();
				let bob = primitives::testing::bob().public();

				// give alice some balance.
				state.unsafe_insert_genesis_value(
					&<BalanceOf<Runtime>>::key_for(alice),
					AccountBalance::from(333).encode().into(),
				);

				let transfer = OuterCall::Balances(Call::Transfer(bob.clone(), 666));

				assert_eq!(
					runtime.dispatch(transfer, alice).unwrap(),
					RuntimeDispatchSuccess::LogicError("Does not have enough funds."),
				);

				assert_eq!(BalanceOf::read(&runtime, bob).unwrap().free, 0);
				assert_eq!(BalanceOf::read(&runtime, alice).unwrap().free, 333);
			}

			#[test]
			fn reserved_cannot_be_transferred() {
				let state = RuntimeState::new().as_arc();
				let runtime = Runtime::new(Arc::clone(&state), 0);
				let alice = primitives::testing::alice().public();
				let bob = primitives::testing::bob().public();

				// give alice some balance.
				state.unsafe_insert_genesis_value(
					&<BalanceOf<Runtime>>::key_for(alice),
					(AccountBalance::from((333, 666))).encode().into(),
				);

				assert_eq!(
					tx_transfer(&runtime, alice, bob, 334).unwrap_err(),
					DispatchError::LogicError("Does not have enough funds.")
				);
			}
		}
	};
}

test_with_rt!(crate::WorkerRuntime, worker_runtime_test);
test_with_rt!(crate::MasterRuntime, master_runtime_test);
