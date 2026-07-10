#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(
    rivus::rvs_missing_doc,
    rivus::rvs_untested_good_fn,
    rivus::rvs_untested_ok_fn
)]

pub fn rvs_process() -> String {
    std::any::type_name::<i32>().to_string()
}
