// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_implicit_execution)]

fn rvs_read_BIS() -> usize {
    let _ = std::fs::remove_file("fixture-marker");
    1
}

fn rvs_inline_asm_operand_BIS() {
    unsafe {
        core::arch::asm!(
            "/* {value} */",
            value = inout(reg) rvs_read_BIS() => _,
        );
    }
}

#[test]
fn test_20260714_inline_asm_inout_call() {
    rvs_inline_asm_operand_BIS();
}
