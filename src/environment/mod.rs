pub(crate) mod analysis_commands;
pub(crate) mod callgraph_cache;
pub(crate) mod capsmap_loader;
pub(crate) mod cargo_targets;
pub(crate) mod fs_guard;
pub(crate) mod graph_render;
pub(crate) mod infer_commands;
pub(crate) mod lint_driver;
pub(crate) mod rename;
pub(crate) mod report_commands;
pub(crate) mod setup;
pub(crate) mod workspace;

/// Saturating-free counter accumulation with a labeled overflow error.
///
/// Precondition: `current` is a running total and `delta` a fresh increment
/// from the same counter; neither may be the saturation sentinel, otherwise
/// the very first accumulation is doomed and the caller passed a corrupted
/// count.
pub(crate) fn rvs_checked_count_sum(
    current: usize,
    delta: usize,
    label: &str,
) -> Result<usize, String> {
    debug_assert!(current < usize::MAX, "running total must not be saturated");
    debug_assert!(delta < usize::MAX, "increment must not be saturated");
    current
        .checked_add(delta)
        .ok_or_else(|| format!("{label} overflow"))
}
