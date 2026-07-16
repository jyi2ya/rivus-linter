#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

struct SetErrorOnDrop<'a>(&'a mut Result<(), u8>);

impl Drop for SetErrorOnDrop<'_> {
    fn drop(&mut self) {
        *self.0 = Err(1);
    }
}

fn rvs_ignore<F>(callback: F) {
    let _callback = std::hint::black_box(callback);
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    let guard = SetErrorOnDrop(&mut result);
    let callback = move || {
        let _guard = std::hint::black_box(&guard);
    };
    rvs_ignore(callback);
    result
}
