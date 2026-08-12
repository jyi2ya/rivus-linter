#![allow(non_snake_case)]

fn rvs_thread_builder_spawn_BIST() {
    let handle = std::thread::Builder::new().spawn(|| {}).unwrap();
    handle.join().unwrap();
}
