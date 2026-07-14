#![allow(non_snake_case)]

const fn rvs_count_B() -> usize {
    1
}

fn rvs_inline_const_call() {
    let _ = const { rvs_count_B() };
}

fn rvs_repeat_const_call() {
    let _ = [0; rvs_count_B()];
}

fn rvs_asm_const_call() {
    unsafe {
        core::arch::asm!("/* {} */", const rvs_count_B());
    }
}
