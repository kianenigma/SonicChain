use executor::{concurrent::*, types::*, *};
use primitives::*;
use rand::seq::SliceRandom;
use runtime::*;
use state::StateEq;

mod datasets;
mod middle_class;
mod millionaires;

const LOG_TARGET: &'static str = "benchmarks";
const NUM_THREADS: usize = 4;

// TODO: more complicated modules: multiSig is fun?
// TODO: I wish I could impl Drop for Master and join() + terminate all workers..
// FIXME: test cases in this crate for determinism.
// FIXME: pool needs reordering once we have a forward, interesting that I don't have this issue yet.
// FIXME: adding initial state to all functions of the executor could potentially make the API
// nicer.
// TODO: remove log as dependency all over the place.

const STAKERS_VALIDATORS: usize = 200;
const STAKERS_NOMINATORS: usize = 500;
const STAKERS_RANDOM: usize = 1000;

#[allow(dead_code)]
fn sequential_stakers() {
	let mut executor = sequential::SequentialExecutor::new();
	let dataset = datasets::world_of_stakers(
		&executor.runtime,
		STAKERS_VALIDATORS,
		STAKERS_NOMINATORS,
		STAKERS_RANDOM,
	);

	let start = std::time::Instant::now();
	executor.author_block(dataset);
	println!("Seq authoring took {:?}", start.elapsed());
}

#[allow(dead_code)]
fn concurrent_stakers<D: tx_distribution::Distributer>() {
	let mut executor = concurrent::ConcurrentExecutor::<Pool, D>::new(NUM_THREADS, false, None);
	let dataset = datasets::world_of_stakers(
		&executor.master.runtime,
		STAKERS_VALIDATORS,
		STAKERS_NOMINATORS,
		STAKERS_RANDOM,
	);

	let initial_state = executor.master.state.dump();

	let start = std::time::Instant::now();
	let (s1, block, _) = executor.author_block(dataset);
	println!("Concurrent authoring took {:?}", start.elapsed());

	executor.clean();
	executor.apply_state(initial_state);
	let (s2, _) = executor.validate_block(block);
	assert!(s1.state_eq(s2));
}

fn main() {
	logging::init_logger();

	millionaires::millionaires_playground_bench();
	// millionaires::growing_economy();
	// middle_class::middle_class_playground_bench();
}
