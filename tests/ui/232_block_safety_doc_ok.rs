// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]

/** Performs work.

# Safety

The caller must uphold the invariant.
*/
unsafe fn rvs_block_doc() {
    let _ = 1;
}
