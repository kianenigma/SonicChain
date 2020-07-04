#![feature(thread_id_value)]
#![feature(debug_non_exhaustive)]

use log::*;
use primitives::*;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;

pub mod master;
pub mod pool;
mod test;
pub mod tx_distribution;
pub mod types;
pub mod worker;

use master::*;
use pool::*;
use tx_distribution::*;
use types::*;
use worker::*;

/// The final state type of the application.
pub type State = runtime::RuntimeState;
/// The final pool type of the application.
pub type Pool = VecPool<Transaction>;

// FIXME: block_construction() and block_validation() interface.
// FIXME: some means of computing the state root, or else comparing the two states.
// FIXME: Start thinking about test scenarios.
// FIXME: add algorithm to do something based on the static annotations.
// FIXME: new module to generate random transactions.
// TODO: A bank example with orphans?

#[macro_export]
macro_rules! log_target {
	($target:tt, $level:tt, $patter:expr $(, $values:expr)* $(,)?) => {
		log::$level!(
			target: $target,
			$patter $(, $values)*
		)
	};
}

/// Spawn `n` new worker threads.
///
/// This _should only be called once_.
fn spawn_workers<P: TransactionPool<Transaction>, D: Distributer>(
	n: usize,
	test_run: bool,
) -> Master<P, D> {
	// One queue for all workers to send to master.
	let (workers_to_master_tx, workers_to_master_rx) = channel();

	let mut master = Master::new_from_thread(workers_to_master_rx);
	let mut to_workers: BTreeMap<ThreadId, Sender<Message>> = Default::default();

	for i in 0..n {
		// clone the state and master id.
		let state_ptr = Arc::clone(&master.state);
		let master_id = master.id;

		// one channel for the master to send to this worker.
		let (master_to_worker_tx, master_to_worker_rx) = channel();
		let worker_to_master_tx = Clone::clone(&workers_to_master_tx);

		// one channel for other workers to send to this worker.
		let (from_others_tx, from_others_rx) = channel();

		let worker_handle = thread::Builder::new()
			.name(format!("Worker#{}", i))
			.spawn(move || {
				// note that we are creating this inside a new thread.
				let mut worker = Worker::new_from_thread(
					master_id,
					state_ptr,
					worker_to_master_tx,
					master_to_worker_rx,
					from_others_rx,
				);

				// wait for the master to send you the btree-map of the send queue to all other
				// threads.
				match worker.from_master.recv().unwrap().payload {
					MessagePayload::FinalizeSetup(data) => worker.to_others = data,
					_ => panic!("Received unexpected message"),
				};

				log::info!("Worker initialized. Parking self.");

				// master will initially park the thread in initialization, and unpark it once done.
				thread::park();

				// run
				if test_run {
					worker.test_run();
				} else {
					worker.run();
				}
			})
			.expect("Failed to spawn a new worker thread.");

		let worker_id = worker_handle.thread().id().as_u64().into();
		to_workers.insert(worker_id, from_others_tx);
		let handle = WorkerHandle::new(master_to_worker_tx, worker_handle);
		master.workers.insert(worker_id, handle);
	}

	assert_eq!(to_workers.len(), n);
	info!("created {} worker threads.", n);

	master.workers.iter().for_each(|(_, handle)| {
		let message = MessagePayload::FinalizeSetup(to_workers.clone()).into();
		handle
			.send
			.send(message)
			.expect("Can always send FinalizeSetup; qed");
	});

	master
}

fn init_logger() {
	use colored::*;
	use std::io::Write;

	let _ = env_logger::Builder::from_env("RUST_LOG")
		.format(|buf, record| {
			writeln!(
				buf,
				"{} {} [{} ({})] - {}",
				chrono::Local::now()
					.format("%Y-%m-%dT%H:%M:%S")
					.to_string()
					.italic()
					.dimmed(),
				match record.level() {
					Level::Error => "Error".red(),
					Level::Warn => "Warn".yellow(),
					Level::Info => "Info".green(),
					Level::Debug => "Debug".magenta(),
					Level::Trace => "Trace".blue(),
				}
				.bold(),
				thread::current().name().unwrap_or("Unnamed thread.").bold(),
				thread::current().id().as_u64(),
				record.args()
			)
		})
		.try_init();
}

fn main() {
	init_logger();

	let num_cpus = num_cpus::get();
	let master = spawn_workers::<Pool, RoundRobin>(num_cpus - 1, false);

	// master.run().
	master.join_all().unwrap();
}
