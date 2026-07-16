#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

struct Guard<'a>(&'a mut Result<(), u8>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        *self.0 = Err(1);
    }
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut result = Ok(());
    {
        let _carrier: Result<(), Guard<'_>> = Err(Guard(&mut result));
    }
    result
}
