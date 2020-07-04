use crate::{decl_storage_map, decl_storage_value};
use crate::{DispatchResult, Dispatchable, GenericRuntime, ValidationResult};
use parity_scale_codec::{Decode, Encode};
use primitives::*;

const MODULE: &'static str = "staking";

#[derive(Encode, Decode, Debug, Clone)]
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

#[derive(Encode, Decode, Default, Debug, Clone)]
pub struct StakingLedger {
	value: Balance,
	controller: Option<AccountId>,
	role: StakingRole,
}

decl_storage_map!(Ledger, "ledger_of", AccountId, StakingLedger);
decl_storage_map!(Bonded, "bonded", AccountId, Option<AccountId>);
decl_storage_map!(Nominations, "nominations", AccountId, Vec<AccountId>);
decl_storage_value!(Validators, "validators", Vec<AccountId>);

#[derive(Debug, Clone, Eq, PartialEq)]
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

impl<R: GenericRuntime> Dispatchable<R> for Call {
	fn dispatch(&self, runtime: &R, origin: AccountId) -> DispatchResult {
		match *self {
			Call::Bond(amount) => tx_bond(runtime, origin, amount),
			Call::Unbond(amount) => tx_unbond(runtime, origin, amount),
			_ => todo!(),
		}
	}

	fn validate(&self, _: &R, _: AccountId) -> ValidationResult {
		Ok(Default::default())
	}
}

fn tx_bond<R: GenericRuntime>(
	_runtime: &R,
	_origin: AccountId,
	_amount: Balance,
) -> DispatchResult {
	unimplemented!()
}

fn tx_unbond<R: GenericRuntime>(
	_runtime: &R,
	_origin: AccountId,
	_amount: Balance,
) -> DispatchResult {
	unimplemented!()
}
