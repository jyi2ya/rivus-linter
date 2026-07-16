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

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    {
        let _guard = SetErrorOnDrop(&mut result);
    }
    result
}
