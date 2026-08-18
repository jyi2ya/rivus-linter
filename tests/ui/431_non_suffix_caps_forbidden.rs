// A/C/U in a suffix is forbidden: they are measured from the signature
// and body facts, never recorded in the name. Each offending name gets
// one dedicated NonSuffixCapInSuffix error with a corrected expected name.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_empty_fn)]
#![allow(rivus::rvs_missing_doc)]
#![allow(rivus::rvs_missing_debug_assert)]
#![allow(rivus::rvs_missing_safety_doc)]
#![allow(rivus::rvs_non_alphabetical_suffix)]

async fn rvs_fetch_AI(x: i32) -> i32 {
    x
}

const fn rvs_factorial_C(n: u32) -> u32 {
    if n <= 1 { 1 } else { n }
}

unsafe fn rvs_dangerous_U() {}

fn rvs_mixed_ACUB() {}
