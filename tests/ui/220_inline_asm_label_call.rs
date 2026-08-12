// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_implicit_execution)]

fn rvs_read_BI() {
    let _ = 42;
}

fn rvs_inline_asm_label() {
    unsafe {
        core::arch::asm!("jmp {}", label {
            rvs_read_BI();
        });
    }
}

#[test]
fn test_20260714_inline_asm_label_call() {
    rvs_inline_asm_label();
}
