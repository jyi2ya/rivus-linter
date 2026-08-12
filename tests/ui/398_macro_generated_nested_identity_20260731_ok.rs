// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

static VALUE: u8 = 1;

fn rvs_side_effect_S() {
    let _ = VALUE;
}

fn rvs_pure() {
    let value = 1_u8;
    let _ = value;
}

macro_rules! invoke_generated {
    ($callee:path) => {{
        fn rvs_generated_S() {
            $callee();
        }
        rvs_generated_S();
    }};
}

fn rvs_invoke_both_S() {
    invoke_generated!(rvs_side_effect_S);
    invoke_generated!(rvs_pure);
}

#[test]
fn test_20260731_macro_generated_nested_identity() {
    rvs_invoke_both_S();
}
