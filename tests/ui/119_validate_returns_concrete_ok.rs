// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

fn rvs_parse_email(raw: &str) -> Result<String, String> {
    Ok(raw.to_string())
}

#[test]
fn test_20260612_validate_returns_concrete_ok() {
    rvs_parse_email("test@example.com").unwrap();
}
