// check-pass
// compile-flags: --test
// The call inside format! arguments is macro-generated: if calls in
// macro expansions were not collected, rvs_macro_caller_BIS would lose
// its propagated caps and the naming contract would fail.
#![allow(non_snake_case)]

fn rvs_effect_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_macro_caller_BIS() {
    let _ = format!("calling {:?}", rvs_effect_BIS());
}

#[test]
fn test_20260612_violation_in_macro() {
    rvs_macro_caller_BIS();
}
