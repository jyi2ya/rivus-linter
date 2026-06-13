#![allow(non_snake_case)]

fn rvs_safe_call() {
    let _ = std::panic::catch_unwind(|| {});
}
