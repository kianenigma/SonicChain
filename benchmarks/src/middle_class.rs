use crate::datasets;
use csv::Writer;
use executor::{concurrent::*, *};
use std::time::Duration;
use tx_distribution::ConnectedComponents;

macro_rules! bench_seq {
	($members:expr, $lucky:expr, $txs:expr, $wtr:ident) => {
		let (authoring, validation) = seq_middle_class_playground($members, $txs, $lucky);
		let authoring_tps = ($txs as f64) / ((authoring.as_millis() as f64) / 1000f64);
		let validation_tps = ($txs as f64) / ((validation.as_millis() as f64) / 1000f64);
		$wtr.write_record(&[
			"seq",
			stringify!($members),
			stringify!($lucky),
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
	($members:expr, $lucky:expr, $txs:expr, $dist:ty, $threads:expr, $wtr:ident) => {
		let (authoring, validation) =
			concurrent_middle_class_playground::<$dist>($members, $txs, $lucky, $threads);
		$wtr.write_record(&[
			concat!("Concurrent(", stringify!($dist), "-", $threads, ")"),
			stringify!($members),
			stringify!($lucky),
			stringify!($txs),
			&authoring.as_millis().to_string(),
			&validation.as_millis().to_string(),
			])
		.unwrap();
		$wtr.flush().unwrap();
	};
}

#[allow(dead_code)]
pub fn middle_class_playground_bench() {
	let mut wtr = Writer::from_path("middle_class_playground.csv").unwrap();
	wtr.write_record(&["middle_class_playground", "-", "-", "-", "-", "-", "-", "-"])
		.unwrap();
	wtr.write_record(&[
		"type",
		"members",
		"lucky",
		"transactions",
		"authoring (ms)",
		"authoring tps",
		"validation (ms)",
		"validation tps",
	])
	.unwrap();

	bench_seq!(1000, 250, 500, wtr);
	bench_seq!(1000, 500, 500, wtr);
	bench_seq!(1000, 1000, 500, wtr);

	bench_concurrent!(1000, 250, 500, ConnectedComponents, 4, wtr);
	bench_concurrent!(1000, 500, 500, ConnectedComponents, 4, wtr);
	bench_concurrent!(1000, 1000, 500, ConnectedComponents, 4, wtr);

	wtr.flush().unwrap();
}

fn seq_middle_class_playground(
	members: usize,
	transfers: usize,
	lucky_members: usize,
) -> (Duration, Duration) {
	let mut executor = sequential::SequentialExecutor::new();
	let dataset = datasets::middle_class_playground(
		&executor.runtime,
		members,
		transfers,
		100,
		lucky_members,
	);
	let initial_state = executor.runtime.state.dump();

	let (valid, authoring_time, validation_time) =
		executor.author_and_validate(dataset, Some(initial_state));
	assert!(valid);
	(authoring_time, validation_time)
}

fn concurrent_middle_class_playground<D: tx_distribution::Distributer>(
	members: usize,
	transfers: usize,
	lucky_members: usize,
	num_threads: usize,
) -> (Duration, Duration) {
	let mut executor = concurrent::ConcurrentExecutor::<Pool, D>::new(num_threads, false, None);
	let dataset = datasets::middle_class_playground(
		&executor.master.runtime,
		members,
		transfers,
		100,
		lucky_members,
	);
	let initial_state = executor.master.state.dump();

	let (valid, authoring_time, validation_time) =
		executor.author_and_validate(dataset, Some(initial_state));
	assert!(valid);
	executor.master.run_terminate();
	executor.master.join_all().unwrap();
	(authoring_time, validation_time)
}
