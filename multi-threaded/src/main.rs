#![feature(thread_id_value)]

use log::*;
use primitives::*;
use state::*;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;

mod master;
mod message;
mod worker;

use master::*;
use message::*;
use worker::*;

type State = runtime::RuntimeState;

/// Spawn `n` new worker threads.
///
/// This _should only be called once_.
fn spawn_workers(n: usize) -> Master {
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
		let predicted_id = (i + 1) as ThreadId + master_id;
		to_workers.insert(predicted_id, from_others_tx);

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

				// this id must be equal to what we predicted.
				assert_eq!(predicted_id, worker.id);

				// wait for the master to send you the btree-map of the send queue to all other threads.
				match worker.from_master.recv().unwrap().payload {
					MessagePayload::FinalizeSetup(data) => worker.to_others = data,
					_ => panic!("Received unexpected message"),
				};

				info!("Worker initialized. Parking self.");

				// master will initially park the thread in initialization, and unpark it once done.
				thread::park();

				// run
				#[cfg(test)]
				worker.test_run();
				#[cfg(not(test))]
				worker.run();
			})
			.expect("Failed to spawn a new worker thread.");

		let worker_id = worker_handle.thread().id().as_u64().into();
		let handle = WorkerHandle::new(master_to_worker_tx, worker_handle);
		master.workers.insert(worker_id, handle);
	}

	assert_eq!(to_workers.len(), n);
	info!("created {} worker threads.", n);

	master.workers.iter().for_each(|(_, handle)| {
		let message = Message {
			from: master.id,
			payload: MessagePayload::FinalizeSetup(to_workers.clone()),
		};
		handle
			.send
			.send(message)
			.expect("Failed to send FinalizeSetup");
	});

	master
}

fn main() {
	use std::io::Write;
	env_logger::Builder::new()
		.format(|buf, record| {
			writeln!(
				buf,
				"{} [{}][{}] - {}",
				chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
				record.level(),
				thread::current().name().unwrap_or("Unnamed thread."),
				record.args()
			)
		})
		.filter(None, LevelFilter::Info)
		.init();
	let num_cpus = num_cpus::get();
	let master = spawn_workers(num_cpus - 1);

	master.join_all().unwrap();
}

#[cfg(test)]
mod main_tests {
	use super::*;

	#[test]
	fn spawn_threads_works() {
		let master = spawn_workers(4);
		std::thread::sleep(std::time::Duration::from_millis(500));
		assert_eq!(master.workers.len(), 4);

		// unpark all workers. Then they will run their test_run automatically.
		master.unpark_all();

		// start the master as well.
		master.run_test();

		// assert to join all okay
		assert!(master.join_all().is_ok())
	}
}
