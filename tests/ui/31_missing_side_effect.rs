#![expect(non_snake_case)]

static CONFIG: &str = "test";

fn rvs_get_config() -> &'static str {
    CONFIG
}
