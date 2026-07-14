// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

mod outer {
    pub mod anyhow {
        #[derive(Debug)]
        pub struct Error;
    }
}

fn rvs_use_local_anyhow_module() {
    use outer::anyhow::Error;

    let _ = core::mem::size_of::<Error>();
}

#[test]
fn test_20260714_banned_local_module_ok() {
    rvs_use_local_anyhow_module();
}
