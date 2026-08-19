// check-pass
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_good_fn)]
#![allow(rivus::rvs_validate_returns_unit)]

/// Effectful filesystem preflight check — not a value validator.
/// Returning unit errors is intentional for effectful preflight, so the
/// shape lint is acknowledged at the crate root.
fn rvs_validate_output_parent_BIS(path: &std::path::Path) -> Result<(), String> {
    if path.parent().is_none() {
        return Err("missing parent".to_string());
    }
    Ok(())
}
