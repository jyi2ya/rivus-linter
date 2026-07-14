#![allow(non_snake_case)]

fn rvs_thread_builder_spawn_BIS() {
    let handle = std::thread::Builder::new().spawn(|| {}).unwrap();
    handle.join().unwrap();
}
