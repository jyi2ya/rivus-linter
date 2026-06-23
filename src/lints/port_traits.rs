use rustc_hir::ItemKind;
use rustc_lint::LateContext;

/// Suffixes that mark a trait as a Port (hexagonal architecture).
/// Methods on such traits get P capability automatically.
pub(crate) const PORT_SUFFIXES: &[&str] = &["Repository", "Client"];

/// Check if a trait name ends with a Port suffix.
pub(crate) fn rvs_is_port_name(name: &str) -> bool {
    PORT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

/// Collect the def_id of all Port traits in the current crate.
/// Returns a set of DefIds that are Port traits.
pub(crate) fn rvs_collect_port_traits_S(
    cx: &LateContext<'_>,
) -> std::collections::HashSet<rustc_span::def_id::DefId> {
    let mut port_traits = std::collections::HashSet::new();
    let krate = cx.tcx.hir_crate_items(());
    for owner in krate.owners() {
        let node = cx.tcx.hir_owner_node(owner);
        if let rustc_hir::OwnerNode::Item(item) = node {
            if let ItemKind::Trait(..) = &item.kind {
                let name = cx.tcx.item_name(owner.def_id).to_string();
                if rvs_is_port_name(&name) {
                    port_traits.insert(owner.def_id.to_def_id());
                }
            }
        }
    }
    port_traits
}
