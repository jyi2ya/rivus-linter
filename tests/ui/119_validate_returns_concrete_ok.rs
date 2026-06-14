// check-pass
// compile-flags: --test
#![allow(non_snake_case)]

/// # Panics
fn rvs_parse_email_P(raw: &str) -> Result<String, String> {
    Ok(raw.to_string())
}

#[test]
fn test_20260612_validate_returns_concrete_ok() {
    rvs_parse_email_P("test@example.com").unwrap();
}
