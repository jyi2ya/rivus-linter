// check-pass
// const-eval calls are compile-time only; they are excluded from the
// runtime callgraph and do not propagate capabilities.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_implicit_execution)]
#![allow(rivus::rvs_untested_good_fn)]

const fn rvs_count() -> usize {
    1
}

fn rvs_inline_const_call() {
    let _ = const { rvs_count() };
}

fn rvs_repeat_const_call() {
    let _ = [0; rvs_count()];
}

fn rvs_asm_const_call() {
    unsafe {
        core::arch::asm!("/* {} */", const rvs_count());
    }
}
