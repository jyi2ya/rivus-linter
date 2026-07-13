use rustc_lint::LateContext;
use rustc_span::def_id::DefId;

/// Suffixes that mark a trait as a Port (hexagonal architecture).
/// Methods on such traits get P capability automatically.
pub(crate) const PORT_SUFFIXES: &[&str] = &["Repository", "Client"];

/// Check if a trait name ends with a Port suffix.
pub(crate) fn rvs_is_port_name(name: &str) -> bool {
    PORT_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

pub(crate) fn rvs_is_local_port_trait_S(cx: &LateContext<'_>, def_id: DefId) -> bool {
    def_id.is_local() && rvs_is_port_name(cx.tcx.item_name(def_id).as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_20260630_is_port_name() {
        assert!(rvs_is_port_name("UserRepository"));
        assert!(rvs_is_port_name("GithubClient"));
        assert!(!rvs_is_port_name("Formatter"));
    }
}
