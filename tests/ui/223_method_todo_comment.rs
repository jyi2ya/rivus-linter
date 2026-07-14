#![allow(non_snake_case)]

struct Worker;

impl Worker {
    fn rvs_inherent(&self) {
        // TODO: implement the inherent method.
    }
}

trait WorkerTrait {
    fn rvs_provided(&self) {
        // FIXME: implement the provided method.
    }
}

trait ImplementedWorker {
    fn rvs_implemented(&self);
}

impl ImplementedWorker for Worker {
    fn rvs_implemented(&self) {
        // TODO: implement the trait method.
    }
}
