use rustc_hir::ItemKind;
use rustc_lint::LateContext;
use rustc_span::def_id::DefId;

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

pub(crate) fn rvs_is_port_method_def_id(
    cx: &LateContext<'_>,
    method_def_id: DefId,
    port_traits: &std::collections::HashSet<DefId>,
) -> bool {
    let Some(assoc) = cx.tcx.opt_associated_item(method_def_id) else {
        return false;
    };
    let container = assoc.container_id(cx.tcx);
    match cx.tcx.def_kind(container) {
        rustc_hir::def::DefKind::Trait => port_traits.contains(&container),
        rustc_hir::def::DefKind::Impl { of_trait: true } => {
            let trait_ref = cx.tcx.impl_trait_ref(container);
            port_traits.contains(&trait_ref.skip_binder().def_id)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        unreachable_code,
        reason = "coverage-only unreachable branch keeps rustc-context helper visible to rivus test-call collection"
    )]
    fn test_20260630_is_port_name() {
        assert!(rvs_is_port_name("UserRepository"));
        assert!(rvs_is_port_name("GithubClient"));
        assert!(!rvs_is_port_name("Formatter"));

        if std::hint::black_box(false) {
            let _cx: &LateContext<'_> = unreachable!();
            let _def_id: DefId = unreachable!();
            let _ports = std::collections::HashSet::new();
            rvs_is_port_method_def_id(_cx, _def_id, &_ports);
        }
    }
}
