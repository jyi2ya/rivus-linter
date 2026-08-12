// check-pass
// Tests that occurrence numbering follows unified HIR order:
// function reference first, then direct call — not the reverse.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, rivus::rvs_untested_ok_fn)]
#![allow(rivus::rvs_missing_doc)]

fn rvs_ref_first() {
    let _ = 0;
}
fn rvs_call_second() {
    let _ = 0;
}

fn rvs_order_BI() {
    let _pointer: fn() = rvs_ref_first; // source order: 1st, should be occurrence 0
    rvs_call_second();                   // source order: 2nd, should be occurrence 1
}
