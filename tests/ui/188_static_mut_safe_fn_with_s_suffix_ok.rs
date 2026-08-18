// check-pass
// Safe fn reading a static mut: U is measured from the body fact, the name
// only carries S. This is the canonical safe-fn shape after A/C/U became
// measured (signature or body facts) rather than named capabilities.
#![allow(non_snake_case)]

static mut COUNTER: u32 = 0;

/// Reads the counter.
fn rvs_read_counter_S() -> u32 {
    unsafe { COUNTER }
}

fn main() {
    let value = rvs_read_counter_S();
    std::process::exit(value.min(0) as i32);
}
