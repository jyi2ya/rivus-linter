// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

fn rvs_dead_branch_body_lints() {
    if false {
        let _ = std::thread::spawn(|| {});
        let _ = std::any::type_name::<u8>();
        let _ = std::panic::catch_unwind(|| {});
        let _: Option<()> = Result::<(), u8>::Err(1).ok();
        drop(Result::<String, u8>::Err(2));
    }
}

fn rvs_uninvoked_closure_body_lints() {
    let _never_called = || {
        let _ = std::thread::spawn(|| {});
        let _ = std::any::type_name::<u16>();
        let _ = std::panic::catch_unwind(|| {});
        let _: Option<()> = Result::<(), u8>::Err(3).ok();
        drop(Result::<String, u8>::Err(4));
    };
}

#[test]
fn test_20260801_dead_hir_body_lints() {
    rvs_dead_branch_body_lints();
    rvs_uninvoked_closure_body_lints();
}
