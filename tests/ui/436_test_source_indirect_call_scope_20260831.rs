// check-pass
// compile-flags: --test
// Test-source indirect calls inherit the enclosing owner's test diagnostic
// scope: closures called inside `mod tests` keep their call observations for
// coverage but produce no RVS_UNSUPPORTED_INDIRECT_CALL production warning.
// The same closure call in production code must keep warning (fixture 419).
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, rivus::rvs_untested_ok_fn)]
#![allow(rivus::rvs_missing_doc)]
#![allow(rivus::rvs_test_name_format)]

#[cfg(test)]
mod tests {
    #[test]
    fn test_20260831_closure_call_in_test_module() {
        let make_num = || 5;
        assert_eq!(make_num(), 5);
    }

    fn rvs_helper() -> i32 {
        let triple = |value| value * 3;
        triple(2)
    }

    #[test]
    fn test_20260831_helper_closure() {
        assert_eq!(rvs_helper(), 6);
    }
}
