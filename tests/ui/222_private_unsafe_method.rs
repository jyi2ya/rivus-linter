#![allow(non_snake_case)]

struct Worker;

impl Worker {
    unsafe fn rvs_run_U(&self) {}
}

trait HiddenWorker {
    unsafe fn rvs_required_U(&self);

    unsafe fn rvs_provided_U(&self) {}
}
