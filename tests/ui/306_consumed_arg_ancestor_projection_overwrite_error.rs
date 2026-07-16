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

fn rvs_process(data: String) -> Result<(), u8> {
    drop(data);
    let mut outer = Outer {
        inner: Inner { result: Ok(()) },
    };
    outer.inner.result = Ok(());
    outer.inner = Inner { result: Err(3) };
    outer.inner.result
}
