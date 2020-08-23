use executor::{concurrent::*, types::*, *};
use primitives::*;
use runtime::*;

// FIXME: means of annotating stuff.
// FIXME: Timings are not really accurate yet.
// FIXME: dataset: world of stakers
// TODO: flaky predictions?
// TODO: more complicated modules: multiSig is fun?

mod datasets {
	use super::*;
	use types::transaction_generator::*;

	pub fn millionaires_playground<R: ModuleRuntime>(
		rt: &R,
		members: usize,
		transfers: usize,
	) -> Vec<Transaction> {
		const AMOUNT: Balance = 100_000_000_000;

		let (transactions, accounts) = bank(members, transfers);
		accounts
			.into_iter()
			.for_each(|acc| endow_account(acc, rt, AMOUNT));
		transactions
	}
}

fn sequential() {
	let mut executor = sequential::SequentialExecutor::new();
	let dataset = datasets::millionaires_playground(&executor.runtime, 10_000, 5_000);

	let start = std::time::Instant::now();
	executor.author_block(dataset);
	println!("Seq authoring took {:?}", start.elapsed());
}

fn concurrent<D: tx_distribution::Distributer>() {
	let mut executor = concurrent::ConcurrentExecutor::<Pool, D>::new(4, false, None);
	let dataset = datasets::millionaires_playground(&executor.master.runtime, 10_000, 5_000);

	let start = std::time::Instant::now();
	executor.author_block(dataset);
	println!("Concurrent authoring took {:?}", start.elapsed());
}

fn main() {
	use tx_distribution::ConnectedComponentsDistributer;
	logging::init_logger();

	concurrent::<ConnectedComponentsDistributer>();
	std::thread::sleep(std::time::Duration::from_secs(4));
	sequential();
}
