#![allow(non_snake_case)]

fn rvs_catch_it() {
    let _ = std::panic::catch_unwind(|| {});
}
