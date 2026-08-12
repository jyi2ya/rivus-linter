// compile-flags: --test
// Tests that in and label inline asm operands have their calls collected.
// const operands are compile-time only and excluded from the runtime
// callgraph; only in(reg) and label calls produce edges here.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_implicit_execution)]
#![allow(rivus::rvs_untested_good_fn)]

const fn rvs_first_B() -> usize {
    1
}

fn rvs_second_BI() -> usize {
    2
}

fn rvs_third_BI() {
    let _ = 3;
}

fn rvs_mixed_asm_operands() {
    unsafe {
        core::arch::asm!(
            "/* {0} {1} {2} */",
            const rvs_first_B(),
            in(reg) rvs_second_BI(),
            label {
                rvs_third_BI();
            },
        );
    }
}

#[test]
fn test_20260810_mixed_asm_operands() {
    rvs_mixed_asm_operands();
}
