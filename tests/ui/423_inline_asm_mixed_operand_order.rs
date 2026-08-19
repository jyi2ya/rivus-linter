// check-pass
// compile-flags: --test
// Tests that in and label inline asm operands have their calls
// collected. const operands are compile-time only and excluded from
// the runtime callgraph; a const fn honestly has no propagated caps,
// so that exclusion is not name-observable — only the in/label calls
// carry the BIS propagation this fixture verifies.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_unsupported_implicit_execution)]
#![allow(rivus::rvs_unsupported_indirect_call)]
#![allow(rivus::rvs_untested_good_fn)]

const fn rvs_first() -> usize {
    1
}

fn rvs_second_BIS() -> usize {
    let _ = std::fs::remove_file("fixture-marker");
    2
}

fn rvs_third_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

fn rvs_mixed_asm_operands_BIS() {
    unsafe {
        core::arch::asm!(
            "/* {0} {1} {2} */",
            const rvs_first(),
            in(reg) rvs_second_BIS(),
            label {
                rvs_third_BIS();
            },
        );
    }
}

#[test]
fn test_20260810_mixed_asm_operands() {
    rvs_mixed_asm_operands_BIS();
}
