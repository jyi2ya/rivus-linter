// check-pass
// compile-flags: --test
// const-eval calls (inline const blocks, array repeat len, asm const
// operand) are compile-time only and excluded from the runtime callgraph.
// The callers stay pure despite calling the const fn, because those calls
// never execute at runtime. A const fn honestly carries no propagated
// caps (no I/O, no statics), so this exclusion is not observable through
// naming views: the fixture documents the callers' purity instead.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_implicit_execution)]
#![allow(rivus::rvs_untested_good_fn)]

const fn rvs_count() -> usize {
    1
}

fn rvs_inline_const_caller() {
    let _ = const { rvs_count() };
}

fn rvs_repeat_const_caller() {
    let _ = [0; rvs_count()];
}

fn rvs_asm_const_caller() {
    unsafe {
        core::arch::asm!("/* {} */", const rvs_count());
    }
}

#[test]
fn test_20260811_const_calls_excluded_from_runtime_graph() {
    rvs_inline_const_caller();
    rvs_repeat_const_caller();
    rvs_asm_const_caller();
}
