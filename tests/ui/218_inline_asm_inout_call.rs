// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_implicit_execution)]

fn rvs_read_BI() -> usize {
    1
}

fn rvs_inline_asm_operand() {
    unsafe {
        core::arch::asm!(
            "/* {value} */",
            value = inout(reg) rvs_read_BI() => _,
        );
    }
}

#[test]
fn test_20260714_inline_asm_inout_call() {
    rvs_inline_asm_operand();
}
