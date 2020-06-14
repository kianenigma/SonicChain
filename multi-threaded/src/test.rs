use crate::spawn_workers;

#[test]
fn spawn_threads_works() {
	let master = spawn_workers(4);
	std::thread::sleep(std::time::Duration::from_millis(500));
	assert_eq!(master.workers.len(), 4);
}
