use crate::{DispatchError, DispatchResult, Dispatchable, GenericRuntime};
use parity_scale_codec::{Decode, Encode};
use primitives::*;

/// The call of the balances module.
#[derive(Debug, Clone, Eq, PartialEq, Encode, Decode)]
pub enum Call {
	Transfer(AccountId, Balance),
}

impl<R: GenericRuntime> Dispatchable<R> for Call {
	fn dispatch(&self, runtime: &R, origin: AccountId) -> DispatchResult {
		match *self {
			Self::Transfer(to, value) => transfer(origin, runtime, to, value),
		}
	}
}

pub mod storage {
	use super::*;

	const MODULE: &'static str = "balances";

	pub struct BalanceOf<R>(std::marker::PhantomData<R>);

	impl<R: GenericRuntime> BalanceOf<R> {
		pub fn key_for(key: primitives::AccountId) -> primitives::Key {
			let mut final_key = Vec::new();
			final_key.extend(format!("{}:{}", MODULE, "balance_of").as_bytes());
			final_key.extend(key.as_ref());
			final_key
		}

		pub fn write(
			runtime: &R,
			key: primitives::AccountId,
			val: primitives::Balance,
		) -> Result<(), ThreadId> {
			let encoded_value = val.encode();
			let final_key = Self::key_for(key);
			runtime.write(&final_key, encoded_value)
		}

		pub fn read(
			runtime: &R,
			key: primitives::AccountId,
		) -> Result<primitives::Balance, ThreadId> {
			let final_key = Self::key_for(key);
			let encoded = runtime.read(&final_key)?;
			Ok(<primitives::Balance as Decode>::decode(&mut &*encoded).unwrap_or_default())
		}

		pub fn mutate(
			runtime: &R,
			key: primitives::AccountId,
			update: impl Fn(&mut Balance) -> (),
		) -> Result<(), ThreadId> {
			let mut old = Self::read(runtime, key.clone())?;
			update(&mut old);
			Self::write(runtime, key, old)
		}
	}
}

fn transfer<R: GenericRuntime>(
	origin: AccountId,
	runtime: &R,
	dest: AccountId,
	value: Balance,
) -> DispatchResult {
	let old_value =
		storage::BalanceOf::read(runtime, origin).map_err(|id| DispatchError::Tainted(id))?;
	if let Some(remaining) = old_value.checked_sub(value) {
		// update origin.
		storage::BalanceOf::write(runtime, origin, remaining)
			.map_err(|id| DispatchError::Tainted(id))?;
		// update dest.
		storage::BalanceOf::mutate(runtime, dest, |old| *old += value)
			.map_err(|id| DispatchError::Tainted(id))?;
		Ok(())
	} else {
		Err(DispatchError::LogicError("Does not have enough funds."))
	}
}

#[cfg(test)]
mod balances_test {
	use super::*;
	use crate::{DispatchError, OuterCall, Runtime, RuntimeState};

	#[test]
	fn transfer_works() {
		let state = RuntimeState::new().as_arc();
		let runtime = Runtime::new(state, 0);
		let alice = primitives::testing::alice().public();
		let bob = primitives::testing::bob().public();

		// give alice some balance.
		storage::BalanceOf::write(&runtime, alice.clone(), 999).unwrap();

		let transfer = OuterCall::Balances(Call::Transfer(bob.clone(), 666));

		runtime.dispatch(transfer, alice).unwrap();

		assert_eq!(storage::BalanceOf::read(&runtime, bob).unwrap(), 666);
		assert_eq!(storage::BalanceOf::read(&runtime, alice).unwrap(), 333);
	}

	#[test]
	fn transfer_fails_if_not_enough_balance() {
		let state = RuntimeState::new().as_arc();
		let runtime = Runtime::new(state, 0);
		let alice = primitives::testing::alice().public();
		let bob = primitives::testing::bob().public();

		// give alice some balance.
		storage::BalanceOf::write(&runtime, alice.clone(), 333).unwrap();

		let transfer = OuterCall::Balances(Call::Transfer(bob.clone(), 666));

		assert_eq!(
			runtime.dispatch(transfer, alice).unwrap_err(),
			DispatchError::LogicError("Does not have enough funds."),
		);

		assert_eq!(storage::BalanceOf::read(&runtime, bob).unwrap(), 0);
		assert_eq!(storage::BalanceOf::read(&runtime, alice).unwrap(), 333);
	}
}
