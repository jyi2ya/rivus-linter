#![allow(non_snake_case)]

static VALUE: u8 = 0;

fn rvs_inline_asm_sym_static() {
    unsafe {
        core::arch::asm!("/* {} */", sym VALUE);
    }
}
