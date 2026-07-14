// compile-flags: --test
#![allow(non_snake_case)]

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
