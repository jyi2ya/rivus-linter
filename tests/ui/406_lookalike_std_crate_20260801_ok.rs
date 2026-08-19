// check-pass
// compile-flags: --test --crate-name=std
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_non_rvs_fn)]
#![allow(rivus::rvs_untested_good_fn)]

mod thread {
    pub mod functions {
        pub fn spawn<F>(callback: F) -> F {
            callback
        }
    }
}

mod any {
    pub fn type_name<T>() -> &'static str {
        let _ = core::mem::size_of::<T>();
        "lookalike"
    }
}

mod panic {
    pub fn catch_unwind<F>(callback: F) -> F {
        callback
    }
}

fn rvs_deferred_effect_BS() {
    let _ = std::env::var("HOME");
}

fn rvs_use_lookalike_std_BST() {
    let _ = thread::functions::spawn(|| rvs_deferred_effect_BS());
    let _ = any::type_name::<u8>();
    let _ = panic::catch_unwind(|| rvs_deferred_effect_BS());
}

#[test]
fn test_20260801_lookalike_std_crate() {
    rvs_use_lookalike_std_BST();
}
