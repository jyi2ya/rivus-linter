use rustc_hash::FxHashMap;
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_lint::LateContext;
use rustc_span::Span;

use super::utils;

/// Cached value type describing one impl block's self-type identity.
/// See `docs/theory/compiler-identity-cache.md` for the caching contract.
#[derive(Debug, Clone)]
pub(crate) struct ImplTypeIdentity {
    pub(crate) readable_path: String,
    pub(crate) marker: String,
    pub(crate) is_nominal_path: bool,
}

/// Per-crate memoization of deterministic identity construction.
///
/// `TyCtxt` queries are immutable during the late lint pass, so every
/// cached entry is a pure function of immutable compiler state: no
/// invalidation, eviction, or cross-crate reuse is possible or needed.
/// The cache lives exactly as long as one `RivusLintPass` (one rustc
/// process, one crate). Negative results are cached like positive ones.
#[derive(Debug, Default)]
pub(crate) struct IdentityCache {
    def_paths: FxHashMap<DefId, String>,
    impl_types: FxHashMap<DefId, Option<ImplTypeIdentity>>,
    span_sources: FxHashMap<Span, Option<String>>,
    generated_bases: FxHashMap<DefId, Option<String>>,
    generated_ordinals: Option<FxHashMap<LocalDefId, usize>>,
}

impl IdentityCache {
    pub(crate) fn rvs_new() -> Self {
        Self::default()
    }

    pub(crate) fn rvs_def_path_BM(&mut self, cx: &LateContext<'_>, did: DefId) -> String {
        if let Some(path) = self.def_paths.get(&did) {
            return path.clone();
        }
        let path = utils::rvs_compute_def_path_BM(self, cx, did);
        self.def_paths.insert(did, path.clone());
        path
    }

    pub(crate) fn rvs_impl_type_identity_BM(
        &mut self,
        cx: &LateContext<'_>,
        impl_def_id: DefId,
    ) -> Option<ImplTypeIdentity> {
        if let Some(identity) = self.impl_types.get(&impl_def_id) {
            return identity.clone();
        }
        let identity = utils::rvs_compute_impl_type_identity_BM(self, cx, impl_def_id);
        self.impl_types.insert(impl_def_id, identity.clone());
        identity
    }

    pub(crate) fn rvs_span_source_identity_M(
        &mut self,
        cx: &LateContext<'_>,
        span: Span,
    ) -> Option<String> {
        if let Some(identity) = self.span_sources.get(&span) {
            return identity.clone();
        }
        let identity = utils::rvs_compute_span_source_identity(cx, span);
        self.span_sources.insert(span, identity.clone());
        identity
    }

    pub(crate) fn rvs_definition_identity_BM(
        &mut self,
        cx: &LateContext<'_>,
        did: DefId,
    ) -> Option<String> {
        let definition_span = cx.tcx.def_span(did);
        if definition_span.from_expansion() {
            return self.rvs_generated_definition_identity_BM(cx, did);
        }
        if !utils::rvs_is_body_nested_definition(cx, did) {
            return None;
        }
        self.rvs_span_source_identity_M(cx, definition_span)
            .map(|definition| format!("definition={definition}"))
    }

    pub(crate) fn rvs_generated_definition_identity_BM(
        &mut self,
        cx: &LateContext<'_>,
        did: DefId,
    ) -> Option<String> {
        let mut identity = self.rvs_generated_base_identity_BM(cx, did)?;
        if let Some(ordinal) = self.rvs_generated_ordinal_BM(cx, did) {
            identity.push_str("|same-source-ordinal=");
            identity.push_str(&ordinal.to_string());
        }
        Some(identity)
    }

    fn rvs_generated_base_identity_BM(
        &mut self,
        cx: &LateContext<'_>,
        did: DefId,
    ) -> Option<String> {
        if let Some(base) = self.generated_bases.get(&did) {
            return base.clone();
        }
        let base = utils::rvs_compute_generated_base_identity_BM(self, cx, did);
        self.generated_bases.insert(did, base.clone());
        base
    }

    fn rvs_generated_ordinal_BM(&mut self, cx: &LateContext<'_>, did: DefId) -> Option<usize> {
        let local_did = did.as_local()?;
        if self.generated_ordinals.is_none() {
            let index = self.rvs_build_generated_ordinal_index_BM(cx);
            self.generated_ordinals = Some(index);
        }
        self.generated_ordinals
            .as_ref()
            .and_then(|index| index.get(&local_did).copied())
    }

    /// Single pass over the crate owners: every owner's generated base
    /// identity is computed once, and same-(kind, base) groups keep their
    /// owner-order ordinals. This replaces the previous per-definition
    /// full-crate rescan (O(generated x owners)).
    fn rvs_build_generated_ordinal_index_BM(
        &mut self,
        cx: &LateContext<'_>,
    ) -> FxHashMap<LocalDefId, usize> {
        let mut entries = Vec::new();
        for owner in cx.tcx.hir_crate_items(()).owners() {
            let candidate = owner.def_id.to_def_id();
            let definition_kind = cx.tcx.def_kind(candidate);
            let base = self.rvs_generated_base_identity_BM(cx, candidate);
            if let Some(base) = base {
                entries.push((owner.def_id, (definition_kind, base)));
            }
        }
        rvs_assign_repetition_ordinals(entries)
    }
}

/// Assigns a repetition ordinal to every entry whose group key is shared
/// with at least one other entry. Ordinals follow the input (owner) order
/// and start at zero; singleton groups produce no entry.
pub(crate) fn rvs_assign_repetition_ordinals<K, G>(entries: Vec<(K, G)>) -> FxHashMap<K, usize>
where
    K: Copy + std::hash::Hash + Eq,
    G: std::hash::Hash + Eq,
{
    let mut group_sizes: FxHashMap<&G, usize> = FxHashMap::default();
    for (_, group) in &entries {
        *group_sizes.entry(group).or_insert(0) += 1;
    }
    let mut ordinals = FxHashMap::default();
    let mut positions: FxHashMap<&G, usize> = FxHashMap::default();
    for (key, group) in &entries {
        if group_sizes.get(group).is_none_or(|size| *size < 2) {
            continue;
        }
        let position = positions.entry(group).or_insert(0);
        ordinals.insert(*key, *position);
        *position += 1;
    }
    ordinals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_20260901_repetition_ordinals_follow_owner_order() {
        let entries = vec![
            (1u32, "a"),
            (2u32, "b"),
            (3u32, "a"),
            (4u32, "c"),
            (5u32, "a"),
        ];
        let ordinals = rvs_assign_repetition_ordinals(entries);
        let mut names: Vec<u32> = ordinals.keys().copied().collect();
        names.sort_unstable();
        let mut output = String::new();
        for name in names {
            output.push_str(&format!("{name}={}\n", ordinals[&name]));
        }
        crate::test_support::rvs_snapshot_BIS(
            "test_20260901_repetition_ordinals_follow_owner_order",
            &output,
        );

        assert_eq!(ordinals.get(&1), Some(&0));
        assert_eq!(ordinals.get(&3), Some(&1));
        assert_eq!(ordinals.get(&5), Some(&2));
        assert!(!ordinals.contains_key(&2));
        assert!(!ordinals.contains_key(&4));
    }

    #[test]
    fn test_20260901_repetition_ordinals_group_by_full_key() {
        let kind_a = 0u8;
        let kind_b = 1u8;
        let entries = vec![(1u32, (kind_a, "same")), (2u32, (kind_b, "same"))];
        let ordinals = rvs_assign_repetition_ordinals(entries);
        let output = format!(
            "first={}\nsecond={}\n",
            ordinals.contains_key(&1),
            ordinals.contains_key(&2)
        );
        crate::test_support::rvs_snapshot_BIS(
            "test_20260901_repetition_ordinals_group_by_full_key",
            &output,
        );

        // Same base text under different definition kinds never groups.
        assert!(!ordinals.contains_key(&1));
        assert!(!ordinals.contains_key(&2));
    }
}
