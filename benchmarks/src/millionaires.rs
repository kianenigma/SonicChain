use crate::datasets;
use csv::Writer;
use executor::{concurrent::*, sequential::*, *};
use std::time::Duration;
use tx_distribution::{ConnectedComponents, RoundRobin};

#[allow(dead_code)]
const LOG_TARGET: &'static str = "millionaires_playground";

macro_rules! bench_seq {
	($members:expr, $txs:expr, $wtr:ident) => {
		let (authoring, validation) = seq_millionaires_playground($members, $txs);
		let authoring_tps = ($txs as f64) / ((authoring.as_millis() as f64) / 1000f64);
		let validation_tps = ($txs as f64) / ((validation.as_millis() as f64) / 1000f64);
		$wtr.write_record(&[
			"Sequential",
			stringify!($members),
			stringify!($txs),
			&authoring.as_millis().to_string(),
			&format!("{:.2}", authoring_tps),
			&validation.as_millis().to_string(),
			&format!("{:.2}", validation_tps),
			])
		.unwrap();
		$wtr.flush().unwrap();
	};
}

macro_rules! bench_concurrent {
	($members:expr, $txs:expr, $dist:ty, $threads:expr, $wtr:ident) => {
		let (authoring, validation) =
			concurrent_millionaires_playground::<$dist>($members, $txs, $threads);
		let authoring_tps = ($txs as f64) / ((authoring.as_millis() as f64) / 1000f64);
		let validation_tps = ($txs as f64) / ((validation.as_millis() as f64) / 1000f64);
		$wtr.write_record(&[
			concat!("Concurrent(", stringify!($dist), "-", $threads, ")"),
			stringify!($members),
			stringify!($txs),
			&authoring.as_millis().to_string(),
			&format!("{:.2}", authoring_tps),
			&validation.as_millis().to_string(),
			&format!("{:.2}", validation_tps),
			])
		.unwrap();
		$wtr.flush().unwrap();
	};
}

#[allow(dead_code)]
pub fn growing_economy_bench() {
	let mut wtr = Writer::from_path("growing_economy.csv").unwrap();
	wtr.write_record(&["growing_economy", "-", "-", "-", "-", "-", "-"])
		.unwrap();
	wtr.write_record(&[
		"type",
		"members",
		"transactions",
		"authoring (ms)",
		"authoring tps",
		"validation (ms)",
		"validation tps",
	])
	.unwrap();

	bench_seq!(1000, 2000, wtr);
	bench_seq!(2000, 2000, wtr);
	bench_seq!(3000, 2000, wtr);
	bench_seq!(4000, 2000, wtr);

	bench_concurrent!(1000, 2000, RoundRobin, 4, wtr);
	bench_concurrent!(2000, 2000, RoundRobin, 4, wtr);
	bench_concurrent!(3000, 2000, RoundRobin, 4, wtr);
	bench_concurrent!(4000, 2000, RoundRobin, 4, wtr);

	bench_concurrent!(1000, 2000, ConnectedComponents, 4, wtr);
	bench_concurrent!(2000, 2000, ConnectedComponents, 4, wtr);
	bench_concurrent!(3000, 2000, ConnectedComponents, 4, wtr);
	bench_concurrent!(4000, 2000, ConnectedComponents, 4, wtr);

	wtr.flush().unwrap();
}

#[allow(dead_code)]
pub fn millionaires_playground_bench() {
	let mut wtr = Writer::from_path("millionaires_playground.csv").unwrap();
	wtr.write_record(&["millionaires_playground", "-", "-", "-", "-", "-", "-"])
		.unwrap();
	wtr.write_record(&[
		"type",
		"members",
		"transactions",
		"authoring (ms)",
		"authoring tps",
		"validation (ms)",
		"validation tps",
	])
	.unwrap();

	bench_seq!(1000, 250, wtr);
	bench_seq!(1000, 500, wtr);
	bench_seq!(1000, 1000, wtr);
	bench_seq!(1000, 2000, wtr);

	bench_concurrent!(1000, 250, RoundRobin, 4, wtr);
	bench_concurrent!(1000, 500, RoundRobin, 4, wtr);
	bench_concurrent!(1000, 1000, RoundRobin, 4, wtr);
	bench_concurrent!(1000, 2000, RoundRobin, 4, wtr);

	bench_concurrent!(1000, 250, ConnectedComponents, 4, wtr);
	bench_concurrent!(1000, 500, ConnectedComponents, 4, wtr);
	bench_concurrent!(1000, 1000, ConnectedComponents, 4, wtr);
	bench_concurrent!(1000, 2000, ConnectedComponents, 4, wtr);

	wtr.flush().unwrap();
}

fn seq_millionaires_playground(members: usize, transactions: usize) -> (Duration, Duration) {
	let mut executor = SequentialExecutor::new();
	let dataset = datasets::millionaires_playground(&executor.runtime, members, transactions);
	let initial_state = executor.runtime.state.dump();

	let (valid, authoring_time, validation_time) =
		executor.author_and_validate(dataset, Some(initial_state));
	assert!(valid);
	(authoring_time, validation_time)
}

fn concurrent_millionaires_playground<D: tx_distribution::Distributer>(
	members: usize,
	transactions: usize,
	num_threads: usize,
) -> (Duration, Duration) {
	let mut executor = ConcurrentExecutor::<Pool, D>::new(num_threads, false, None);
	let dataset =
		datasets::millionaires_playground(&executor.master.runtime, members, transactions);
	let initial_state = executor.master.state.dump();

	let (valid, authoring_time, validation_time) =
		executor.author_and_validate(dataset, Some(initial_state));
	assert!(valid);
	executor.master.run_terminate();
	executor.master.join_all().unwrap();
	(authoring_time, validation_time)
}
