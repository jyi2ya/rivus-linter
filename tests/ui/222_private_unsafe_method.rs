#![allow(non_snake_case)]

struct Worker;

impl Worker {
    unsafe fn rvs_run(&self) {}
}

trait HiddenWorker {
    unsafe fn rvs_required(&self);

    unsafe fn rvs_provided(&self) {}
}
