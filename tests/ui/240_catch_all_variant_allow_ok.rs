// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]

enum ParseError {
    #[allow(rivus::rvs_catch_all_error_variant, reason = "handled at this boundary")]
    Other,
}
