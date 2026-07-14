// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

#[derive(Debug)]
struct Holder {
    #[expect(rivus::rvs_borrowed_param)]
    value: &'static String,
}

fn rvs_run() {
    #[expect(rivus::rvs_error_swallow)]
    let _ = Result::<(), ()>::Err(()).ok();
}

#[test]
fn test_20260714_scoped_expectations_ok() {
    rvs_run();
    let holder = Holder {
        value: Box::leak(Box::new(String::new())),
    };
    assert!(holder.value.is_empty());
}
