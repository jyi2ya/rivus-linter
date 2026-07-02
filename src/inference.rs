use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::artifacts::FnBehavior;
use crate::capability::{Capability, CapabilityPolicy, CapabilitySet};
use crate::capsmap;

/// Build a "method@trait_path" → set-of-keys index from callgraph keys.
pub(crate) fn rvs_build_impl_index(
    callgraph: &BTreeMap<String, FnBehavior>,
) -> HashMap<String, Vec<String>> {
    let mut idx: HashMap<String, Vec<String>> = HashMap::new();
    for key in callgraph.keys() {
        if let Some(at_pos) = key.find('@') {
            let (method, suffix_with_sep) = key.split_at(at_pos);
            let Some(suffix) = suffix_with_sep.strip_prefix('@') else {
                continue;
            };
            let method_name = method.rsplit("::").next().unwrap_or(method);
            let lookup = format!("{method_name}@{suffix}");
            idx.entry(lookup).or_default().push(key.clone());
        }
    }
    idx
}

/// Infer capabilities from behavioral flags alone (no propagation).
pub(crate) fn rvs_infer_signature_caps(behavior: &FnBehavior) -> CapabilitySet {
    CapabilityPolicy::rvs_signature_caps(behavior.facts)
}

/// Format an error message for unknown callees.
pub(crate) fn rvs_format_unknown_callees(
    unknown: &BTreeMap<String, BTreeSet<String>>,
    header: &str,
) -> String {
    let mut msg = String::from(header);
    for (callee, callers) in unknown {
        msg.push_str(&format!("  {callee}=\n"));
        for caller in callers.iter().take(3) {
            msg.push_str(&format!("    called by: {caller}\n"));
        }
        if callers.len() > 3 {
            msg.push_str(&format!("    ... and {} more\n", callers.len() - 3));
        }
    }
    msg
}

/// Generate trait-method aliases (e.g. `std::io::Read::read`) from impl-method keys.
pub(crate) fn rvs_generate_trait_aliases_MP(
    inferred: &BTreeMap<String, CapabilitySet>,
    impl_index: &HashMap<String, Vec<String>>,
    callgraph: &BTreeMap<String, FnBehavior>,
) -> BTreeMap<String, CapabilitySet> {
    let mut aliases = BTreeMap::new();
    let mut seen = HashSet::new();
    for key in inferred.keys() {
        if let Some(at_pos) = key.find('@') {
            let (method_full, trait_path_with_sep) = key.split_at(at_pos);
            let Some(trait_path) = trait_path_with_sep.strip_prefix('@') else {
                continue;
            };
            if let Some(method_name) = method_full.rsplit("::").next() {
                let alias = format!("{trait_path}::{method_name}");
                if seen.insert(alias.clone())
                    && let Some(voted) =
                        rvs_resolve_impl_union_M(&alias, impl_index, inferred, callgraph)
                {
                    aliases.insert(alias, voted);
                }
            }
        }
    }
    aliases
}

/// Convert a `CapabilitySet` to its uppercase letter string.
pub(crate) fn rvs_caps_to_string(caps: &CapabilitySet) -> String {
    caps.rvs_iter().map(|c| c.rvs_as_char()).collect()
}

pub(crate) fn rvs_infer_caps_M(
    callgraph: &BTreeMap<String, FnBehavior>,
    seed: &capsmap::CapsMap,
) -> BTreeMap<String, CapabilitySet> {
    let mut inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();

    for (func, behavior) in callgraph {
        if let Some(caps) = seed.rvs_lookup(func) {
            inferred.insert(func.clone(), caps.clone());
        } else {
            inferred.insert(func.clone(), rvs_infer_signature_caps(behavior));
        }
    }
    for behavior in callgraph.values() {
        for callee in &behavior.calls {
            if !inferred.contains_key(callee)
                && let Some(caps) = seed.rvs_lookup(callee)
            {
                inferred.insert(callee.clone(), caps.clone());
            }
        }
    }

    let impl_index = rvs_build_impl_index(callgraph);

    let max_iterations = 16;
    for _iteration in 0..max_iterations {
        let mut changed = false;
        for (func, behavior) in callgraph {
            if seed.rvs_lookup(func).is_some() {
                continue;
            }
            if behavior.facts.is_port_method {
                continue;
            }
            let mut combined = inferred
                .get(func)
                .cloned()
                .unwrap_or_else(CapabilitySet::rvs_new);
            for callee in &behavior.calls {
                let callee_caps = inferred
                    .get(callee)
                    .or_else(|| seed.rvs_lookup(callee))
                    .cloned();

                let callee_caps = callee_caps.or_else(|| {
                    if !callee.contains('@') {
                        rvs_resolve_impl_union_M(callee, &impl_index, &inferred, callgraph)
                    } else {
                        None
                    }
                });
                if let Some(cc) = callee_caps {
                    for cap in cc.rvs_iter() {
                        if !CapabilityPolicy::rvs_is_propagated_cap(cap) {
                            continue;
                        }
                        if !combined.rvs_contains(cap) {
                            combined.rvs_insert_M(cap);
                            changed = true;
                        }
                    }
                }
            }
            inferred.insert(func.clone(), combined);
        }
        if !changed {
            break;
        }
    }
    inferred
}

/// Resolve a trait method callee by taking the union of all impl methods.
pub(crate) fn rvs_resolve_impl_union_M(
    callee: &str,
    impl_index: &HashMap<String, Vec<String>>,
    inferred: &BTreeMap<String, CapabilitySet>,
    callgraph: &BTreeMap<String, FnBehavior>,
) -> Option<CapabilitySet> {
    let (trait_path, method) = callee.rsplit_once("::")?;
    let lookup_key = format!("{method}@{trait_path}");
    let impl_keys = impl_index.get(&lookup_key)?;

    for key in impl_keys {
        if let Some(behavior) = callgraph.get(key)
            && behavior.facts.is_port_method
        {
            let mut caps = CapabilitySet::rvs_new();
            caps.rvs_insert_M(Capability::P);
            return Some(caps);
        }
    }

    let mut cap_counts: HashMap<Capability, usize> = HashMap::new();
    let mut total = 0usize;
    for key in impl_keys {
        if let Some(caps) = inferred.get(key) {
            total += 1;
            for cap in caps.rvs_iter() {
                if CapabilityPolicy::rvs_is_propagated_cap(cap) {
                    *cap_counts.entry(cap).or_default() += 1;
                }
            }
        }
    }

    if total == 0 {
        return None;
    }

    let threshold = total.div_ceil(2);
    let mut union = CapabilitySet::rvs_new();
    for (cap, count) in &cap_counts {
        if *count >= threshold {
            union.rvs_insert_M(*cap);
        }
    }
    Some(union)
}

pub(crate) fn rvs_format_capsmap(caps: &BTreeMap<String, CapabilitySet>) -> String {
    let mut lines: Vec<String> = caps
        .iter()
        .map(|(name, cs)| {
            let caps_str = rvs_caps_to_string(cs);
            if caps_str.is_empty() {
                format!("{name}=")
            } else {
                let desc: String = cs
                    .rvs_iter()
                    .map(|c| c.rvs_description())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{name}={caps_str} # {desc}")
            }
        })
        .collect();
    lines.sort();
    lines.join("\n") + "\n"
}

pub(crate) fn rvs_collect_direct_external_deps(
    callgraph: &BTreeMap<String, FnBehavior>,
    local_crate_prefixes: &BTreeSet<String>,
    seed: &capsmap::CapsMap,
    inferred: &BTreeMap<String, CapabilitySet>,
    impl_index: &HashMap<String, Vec<String>>,
) -> (
    BTreeMap<String, CapabilitySet>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let local_prefixes: Vec<String> = local_crate_prefixes
        .iter()
        .map(|name| format!("{name}::"))
        .collect();
    let mut known: BTreeMap<String, CapabilitySet> = BTreeMap::new();
    let mut unknown: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (func, behavior) in callgraph {
        if !local_prefixes.iter().any(|prefix| func.starts_with(prefix)) {
            continue;
        }
        for callee in &behavior.calls {
            if local_prefixes
                .iter()
                .any(|prefix| callee.starts_with(prefix))
            {
                continue;
            }
            if seed.rvs_lookup(callee).is_some() {
                continue;
            }
            if let Some(caps) = inferred.get(callee) {
                known.entry(callee.clone()).or_insert_with(|| caps.clone());
            } else if let Some(caps) =
                rvs_resolve_impl_union_M(callee, impl_index, inferred, callgraph)
            {
                known.entry(callee.clone()).or_insert(caps);
            } else {
                unknown
                    .entry(callee.clone())
                    .or_default()
                    .insert(func.clone());
            }
        }
    }
    (known, unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilityFacts;

    fn rvs_snapshot_BIS(name: &str, content: &str) {
        std::fs::create_dir_all("test_out").unwrap();
        std::fs::write(format!("test_out/{name}.out"), content).unwrap();
    }

    /// Helper: build a default `FnBehavior` with all flags false and no calls.
    fn rvs_make_behavior() -> FnBehavior {
        FnBehavior {
            calls: BTreeSet::new(),
            facts: CapabilityFacts::default(),
            is_trait_impl: false,
            is_test: false,
        }
    }

    // ─── rvs_infer_caps_M ────────────────────────────────────────────────

    #[test]
    fn test_20260609_infer_caps_empty_callgraph() {
        let callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        rvs_snapshot_BIS(
            "test_20260609_infer_caps_empty_callgraph",
            &format!("{result:?}"),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_20260613_seed_freeze_prevents_propagation() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();

        let mut cap_overflow = rvs_make_behavior();
        cap_overflow.calls.insert("core::panicking::panic".into());
        callgraph.insert("alloc::raw_vec::capacity_overflow".into(), cap_overflow);

        let panic = rvs_make_behavior();
        callgraph.insert("core::panicking::panic".into(), panic);

        let mut handle_error = rvs_make_behavior();
        handle_error
            .calls
            .insert("alloc::raw_vec::capacity_overflow".into());
        callgraph.insert("alloc::raw_vec::handle_error".into(), handle_error);

        let seed = capsmap::CapsMap::rvs_parse(
            "alloc::raw_vec::capacity_overflow=\nalloc::raw_vec::handle_error=\n",
        )
        .unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);

        let cap_caps = result.get("alloc::raw_vec::capacity_overflow");
        assert!(
            cap_caps.is_none_or(|c| c.rvs_is_empty()),
            "capacity_overflow should be frozen to empty by seed, got: {cap_caps:?}"
        );

        let handle_caps = result.get("alloc::raw_vec::handle_error");
        assert!(
            handle_caps.is_none_or(|c| c.rvs_is_empty()),
            "handle_error should be frozen to empty by seed, got: {handle_caps:?}"
        );
    }

    #[test]
    fn test_20260609_infer_caps_single_pure() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        callgraph.insert("my_crate::rvs_add".into(), rvs_make_behavior());
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_single_pure", &output);
        assert!(
            result
                .get("my_crate::rvs_add")
                .is_none_or(|c| c.rvs_is_empty())
        );
    }

    #[test]
    fn test_20260609_infer_caps_single_panic() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let behavior = rvs_make_behavior();
        callgraph.insert("my_crate::rvs_divide".into(), behavior);
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_single_panic", &output);
        let caps = result
            .get("my_crate::rvs_divide")
            .expect("should have entry");
        assert!(caps.rvs_is_empty());
        assert_eq!(caps.rvs_len(), 0);
    }

    #[test]
    fn test_20260609_infer_caps_single_static_ref() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.facts.has_static_ref = true;
        callgraph.insert("my_crate::rvs_get_env_S".into(), behavior);
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_single_static_ref", &output);
        let caps = result
            .get("my_crate::rvs_get_env_S")
            .expect("should have entry");
        assert!(caps.rvs_contains(Capability::S));
        assert_eq!(caps.rvs_len(), 1);
    }

    #[test]
    fn test_20260609_infer_caps_single_unsafe_block() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let behavior = rvs_make_behavior();
        callgraph.insert("my_crate::rvs_ffi_call".into(), behavior);
        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let caps = result.get("my_crate::rvs_ffi_call");
        assert!(caps.is_some());
        assert!(caps.unwrap().rvs_is_empty());
    }

    #[test]
    fn test_20260609_infer_caps_propagation_caller_gets_io() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut caller_behavior = rvs_make_behavior();
        caller_behavior
            .calls
            .insert("std::fs::read_to_string".into());
        callgraph.insert("my_crate::rvs_process".into(), caller_behavior);
        callgraph.insert("std::fs::read_to_string".into(), rvs_make_behavior());

        let seed = capsmap::CapsMap::rvs_parse("std::fs::read_to_string=BI").unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS(
            "test_20260609_infer_caps_propagation_caller_gets_io",
            &output,
        );

        let caller_caps = result
            .get("my_crate::rvs_process")
            .expect("caller should have entry");
        assert!(caller_caps.rvs_contains(Capability::B));
        assert!(caller_caps.rvs_contains(Capability::I));
        assert_eq!(caller_caps.rvs_len(), 2);
    }

    #[test]
    fn test_20260609_infer_caps_propagation_chain() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut a_behavior = rvs_make_behavior();
        a_behavior.calls.insert("my_crate::B".into());
        callgraph.insert("my_crate::A".into(), a_behavior);

        let mut b_behavior = rvs_make_behavior();
        b_behavior.calls.insert("my_crate::C".into());
        callgraph.insert("my_crate::B".into(), b_behavior);

        callgraph.insert("my_crate::C".into(), rvs_make_behavior());

        let seed = capsmap::CapsMap::rvs_parse("my_crate::C=S").unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_propagation_chain", &output);

        let a_caps = result.get("my_crate::A").expect("A should have entry");
        let b_caps = result.get("my_crate::B").expect("B should have entry");
        assert!(a_caps.rvs_contains(Capability::S));
        assert!(b_caps.rvs_contains(Capability::S));
    }

    #[test]
    fn test_20260609_infer_caps_cycle_self_recursive() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert("my_crate::rvs_loop".into());
        callgraph.insert("my_crate::rvs_loop".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_cycle_self_recursive", &output);

        assert!(
            result
                .get("my_crate::rvs_loop")
                .is_none_or(|c| c.rvs_is_empty())
        );
    }

    #[test]
    fn test_20260609_infer_caps_cycle_mutual_recursion() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut a_behavior = rvs_make_behavior();
        a_behavior.calls.insert("my_crate::B".into());
        callgraph.insert("my_crate::A".into(), a_behavior);

        let mut b_behavior = rvs_make_behavior();
        b_behavior.calls.insert("my_crate::A".into());
        callgraph.insert("my_crate::B".into(), b_behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_cycle_mutual_recursion", &output);

        assert!(result.get("my_crate::A").is_none_or(|c| c.rvs_is_empty()));
        assert!(result.get("my_crate::B").is_none_or(|c| c.rvs_is_empty()));
    }

    #[test]
    fn test_20260609_infer_caps_seed_override() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let behavior = rvs_make_behavior();
        callgraph.insert("my_crate::rvs_read_BI".into(), behavior);

        let seed = capsmap::CapsMap::rvs_parse("my_crate::rvs_read_BI=BI").unwrap();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_seed_override", &output);

        let caps = result
            .get("my_crate::rvs_read_BI")
            .expect("should have entry");
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert!(
            !caps.rvs_contains(Capability::T),
            "seed should override behavioral flags"
        );
        assert_eq!(caps.rvs_len(), 2);
    }

    #[test]
    fn test_20260609_infer_caps_rvs_suffix_from_name() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.facts.has_async = true;
        behavior.facts.has_mut_param = true;
        callgraph.insert("my_crate::rvs_write_db_ABM".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS("test_20260609_infer_caps_rvs_suffix_from_name", &output);

        let caps = result
            .get("my_crate::rvs_write_db_ABM")
            .expect("should have entry");
        assert!(caps.rvs_contains(Capability::A));
        assert!(caps.rvs_contains(Capability::M));
        assert_eq!(caps.rvs_len(), 2);
    }

    #[test]
    fn test_20260613_infer_caps_propagation_from_bimps_callee() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();

        let mut caller_behavior = rvs_make_behavior();
        caller_behavior.facts.has_mut_param = true;
        caller_behavior
            .calls
            .insert("std::sys::process::unix::unix::impl::spawn".into());
        callgraph.insert("std::process::impl::spawn".into(), caller_behavior);

        let mut callee_behavior = rvs_make_behavior();
        callee_behavior.facts.has_mut_param = true;
        callee_behavior
            .calls
            .insert("std::sys::pal::unix::kernel_copy::rvs_write".into());
        callee_behavior.calls.insert("std::sys::cycle_a".into());
        callgraph.insert(
            "std::sys::process::unix::unix::impl::spawn".into(),
            callee_behavior,
        );

        let mut cycle_a = rvs_make_behavior();
        cycle_a.calls.insert("std::sys::cycle_b".into());
        callgraph.insert("std::sys::cycle_a".into(), cycle_a);

        let mut cycle_b = rvs_make_behavior();
        cycle_b.calls.insert("std::sys::cycle_a".into());
        callgraph.insert("std::sys::cycle_b".into(), cycle_b);

        let seed =
            capsmap::CapsMap::rvs_parse("std::sys::pal::unix::kernel_copy::rvs_write=BIS").unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);
        let output = rvs_format_capsmap(&result);
        rvs_snapshot_BIS(
            "test_20260613_infer_caps_propagation_from_bimps_callee",
            &output,
        );

        let callee_caps = result
            .get("std::sys::process::unix::unix::impl::spawn")
            .expect("callee should have entry");
        assert!(
            callee_caps.rvs_contains(Capability::B),
            "callee should have B from deep callee"
        );
        assert!(
            callee_caps.rvs_contains(Capability::I),
            "callee should have I from deep callee"
        );
        assert!(
            callee_caps.rvs_contains(Capability::M),
            "callee should have M from has_mut_param"
        );
        assert!(
            callee_caps.rvs_contains(Capability::S),
            "callee should have S from deep callee"
        );

        let caller_caps = result
            .get("std::process::impl::spawn")
            .expect("caller should have entry");
        assert!(
            caller_caps.rvs_contains(Capability::B),
            "caller should have B propagated from callee"
        );
        assert!(
            caller_caps.rvs_contains(Capability::I),
            "caller should have I propagated from callee"
        );
        assert!(
            caller_caps.rvs_contains(Capability::M),
            "caller should have M from has_mut_param"
        );
        assert!(
            caller_caps.rvs_contains(Capability::S),
            "caller should have S propagated from callee"
        );
    }

    // ─── rvs_resolve_impl_union_M ────────────────────────────────────────

    #[test]
    fn test_20260613_impl_union_majority_vote() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();

        let mut caller = rvs_make_behavior();
        caller.calls.insert("std::io::Read::read".into());
        callgraph.insert("my_crate::rvs_copy".into(), caller);

        let mut file_read = rvs_make_behavior();
        file_read.facts.has_mut_param = true;
        file_read.calls.insert("libc::unix::read".into());
        callgraph.insert("std::fs::read@std::io::Read".into(), file_read);

        let mut cursor_read = rvs_make_behavior();
        cursor_read.facts.has_mut_param = true;
        callgraph.insert("std::io::cursor::read@std::io::Read".into(), cursor_read);

        let mut slice_read = rvs_make_behavior();
        slice_read.facts.has_mut_param = true;
        callgraph.insert("std::io::impls::read@std::io::Read".into(), slice_read);

        let seed = capsmap::CapsMap::rvs_parse("libc::unix::read=BI").unwrap();

        let result = rvs_infer_caps_M(&callgraph, &seed);

        let caller_caps = result.get("my_crate::rvs_copy").expect("caller exists");
        assert!(
            !caller_caps.rvs_contains(Capability::M),
            "M: not propagated"
        );
        assert!(
            !caller_caps.rvs_contains(Capability::B),
            "B: 1/3 = minority, should not propagate"
        );
        assert!(
            !caller_caps.rvs_contains(Capability::I),
            "I: 1/3 = minority, should not propagate"
        );
    }

    #[test]
    fn test_20260614_m_not_propagated_from_direct_call() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();

        let mut caller = rvs_make_behavior();
        caller.facts.has_async = true;
        caller.calls.insert("my_crate::sort_inplace".into());
        callgraph.insert("my_crate::handle".into(), caller);

        let mut callee = rvs_make_behavior();
        callee.facts.has_mut_param = true;
        callgraph.insert("my_crate::sort_inplace".into(), callee);

        let seed = capsmap::CapsMap::rvs_new();
        let result = rvs_infer_caps_M(&callgraph, &seed);

        let caller_caps = result.get("my_crate::handle").expect("caller exists");
        assert!(
            !caller_caps.rvs_contains(Capability::M),
            "M should NOT propagate — signature-only capability"
        );
        assert!(caller_caps.rvs_contains(Capability::A), "A from has_async");
    }

    #[test]
    fn test_20260613_impl_union_no_cross_trait() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();

        let mut caller = rvs_make_behavior();
        caller.calls.insert("std::io::Read::read".into());
        callgraph.insert("my_crate::rvs_read_data".into(), caller);

        let mut file_read = rvs_make_behavior();
        file_read.calls.insert("libc::unix::read".into());
        callgraph.insert("std::fs::read@std::io::Read".into(), file_read);

        let mut rwlock_read = rvs_make_behavior();
        rwlock_read.facts.has_mut_param = true;
        callgraph.insert(
            "std::sync::rwlock::read@std::sync::RwLock".into(),
            rwlock_read,
        );

        let seed = capsmap::CapsMap::rvs_parse("libc::unix::read=BI").unwrap();
        let result = rvs_infer_caps_M(&callgraph, &seed);

        let caller_caps = result
            .get("my_crate::rvs_read_data")
            .expect("caller exists");
        assert!(
            caller_caps.rvs_contains(Capability::B),
            "should get B from Read::read impl"
        );
        assert!(
            !caller_caps.rvs_contains(Capability::M),
            "should NOT get M from RwLock::read (different trait)"
        );
    }

    // ─── rvs_format_capsmap ────────────────────────────────────────────

    #[test]
    fn test_20260609_format_capsmap_empty() {
        let map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        let output = rvs_format_capsmap(&map);
        rvs_snapshot_BIS("test_20260609_format_capsmap_empty", &output);
        assert_eq!(output, "\n");
    }

    #[test]
    fn test_20260609_format_capsmap_single_entry() {
        let mut map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        map.insert(
            "std::fs::read".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );
        let output = rvs_format_capsmap(&map);
        rvs_snapshot_BIS("test_20260609_format_capsmap_single_entry", &output);
        assert_eq!(output, "std::fs::read=BI # Blocking IO\n");
    }

    #[test]
    fn test_20260609_format_capsmap_multiple_sorted() {
        let mut map: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        map.insert(
            "std::process::exit".into(),
            CapabilitySet::rvs_from_validated("S"),
        );
        map.insert("HashMap::new".into(), CapabilitySet::rvs_new());
        map.insert(
            "std::fs::read".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );
        let output = rvs_format_capsmap(&map);
        rvs_snapshot_BIS("test_20260609_format_capsmap_multiple_sorted", &output);
        let lines: Vec<&str> = output.trim_end().lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("HashMap::new"));
        assert!(lines[1].starts_with("std::fs::read"));
        assert!(lines[2].starts_with("std::process::exit"));
    }

    // ─── rvs_collect_direct_external_deps ────────────────────────────────

    #[test]
    fn test_20260630_collect_direct_external_deps_uses_bin_prefix() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut local = rvs_make_behavior();
        local.calls.insert("serde_json::de::from_str".to_string());
        callgraph.insert("cargo_rivus::rvs_parse".to_string(), local);

        let seed = capsmap::CapsMap::rvs_new();
        let mut inferred = BTreeMap::new();
        inferred.insert(
            "serde_json::de::from_str".to_string(),
            CapabilitySet::rvs_new(),
        );
        let prefixes = BTreeSet::from(["rivus_linter".to_string(), "cargo_rivus".to_string()]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &callgraph,
            &prefixes,
            &seed,
            &inferred,
            &HashMap::new(),
        );

        rvs_snapshot_BIS(
            "test_20260630_collect_direct_external_deps_uses_bin_prefix",
            &format!("known={known:?}\nunknown={unknown:?}"),
        );
        assert!(known.contains_key("serde_json::de::from_str"));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260611_unknown_callee_reported_as_error() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior
            .calls
            .insert("some_external_crate::unknown_fn".into());
        callgraph.insert("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        let local_prefixes = BTreeSet::from(["my_crate".to_string()]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &callgraph,
            &local_prefixes,
            &seed,
            &inferred,
            &HashMap::new(),
        );

        assert!(known.is_empty());
        assert!(
            unknown.contains_key("some_external_crate::unknown_fn"),
            "unknown callee must be reported as error"
        );
        assert_eq!(unknown.len(), 1);
        assert!(unknown["some_external_crate::unknown_fn"].contains("my_crate::caller"));
    }

    #[test]
    fn test_20260611_inferred_callee_is_known() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior
            .calls
            .insert("some_external_crate::known_fn".into());
        callgraph.insert("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_new();
        let mut inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        inferred.insert(
            "some_external_crate::known_fn".into(),
            CapabilitySet::rvs_from_validated("BI"),
        );
        let local_prefixes = BTreeSet::from(["my_crate".to_string()]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &callgraph,
            &local_prefixes,
            &seed,
            &inferred,
            &HashMap::new(),
        );

        let caps = known
            .get("some_external_crate::known_fn")
            .expect("should have entry in known");
        assert!(caps.rvs_contains(Capability::B));
        assert!(caps.rvs_contains(Capability::I));
        assert!(unknown.is_empty());
    }

    #[test]
    fn test_20260611_seed_callee_is_skipped() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert("std::fs::write".into());
        callgraph.insert("my_crate::caller".into(), behavior);

        let seed = capsmap::CapsMap::rvs_parse("std::fs::write=BI").unwrap();
        let inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        let local_prefixes = BTreeSet::from(["my_crate".to_string()]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &callgraph,
            &local_prefixes,
            &seed,
            &inferred,
            &HashMap::new(),
        );

        assert!(!known.contains_key("std::fs::write"));
        assert!(!unknown.contains_key("std::fs::write"));
    }

    #[test]
    fn test_20260613_inherent_impl_no_collision() {
        let mut callgraph: BTreeMap<String, FnBehavior> = BTreeMap::new();
        let mut behavior = rvs_make_behavior();
        behavior.calls.insert("std::time::SystemTime::now".into());
        callgraph.insert("my_crate::rvs_get_time".into(), behavior);

        let seed = capsmap::CapsMap::rvs_parse("std::time::SystemTime::now=S").unwrap();

        let inferred: BTreeMap<String, CapabilitySet> = BTreeMap::new();
        let local_prefixes = BTreeSet::from(["my_crate".to_string()]);

        let (known, unknown) = rvs_collect_direct_external_deps(
            &callgraph,
            &local_prefixes,
            &seed,
            &inferred,
            &HashMap::new(),
        );

        assert!(
            !unknown.contains_key("std::time::SystemTime::now"),
            "seed entry should match the full def_path"
        );
        assert!(!known.contains_key("std::time::SystemTime::now"));
    }

    // ─── coverage ────────────────────────────────────────────────────────

    #[test]
    fn test_20260630_main_helper_coverage() {
        let mut merged = rvs_make_behavior();
        let mut other = rvs_make_behavior();
        other.calls.insert("std::io::Read::read".into());
        other.facts.has_async = true;
        merged.rvs_merge_M(&other);
        assert!(merged.calls.contains("std::io::Read::read"));
        assert!(merged.facts.has_async);

        let mut callgraph = BTreeMap::new();
        let mut impl_behavior = rvs_make_behavior();
        impl_behavior.facts.has_mut_param = true;
        let inferred_caps = rvs_infer_signature_caps(&impl_behavior);
        callgraph.insert("std::fs::read@std::io::Read".into(), impl_behavior);

        let impl_index = rvs_build_impl_index(&callgraph);
        assert!(impl_index.contains_key("read@std::io::Read"));

        assert!(inferred_caps.rvs_contains(Capability::M));

        let mut unknown = BTreeMap::new();
        unknown.insert(
            "missing::fn".to_string(),
            BTreeSet::from(["caller::fn".to_string()]),
        );
        let formatted = rvs_format_unknown_callees(&unknown, "header\n");
        assert!(formatted.contains("missing::fn"));

        let caps_str = rvs_caps_to_string(&CapabilitySet::rvs_from_validated("BI"));
        assert_eq!(caps_str, "BI");

        let inferred = BTreeMap::from([(
            "std::fs::read@std::io::Read".to_string(),
            CapabilitySet::rvs_from_validated("BI"),
        )]);
        let aliases = rvs_generate_trait_aliases_MP(&inferred, &impl_index, &callgraph);
        assert_eq!(
            aliases.get("std::io::Read::read"),
            Some(&CapabilitySet::rvs_from_validated("BI"))
        );

        let union =
            rvs_resolve_impl_union_M("std::io::Read::read", &impl_index, &inferred, &callgraph);
        assert_eq!(union, Some(CapabilitySet::rvs_from_validated("BI")));
    }
}
