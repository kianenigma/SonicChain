use crate::{DispatchResult, Dispatchable};
use primitives::*;
use state::GenericState;

/// The call of the balances module.
#[derive(Debug, Clone)]
pub enum Call {
	Transfer(AccountId, Balance),
}

impl<S: GenericState<Key, Value, ThreadId>> Dispatchable<S> for Call {
	fn dispatch(&self, state: &S, origin: AccountId) -> DispatchResult {
		match *self {
			Self::Transfer(to, value) => transfer(state, origin, to, value),
		}
	}
}

fn transfer<S: GenericState<Key, Value, ThreadId>>(
	state: &S,
	_origin: AccountId,
	_to: AccountId,
	_value: Balance,
) -> DispatchResult {
	// TODO: this is quite important to finalize.
	state.read(&10, 1u64.into())?;
	state.write(&10, vec![1, 2, 3], 1u32.into())?;
	Ok(())
}
