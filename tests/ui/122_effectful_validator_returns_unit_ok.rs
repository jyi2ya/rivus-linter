// check-pass
#![allow(non_snake_case)]

/// Effectful filesystem preflight check — not a value validator.
fn rvs_validate_output_parent_BIS(path: &std::path::Path) -> Result<(), String> {
    if path.parent().is_none() {
        return Err("missing parent".to_string());
    }
    Ok(())
}
