#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_implicit_execution)]

static VALUE: u8 = 0;

fn rvs_inline_asm_sym_static() {
    unsafe {
        core::arch::asm!("/* {} */", sym VALUE);
    }
}
