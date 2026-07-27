#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]

trait Validator {
    fn rvs_validate(&self, raw: &str) -> Result<(), String>;
}
