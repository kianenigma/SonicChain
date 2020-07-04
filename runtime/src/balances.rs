use crate::decl_storage_map;
use crate::{DispatchError, DispatchResult, Dispatchable, GenericRuntime, ValidationResult};
use parity_scale_codec::{Decode, Encode};
use primitives::*;

const MODULE: &'static str = "balances";

decl_storage_map!(
	BalanceOf,
	"balance_of",
	primitives::AccountId,
	primitives::Balance
);

/// The call of the balances module.
#[derive(Debug, Clone, Eq, PartialEq, Encode, Decode)]
pub enum Call {
	Transfer(AccountId, Balance),
}

impl<R: GenericRuntime> Dispatchable<R> for Call {
	fn dispatch(&self, runtime: &R, origin: AccountId) -> DispatchResult {
		match *self {
			Self::Transfer(to, value) => tx_transfer(origin, runtime, to, value),
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

fn tx_transfer<R: GenericRuntime>(
	origin: AccountId,
	runtime: &R,
	dest: AccountId,
	value: Balance,
) -> DispatchResult {
	// If we fail at this step, it is fine. We have not written anything yet.
	let old_value =
		BalanceOf::read(runtime, origin).map_err(|id| DispatchError::Tainted(id, false))?;

	if let Some(remaining) = old_value.checked_sub(value) {
		// update origin. Failure is okay.
		BalanceOf::write(runtime, origin, remaining)
			.map_err(|id| DispatchError::Tainted(id, false))?;

		// update dest.
		BalanceOf::mutate(runtime, dest, |old| *old += value)
			.map_err(|id| DispatchError::Tainted(id, true))?;
		Ok(())
	} else {
		Err(DispatchError::LogicError("Does not have enough funds."))
	}
}

#[cfg(test)]
mod balances_test {
	use super::*;
	use crate::{DispatchError, OuterCall, RuntimeState, WorkerRuntime};
	use std::sync::Arc;

	// TODO: test this and storage macros with both runtime. They should both have similar
	// behaviors.

	#[test]
	fn transfer_works() {
		let state = RuntimeState::new().as_arc();
		let runtime = WorkerRuntime::new(Arc::clone(&state), 0);
		let alice = primitives::testing::alice().public();
		let bob = primitives::testing::bob().public();

		// give alice some balance.
		state.unsafe_insert_genesis_value(
			&<BalanceOf<WorkerRuntime>>::key_for(alice),
			(999 as Balance).encode().into(),
		);

		let transfer = OuterCall::Balances(Call::Transfer(bob.clone(), 666));

		runtime.dispatch(&transfer, alice).unwrap();

		assert_eq!(BalanceOf::read(&runtime, bob).unwrap(), 666);
		assert_eq!(BalanceOf::read(&runtime, alice).unwrap(), 333);
	}

	#[test]
	fn transfer_fails_if_not_enough_balance() {
		let state = RuntimeState::new().as_arc();
		let runtime = WorkerRuntime::new(Arc::clone(&state), 0);
		let alice = primitives::testing::alice().public();
		let bob = primitives::testing::bob().public();

		// give alice some balance.
		state.unsafe_insert_genesis_value(
			&<BalanceOf<WorkerRuntime>>::key_for(alice),
			(333 as Balance).encode().into(),
		);

		let transfer = OuterCall::Balances(Call::Transfer(bob.clone(), 666));

		assert_eq!(
			runtime.dispatch(&transfer, alice).unwrap_err(),
			DispatchError::LogicError("Does not have enough funds."),
		);

		assert_eq!(BalanceOf::read(&runtime, bob).unwrap(), 0);
		assert_eq!(BalanceOf::read(&runtime, alice).unwrap(), 333);
	}
}
