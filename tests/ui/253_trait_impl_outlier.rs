#![allow(non_snake_case)]

trait FromString {
    fn rvs_parse(input: &str) -> usize;
}

unsafe extern "Rust" {
    fn rvs_environment_effect_S();
}

struct Alpha;
struct Beta;
struct EnvValue;

impl FromString for Alpha {
    fn rvs_parse(_input: &str) -> usize {
        0
    }
}

impl FromString for Beta {
    fn rvs_parse(_input: &str) -> usize {
        0
    }
}

impl FromString for EnvValue {
    fn rvs_parse(_input: &str) -> usize {
        unsafe { rvs_environment_effect_S() };
        0
    }
}
