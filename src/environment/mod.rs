pub(crate) mod analysis_commands;
pub(crate) mod callgraph_cache;
pub(crate) mod capsmap_loader;
pub(crate) mod cargo_targets;
pub(crate) mod fs_guard;
pub(crate) mod infer_commands;
pub(crate) mod lint_driver;
pub(crate) mod rename;
pub(crate) mod report_commands;
pub(crate) mod setup;
pub(crate) mod workspace;

/// Saturating-free counter accumulation with a labeled overflow error.
pub(crate) fn rvs_checked_count_sum(
    current: usize,
    delta: usize,
    label: &str,
) -> Result<usize, String> {
    debug_assert!(current.checked_add(0).is_some(), "current count is valid");
    debug_assert!(delta.checked_add(0).is_some(), "delta count is valid");
    current
        .checked_add(delta)
        .ok_or_else(|| format!("{label} overflow"))
}
