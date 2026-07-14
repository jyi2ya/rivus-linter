#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

fn rvs_run() {
    let result = Result::<(), ()>::Err(());
    let _ = Result::ok(result);

    let result = Result::<(), ()>::Err(());
    let _ = Result::unwrap_or_default(result);
}
