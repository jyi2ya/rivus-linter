#![allow(non_snake_case)]

trait FromString {
    fn rvs_parse(input: &str) -> usize;
}

static ENV_FLAG: u8 = 0;

unsafe fn rvs_environment_effect_S() -> u8 {
    ENV_FLAG
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
