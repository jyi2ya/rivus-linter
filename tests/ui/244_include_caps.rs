// check-pass
// Calls inside include!'d files are collected: both chain functions
// live in 244_include_caps.inc, and rvs_entry_BIS only keeps its caps
// if the include-file edges are part of the callgraph.
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn, reason = "fixture isolates include source tracking")]

fn rvs_blocking_BIS() {
    let _ = std::fs::remove_file("fixture-marker");
}

include!("244_include_caps.inc");
