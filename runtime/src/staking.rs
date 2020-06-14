use crate::{DispatchResult, Dispatchable};
use primitives::*;
use state::GenericState;

#[derive(Debug, Clone)]
pub enum Call {
	/// Bond some funds.
	Bond(Balance),
	/// Unbond some funds.
	Unbond(Balance),
	/// Set intention to validate.
	Validate,
	/// Set intention to nominate.
	Nominate(Balance, Vec<AccountId>),
	/// Set intention to do nothing anymore.
	Chill,
}

impl<S: GenericState<Key, Value, ThreadId>> Dispatchable<S> for Call {
	fn dispatch(&self, state: &S, origin: AccountId) -> DispatchResult {
		match *self {
			Call::Bond(amount) => bond(state, origin, amount),
			Call::Unbond(amount) => unbond(state, origin, amount),
			_ => todo!(),
		}
	}
}

fn bond<S: GenericState<Key, Value, ThreadId>>(
	_state: &S,
	_origin: AccountId,
	_amount: Balance,
) -> DispatchResult {
	unimplemented!()
}

fn unbond<S: GenericState<Key, Value, ThreadId>>(
	_state: &S,
	_origin: AccountId,
	_amount: Balance,
) -> DispatchResult {
	unimplemented!()
}

#[allow(dead_code)]
fn validate<S: GenericState<Key, Value, ThreadId>>(
	_state: &S,
	_origin: AccountId,
) -> DispatchResult {
	unimplemented!()
}

#[allow(dead_code)]
fn nominate<S: GenericState<Key, Value, ThreadId>>(
	_state: &S,
	_origin: AccountId,
) -> DispatchResult {
	unimplemented!()
}

#[allow(dead_code)]
fn chill<S: GenericState<Key, Value, ThreadId>>(_state: &S, _origin: AccountId) -> DispatchResult {
	unimplemented!()
}
