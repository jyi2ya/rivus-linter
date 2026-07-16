#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

struct Inner {
    result: Result<(), u8>,
}

struct Outer {
    inner: Inner,
}

struct SetErrorOnDrop<'a>(&'a mut Inner);

impl Drop for SetErrorOnDrop<'_> {
    fn drop(&mut self) {
        self.0.result = Err(3);
    }
}

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut outer = Outer {
        inner: Inner { result: Ok(()) },
    };
    outer.inner.result = Ok(());
    {
        let _guard = SetErrorOnDrop(&mut outer.inner);
    }
    outer.inner.result
}
