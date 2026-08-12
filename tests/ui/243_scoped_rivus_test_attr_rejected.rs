// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]

mod first {
    #[test]
    #[allow(rivus::rvs_duplicate_test, reason = "fixture checks node-level lint control")]
    fn test_20260714_duplicate_test_allow_ok() {}
}

mod second {
    #[test]
    #[allow(rivus::rvs_duplicate_test, reason = "fixture checks node-level lint control")]
    fn test_20260714_duplicate_test_allow_ok() {}
}
