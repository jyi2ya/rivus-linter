#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

trait Observer {
    fn rvs_observe_M(&self, result: &mut Result<(), u8>);
}

fn rvs_process<T: Observer>(data: String, observer: &T) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    observer.rvs_observe_M(&mut result);
    result
}
