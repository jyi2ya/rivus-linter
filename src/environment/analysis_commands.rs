use std::collections::BTreeSet;
use std::path::Path;

use super::rename;
use crate::callgraph::rvs_is_std_like_def_path;
use crate::inference::{FnContractDiff, PreparedLocalAnalysis};
use crate::symbols::DefPath;

use super::cargo_targets::{
    CargoTargetScope, rvs_detect_local_crate_prefixes_BIS,
    rvs_detect_local_crate_prefixes_for_function_query_BIS,
};
use super::workspace::{
    rvs_canonical_cargo_project_BIS, rvs_collect_callgraph_and_caps_BIST,
    rvs_load_callgraph_and_caps_for_function_BIST, rvs_load_local_crate_prefixes_BIS,
};
use crate::function_classification::LocalScope;

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_annotate_BIPST(path: &Path) -> Result<(), String> {
    let project_path = rvs_canonical_cargo_project_BIS(path)?;
    let target_scope = CargoTargetScope::Production;
    let local_crate_names = rvs_detect_local_crate_prefixes_BIS(&project_path, target_scope)?;
    let (mut callgraph, seed) =
        rvs_collect_callgraph_and_caps_BIST(&project_path, target_scope, &local_crate_names)?;
    let diffs =
        PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &seed, &local_crate_names).diffs;
    let mut candidates = Vec::new();
    for diff in diffs {
        if !diff.rvs_has_name_mismatch() {
            continue;
        }
        candidates.push(rename::SourceRenameCandidate::rvs_new(
            diff.def_path,
            diff.expected_name,
        ));
    }
    let plan = rename::rvs_build_source_rename_plan_BIST(
        &callgraph,
        &project_path,
        candidates,
        "annotate",
    )?;
    rename::rvs_execute_source_rename_plan_BIST(&project_path, &plan, "annotate", "Annotate")
}

/// # Panics
///
/// Panics if the current executable path, current directory, or cargo cannot be resolved.
pub(crate) fn rvs_run_why_BIPST(function: &str, path: &Path) -> Result<(), String> {
    let project_path = rvs_canonical_cargo_project_BIS(path)?;
    let target_scope = CargoTargetScope::WithTestExampleBench;

    let local_crate_names = if rvs_is_std_like_def_path(function) {
        match rvs_detect_local_crate_prefixes_for_function_query_BIS(&project_path, target_scope)? {
            Some(names) if LocalScope::rvs_new(&names).rvs_contains_str(function) => names,
            Some(_) | None => std::collections::BTreeSet::new(),
        }
    } else {
        rvs_load_local_crate_prefixes_BIS(&project_path, target_scope)?
    };
    let (mut callgraph, seed) = rvs_load_callgraph_and_caps_for_function_BIST(
        &project_path,
        function,
        target_scope,
        &local_crate_names,
    )?;
    let analysis = PreparedLocalAnalysis::rvs_prepare_M(&mut callgraph, &seed, &local_crate_names);
    let resolver = analysis.rvs_resolver(&callgraph, &seed);

    let exact_or_readable_matches =
        rvs_why_function_matches(&callgraph, analysis.rvs_synthetic_paths(), function);
    let function_path = match exact_or_readable_matches.as_slice() {
        [function_path] => function_path.clone(),
        [] => {
            let candidates: Vec<DefPath> = callgraph
                .rvs_keys()
                .chain(analysis.rvs_synthetic_paths().iter())
                .filter(|k| k.rvs_contains(function))
                .take(10)
                .cloned()
                .collect();
            if candidates.is_empty() {
                return Err(format!("function '{function}' not found in callgraph"));
            }
            eprintln!("Exact match not found. Did you mean:");
            for c in &candidates {
                let caps_str = rvs_format_why_path_caps(c, &analysis, &resolver);
                eprintln!("  {c}{caps_str}");
            }
            return Err(format!(
                "function '{function}' not found; see suggestions above"
            ));
        }
        matches => {
            return Err(format!(
                "function '{function}' is ambiguous: {} specialized implementations share this readable path",
                matches.len()
            ));
        }
    };
    let function_key = function_path.rvs_as_str();
    let behavior = callgraph.rvs_get(function_key);
    let is_synthetic = analysis.rvs_synthetic_paths().contains(&function_path);

    let knowledge_completeness = rvs_why_path_completeness(&function_path, &analysis, &resolver);
    let caps_str = rvs_format_why_path_caps(&function_path, &analysis, &resolver);
    println!("{function_path}{caps_str}");
    for line in rvs_format_enforced_contract_diff_summary(
        &analysis.diffs,
        function_key,
        knowledge_completeness,
    ) {
        println!("  {line}");
    }
    for line in rvs_format_capsmap_knowledge(&seed, &function_path) {
        println!("  {line}");
    }
    for line in rvs_format_trait_vote_summary(&analysis, &function_path) {
        println!("  {line}");
    }
    println!();

    if is_synthetic {
        println!("  {}", rvs_callee_absence_message(false, true));
        return Ok(());
    }

    let behavior = behavior.expect("never: non-synthetic function was found in callgraph");

    if !behavior.has_body {
        println!("  {}", rvs_callee_absence_message(false, false));
        return Ok(());
    }

    if behavior.calls.is_empty() {
        println!("  {}", rvs_callee_absence_message(true, false));
        return Ok(());
    }

    println!("  callees:");
    for line in rvs_format_why_callees(behavior, &analysis, &resolver) {
        println!("{line}");
    }

    Ok(())
}

fn rvs_format_capsmap_knowledge(caps: &crate::capsmap::CapsMap, function: &DefPath) -> Vec<String> {
    let Some(info) = caps.rvs_lookup_info_def_path(function) else {
        return Vec::new();
    };
    let mut lines = vec![format!(
        "caps knowledge: basis={}, completeness={}",
        info.rvs_basis().rvs_name(),
        info.rvs_completeness().rvs_name()
    )];
    if let crate::capability::CapabilityBasis::TraitVote {
        implementations,
        threshold,
        votes,
    } = info.rvs_basis()
    {
        let counts = crate::capability::CapabilityPolicy::rvs_propagated_caps()
            .into_iter()
            .map(|capability| {
                format!(
                    "{}={}/{}",
                    capability.rvs_as_char(),
                    votes.get(&capability).copied().unwrap_or(0),
                    implementations
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let selected = if info.rvs_caps().rvs_is_empty() {
            if info.rvs_completeness() == crate::capability::CapabilityCompleteness::Complete {
                "none".to_string()
            } else {
                "(no known propagated caps)".to_string()
            }
        } else {
            rvs_format_caps_letters(info.rvs_caps())
        };
        lines.push(format!(
            "persisted trait vote: selected={}, threshold={threshold}/{implementations}, votes: {counts}",
            selected
        ));
    }
    if let Some(source) = info.rvs_source() {
        lines.push(format!(
            "caps source: {}:{} ({})",
            source.file.display(),
            source.line,
            source.layer
        ));
    }
    lines
}

fn rvs_format_trait_vote_summary(
    analysis: &PreparedLocalAnalysis,
    function: &DefPath,
) -> Vec<String> {
    let trait_method = function
        .rvs_trait_method_identity()
        .map(|identity| identity.rvs_trait_method_path())
        .unwrap_or_else(|| function.clone());
    let Some(vote) = analysis.rvs_trait_votes().get(&trait_method) else {
        return Vec::new();
    };
    let vote_complete = vote.rvs_is_complete();
    let selected = if vote.selected_caps.rvs_is_empty() {
        if vote_complete {
            "none".to_string()
        } else {
            "(no known propagated caps)".to_string()
        }
    } else {
        rvs_format_caps_letters(&vote.selected_caps)
    };
    let counts = crate::capability::CapabilityPolicy::rvs_propagated_caps()
        .into_iter()
        .map(|capability| {
            format!(
                "{}={}/{}",
                capability.rvs_as_char(),
                vote.counts.get(&capability).copied().unwrap_or(0),
                vote.implementations.len()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut lines = vec![format!(
        "trait vote: selected={selected}, threshold={}/{}, completeness={}",
        vote.threshold,
        vote.implementations.len(),
        if vote_complete {
            "complete"
        } else {
            "incomplete"
        }
    )];
    lines.push(format!("trait votes: {counts}"));
    if *function == trait_method {
        let incomplete_implementations: Vec<&crate::inference::TraitVoteImplementation> = vote
            .implementations
            .iter()
            .filter(|implementation| implementation.incomplete)
            .collect();
        lines.extend(
            incomplete_implementations
                .iter()
                .take(5)
                .map(|implementation| {
                    format!(
                        "incomplete trait implementation: {} (known propagated caps: {})",
                        implementation.path,
                        rvs_format_known_propagated_caps(&implementation.propagated_caps)
                    )
                }),
        );
        if incomplete_implementations.len() > 5 {
            lines.push(format!(
                "... and {} more incomplete trait implementations",
                incomplete_implementations.len() - 5
            ));
        }
    }
    if let Some(implementation) = vote
        .implementations
        .iter()
        .find(|implementation| implementation.path == *function)
    {
        if implementation.incomplete {
            lines.push(format!(
                "known trait impl contribution (incomplete): {}",
                rvs_format_known_propagated_caps(&implementation.propagated_caps)
            ));
        } else {
            let contribution = rvs_format_known_propagated_caps(&implementation.propagated_caps);
            lines.push(format!("trait impl contribution: {contribution}"));
        }
        if let Some(outlier) = analysis
            .trait_impl_outliers
            .iter()
            .find(|outlier| outlier.implementation == *function)
        {
            lines.push(format!(
                "trait impl outlier caps: {}",
                rvs_format_caps_letters(&outlier.unexpected_caps)
            ));
        }
    }
    lines
}

fn rvs_format_why_callees(
    behavior: &crate::artifacts::FnNode,
    analysis: &PreparedLocalAnalysis,
    resolver: &crate::inference::CalleeCapsResolver<'_>,
) -> Vec<String> {
    behavior
        .calls
        .keys()
        .map(|callee| {
            let caps = resolver.rvs_for_explanation_view(&callee.def_path);
            let completeness = rvs_why_path_completeness(&callee.def_path, analysis, resolver);
            format!(
                "    {}: {}",
                callee.def_path,
                rvs_format_why_callee_caps(caps.as_ref(), completeness)
            )
        })
        .collect()
}

fn rvs_format_why_path_caps(
    path: &DefPath,
    analysis: &PreparedLocalAnalysis,
    resolver: &crate::inference::CalleeCapsResolver<'_>,
) -> String {
    let caps = resolver.rvs_for_explanation_view(path);
    rvs_format_why_caps(
        caps.as_ref(),
        rvs_why_path_completeness(path, analysis, resolver),
    )
}

fn rvs_why_path_completeness(
    path: &DefPath,
    analysis: &PreparedLocalAnalysis,
    resolver: &crate::inference::CalleeCapsResolver<'_>,
) -> crate::capability::CapabilityCompleteness {
    if let Some(info) = resolver.rvs_incomplete_exact_caps_info(path) {
        info.rvs_completeness()
    } else if analysis.rvs_incomplete_paths().contains(path) {
        crate::capability::CapabilityCompleteness::Incomplete
    } else {
        crate::capability::CapabilityCompleteness::Complete
    }
}

fn rvs_format_why_callee_caps(
    caps: Option<&crate::capability::CapabilitySet>,
    completeness: crate::capability::CapabilityCompleteness,
) -> String {
    use crate::capability::CapabilityCompleteness::{Complete, Incomplete, Unknown};

    match (caps, completeness) {
        (Some(caps), Complete) if caps.rvs_is_empty() => "(pure)".to_string(),
        (Some(caps), Complete) => {
            format!("{} ({})", caps.rvs_letters(), caps.rvs_descriptions())
        }
        (Some(caps), Incomplete) if caps.rvs_is_empty() => {
            "(no known caps) [incomplete]".to_string()
        }
        (Some(caps), Incomplete) => format!(
            "{} ({}) [incomplete]",
            caps.rvs_letters(),
            caps.rvs_descriptions()
        ),
        (Some(caps), Unknown) if caps.rvs_is_empty() => {
            "(no known caps) [completeness unknown]".to_string()
        }
        (Some(caps), Unknown) => format!(
            "{} ({}) [completeness unknown]",
            caps.rvs_letters(),
            caps.rvs_descriptions()
        ),
        (None, Complete) => "(unknown)".to_string(),
        (None, Incomplete) => "(unknown) [incomplete]".to_string(),
        (None, Unknown) => "(unknown) [completeness unknown]".to_string(),
    }
}

fn rvs_format_known_propagated_caps(caps: &crate::capability::CapabilitySet) -> String {
    let letters = caps.rvs_letters();
    if letters.is_empty() {
        "none".to_string()
    } else {
        letters
    }
}

fn rvs_format_caps_letters(caps: &crate::capability::CapabilitySet) -> String {
    let letters = caps.rvs_letters();
    if letters.is_empty() {
        "(pure)".to_string()
    } else {
        letters
    }
}

fn rvs_why_function_matches(
    callgraph: &crate::artifacts::FnGraph,
    synthetic_paths: &BTreeSet<DefPath>,
    function: &str,
) -> Vec<DefPath> {
    let exact = DefPath::from(function);
    if callgraph.rvs_get(function).is_some() || synthetic_paths.contains(&exact) {
        return vec![exact];
    }
    callgraph
        .rvs_keys()
        .chain(synthetic_paths)
        .filter(|path| path.rvs_user_path() == function)
        .cloned()
        .collect()
}

fn rvs_format_why_caps(
    caps: Option<&crate::capability::CapabilitySet>,
    completeness: crate::capability::CapabilityCompleteness,
) -> String {
    use crate::capability::CapabilityCompleteness::{Complete, Incomplete, Unknown};

    let Some(caps) = caps else {
        return match completeness {
            Complete => " (unknown)".to_string(),
            Incomplete => " (unknown; inference incomplete)".to_string(),
            Unknown => " (unknown; completeness unknown)".to_string(),
        };
    };
    let letters = caps.rvs_letters();
    if letters.is_empty() {
        match completeness {
            Complete => " (pure)".to_string(),
            Incomplete => " (incomplete; no known caps)".to_string(),
            Unknown => " (no known caps; completeness unknown)".to_string(),
        }
    } else {
        let mut description = caps
            .rvs_iter()
            .map(|capability| capability.rvs_description())
            .collect::<Vec<_>>()
            .join(" ");
        match completeness {
            Complete => {}
            Incomplete => description.push_str("; inference incomplete"),
            Unknown => description.push_str("; completeness unknown"),
        }
        format!(" = {letters} ({description})")
    }
}

fn rvs_callee_absence_message(had_collected_body: bool, is_synthetic: bool) -> &'static str {
    if had_collected_body {
        "(no callees)"
    } else if is_synthetic {
        "(function body not collected; callees unknown)"
    } else {
        "(function has no body; callees intentionally absent)"
    }
}

fn rvs_format_contract_diff_summary(
    diff: &FnContractDiff,
    completeness: crate::capability::CapabilityCompleteness,
) -> Vec<String> {
    use crate::capability::CapabilityCompleteness::{Complete, Incomplete, Unknown};

    let mut lines = Vec::new();
    if diff.expected_name != diff.actual_name {
        lines.push(format!("expected name: {}", diff.expected_name));
    }
    lines.push(format!(
        "declared caps: {}",
        rvs_format_optional_caps(diff.declared_public_caps.as_ref())
    ));
    let expected_label = match completeness {
        Complete => "expected caps",
        Incomplete => "known expected caps (incomplete)",
        Unknown => "known expected caps (completeness unknown)",
    };
    let expected_caps = if completeness != Complete && diff.expected_public_caps.rvs_is_empty() {
        "(no known caps)".to_string()
    } else {
        rvs_format_optional_caps(Some(&diff.expected_public_caps))
    };
    lines.push(format!("{expected_label}: {}", expected_caps));
    if let Some(declared) = diff.declared_public_caps.as_ref() {
        let expected = &diff.expected_public_caps;
        let missing: Vec<_> = expected
            .rvs_iter()
            .filter(|cap| !declared.rvs_contains(*cap))
            .map(|cap| cap.rvs_as_char())
            .collect();
        let extra: Vec<_> = declared
            .rvs_iter()
            .filter(|cap| !expected.rvs_contains(*cap))
            .map(|cap| cap.rvs_as_char())
            .collect();
        if !missing.is_empty() {
            lines.push(format!(
                "missing caps: {}",
                missing.iter().copied().collect::<String>()
            ));
        }
        if completeness == Complete && !extra.is_empty() {
            lines.push(format!(
                "extra declared caps: {}",
                extra.iter().copied().collect::<String>()
            ));
        }
    }
    let mismatch_labels: Vec<&str> = diff
        .rvs_mismatch_kinds()
        .into_iter()
        .map(|kind| kind.rvs_as_str())
        .collect();
    if !mismatch_labels.is_empty() {
        lines.push(format!("mismatches: {}", mismatch_labels.join(", ")));
    }
    lines
}

fn rvs_format_optional_caps(caps: Option<&crate::capability::CapabilitySet>) -> String {
    match caps {
        Some(caps) => rvs_format_caps_letters(caps),
        None => "(not declared)".to_string(),
    }
}

fn rvs_format_enforced_contract_diff_summary(
    diffs: &[FnContractDiff],
    function: &str,
    completeness: crate::capability::CapabilityCompleteness,
) -> Vec<String> {
    let Some(diff) = diffs
        .iter()
        .find(|diff| diff.def_path.rvs_as_str() == function)
    else {
        return Vec::new();
    };
    rvs_format_contract_diff_summary(diff, completeness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::{CallEdgeType, FunctionIdentity};
    use std::collections::HashMap;

    use crate::artifacts::{FnGraph, FnNode, FnSource};
    use crate::symbols::{CrateName, FnName};
    use crate::test_support::{
        rvs_caps_v2, rvs_make_capsmap, rvs_make_cargo_project_BIS, rvs_make_temp_dir_BIS,
        rvs_snapshot_BIS,
    };

    #[test]
    fn test_20260703_format_contract_diff_summary() {
        let diff = FnContractDiff {
            def_path: DefPath::from("demo::rvs_fetch_ABI"),
            actual_name: FnName::from("rvs_fetch_ABI"),
            expected_name: FnName::from("rvs_fetch_P"),
            declared_public_caps: Some(crate::capability::CapabilitySet::rvs_from_validated("ABI")),
            expected_public_caps: crate::capability::CapabilitySet::rvs_from_validated("AP"),
        };
        let complete_lines = rvs_format_contract_diff_summary(
            &diff,
            crate::capability::CapabilityCompleteness::Complete,
        );
        let incomplete_lines = rvs_format_contract_diff_summary(
            &diff,
            crate::capability::CapabilityCompleteness::Incomplete,
        );
        let empty_lower_bound_diff = FnContractDiff {
            def_path: DefPath::from("demo::rvs_parse_S"),
            actual_name: FnName::from("rvs_parse_S"),
            expected_name: FnName::from("rvs_parse_S"),
            declared_public_caps: Some(crate::capability::CapabilitySet::rvs_from_validated("S")),
            expected_public_caps: crate::capability::CapabilitySet::rvs_new(),
        };
        let empty_lower_bound_lines = rvs_format_contract_diff_summary(
            &empty_lower_bound_diff,
            crate::capability::CapabilityCompleteness::Incomplete,
        );
        let unknown_completeness_lines = rvs_format_contract_diff_summary(
            &empty_lower_bound_diff,
            crate::capability::CapabilityCompleteness::Unknown,
        );
        let none_caps = rvs_format_optional_caps(None);
        rvs_snapshot_BIS(
            "test_20260703_format_contract_diff_summary",
            &format!(
                "complete:\n{}\nincomplete:\n{}\nempty lower bound:\n{}\nunknown completeness:\n{}\nnone={none_caps}\n",
                complete_lines.join("\n"),
                incomplete_lines.join("\n"),
                empty_lower_bound_lines.join("\n"),
                unknown_completeness_lines.join("\n")
            ),
        );

        assert_eq!(
            complete_lines,
            vec![
                "expected name: rvs_fetch_P".to_string(),
                "declared caps: ABI".to_string(),
                "expected caps: AP".to_string(),
                "missing caps: P".to_string(),
                "extra declared caps: BI".to_string(),
                "mismatches: name_mismatch, missing_port".to_string(),
            ]
        );
        assert_eq!(
            incomplete_lines,
            vec![
                "expected name: rvs_fetch_P".to_string(),
                "declared caps: ABI".to_string(),
                "known expected caps (incomplete): AP".to_string(),
                "missing caps: P".to_string(),
                "mismatches: name_mismatch, missing_port".to_string(),
            ]
        );
        assert_eq!(
            empty_lower_bound_lines,
            vec![
                "declared caps: S".to_string(),
                "known expected caps (incomplete): (no known caps)".to_string(),
            ]
        );
        assert_eq!(
            unknown_completeness_lines,
            vec![
                "declared caps: S".to_string(),
                "known expected caps (completeness unknown): (no known caps)".to_string(),
            ]
        );
    }

    #[test]
    fn test_20260715_why_formats_trait_vote_and_impl_contribution() {
        let node = || FnNode {
            sources: BTreeSet::from([FnSource::rvs_new("src/lib.rs".into(), 1, 2)]),
            ..FnNode::default()
        };
        let mut graph = FnGraph::rvs_new();
        let mut declaration = node();
        declaration.has_body = false;
        graph.rvs_insert_M(DefPath::from("demo::FromString::rvs_parse"), declaration);
        let signature_only_path = DefPath::from("demo::Alpha::rvs_parse@demo::FromString");
        let mut signature_only = node();
        signature_only.is_trait_impl = true;
        signature_only.facts.has_async = true;
        graph.rvs_insert_M(signature_only_path.clone(), signature_only);
        let mut beta = node();
        beta.is_trait_impl = true;
        graph.rvs_insert_M(
            DefPath::from("demo::Beta::rvs_parse@demo::FromString"),
            beta,
        );
        let mut outlier = node();
        outlier.is_trait_impl = true;
        outlier.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("dep::environment"),
            },
            CallEdgeType::Strong,
        );
        let outlier_path = DefPath::from("demo::EnvValue::rvs_parse@demo::FromString");
        graph.rvs_insert_M(outlier_path.clone(), outlier);
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &rvs_make_capsmap(&[("dep::environment", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );

        let trait_lines =
            rvs_format_trait_vote_summary(&analysis, &DefPath::from("demo::FromString::rvs_parse"));
        let impl_lines = rvs_format_trait_vote_summary(&analysis, &outlier_path);
        let signature_only_lines = rvs_format_trait_vote_summary(&analysis, &signature_only_path);
        let output = format!(
            "trait:\n{}\nimpl:\n{}\nsignature-only impl:\n{}\n",
            trait_lines.join("\n"),
            impl_lines.join("\n"),
            signature_only_lines.join("\n"),
        );
        rvs_snapshot_BIS(
            "test_20260715_why_formats_trait_vote_and_impl_contribution",
            &output,
        );

        assert!(output.contains("threshold=2/3"));
        assert!(output.contains("trait impl contribution: S"));
        assert!(output.contains("trait impl contribution: none"));
        assert!(!output.contains("selected=(pure)"));
        assert!(output.contains("trait impl outlier caps: S"));
    }

    #[test]
    fn test_20260721_why_lists_incomplete_trait_vote_implementations() {
        let node = || FnNode {
            sources: BTreeSet::from([FnSource::rvs_new("src/lib.rs".into(), 1, 2)]),
            ..FnNode::default()
        };
        let mut graph = FnGraph::rvs_new();
        let mut declaration = node();
        declaration.has_body = false;
        let trait_path = DefPath::from("demo::Parser::rvs_parse");
        graph.rvs_insert_M(trait_path.clone(), declaration);

        let mut complete_impl = node();
        complete_impl.is_trait_impl = true;
        graph.rvs_insert_M(
            DefPath::from("demo::Complete::rvs_parse@demo::Parser"),
            complete_impl,
        );

        let mut empty_incomplete_impl = node();
        empty_incomplete_impl.is_trait_impl = true;
        empty_incomplete_impl.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("dep::opaque_empty"),
            },
            CallEdgeType::Strong,
        );
        let empty_incomplete_path = DefPath::from("demo::EmptyIncomplete::rvs_parse@demo::Parser");
        graph.rvs_insert_M(empty_incomplete_path.clone(), empty_incomplete_impl);

        let mut stateful_incomplete_impl = node();
        stateful_incomplete_impl.is_trait_impl = true;
        stateful_incomplete_impl.facts.has_static_ref = true;
        stateful_incomplete_impl.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("dep::opaque_stateful"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(
            DefPath::from("demo::StatefulIncomplete::rvs_parse@demo::Parser"),
            stateful_incomplete_impl,
        );

        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let graph_output = rvs_format_trait_vote_summary(&analysis, &trait_path).join("\n");
        let implementation_output =
            rvs_format_trait_vote_summary(&analysis, &empty_incomplete_path).join("\n");

        let mut persisted_caps = crate::capsmap::CapsMap::rvs_new();
        persisted_caps.rvs_insert_info_M(
            crate::symbols::CapsMapKey::from("dep::Parser::rvs_parse"),
            crate::capability::CapabilityInfo::rvs_trait_vote(
                crate::capability::CapabilitySet::rvs_new(),
                3,
                2,
                std::collections::BTreeMap::from([(crate::capability::Capability::S, 1)]),
                crate::capability::CapabilityCompleteness::Incomplete,
            ),
        );
        let persisted_output =
            rvs_format_capsmap_knowledge(&persisted_caps, &DefPath::from("dep::Parser::rvs_parse"))
                .join("\n");
        let output = format!(
            "graph:\n{graph_output}\nimplementation:\n{implementation_output}\npersisted:\n{persisted_output}\n"
        );
        rvs_snapshot_BIS(
            "test_20260721_why_lists_incomplete_trait_vote_implementations",
            &output,
        );

        assert!(output.contains("selected=(no known propagated caps)"));
        assert!(output.contains(
            "demo::EmptyIncomplete::rvs_parse@demo::Parser (known propagated caps: none)"
        ));
        assert!(output.contains(
            "demo::StatefulIncomplete::rvs_parse@demo::Parser (known propagated caps: S)"
        ));
        assert!(persisted_output.contains("selected=(no known propagated caps)"));
        assert!(!persisted_output.contains("implementation:"));
        assert!(implementation_output.contains("known trait impl contribution (incomplete): none"));
    }

    #[test]
    fn test_20260721_why_marks_incomplete_direct_callees() {
        let node = || FnNode {
            sources: BTreeSet::from([FnSource::rvs_new("src/lib.rs".into(), 1, 2)]),
            ..FnNode::default()
        };
        let mut graph = FnGraph::rvs_new();
        let caller_path = DefPath::from("demo::rvs_run_BS");
        let mut caller = node();
        caller.calls.extend(
            [
                "demo::rvs_complete",
                "demo::rvs_partial_S",
                "dep::cached_blocking",
                "dep::cached_empty",
                "dep::migrated_blocking",
                "dep::migrated_empty",
                "dep::opaque",
            ]
            .into_iter()
            .map(|c| {
                (
                    FunctionIdentity {
                        crate_id: 1,
                        def_path: DefPath::from(c),
                    },
                    CallEdgeType::Strong,
                )
            }),
        );
        graph.rvs_insert_M(caller_path.clone(), caller);
        graph.rvs_insert_M(DefPath::from("demo::rvs_complete"), node());

        let mut partial = node();
        partial.facts.has_static_ref = true;
        partial.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("dep::opaque_nested"),
            },
            CallEdgeType::Strong,
        );
        graph.rvs_insert_M(DefPath::from("demo::rvs_partial_S"), partial);

        let mut caps = crate::capsmap::CapsMap::rvs_new();
        caps.rvs_insert_info_M(
            crate::symbols::CapsMapKey::from("dep::cached_blocking"),
            crate::capability::CapabilityInfo::rvs_new(
                crate::capability::CapabilitySet::rvs_from_validated("B"),
                crate::capability::CapabilityBasis::Inferred,
                crate::capability::CapabilityCompleteness::Incomplete,
            ),
        );
        caps.rvs_insert_info_M(
            crate::symbols::CapsMapKey::from("dep::cached_empty"),
            crate::capability::CapabilityInfo::rvs_new(
                crate::capability::CapabilitySet::rvs_new(),
                crate::capability::CapabilityBasis::Inferred,
                crate::capability::CapabilityCompleteness::Incomplete,
            ),
        );
        caps.rvs_insert_info_M(
            crate::symbols::CapsMapKey::from("dep::migrated_blocking"),
            crate::capability::CapabilityInfo::rvs_new(
                crate::capability::CapabilitySet::rvs_from_validated("B"),
                crate::capability::CapabilityBasis::Inferred,
                crate::capability::CapabilityCompleteness::Unknown,
            ),
        );
        caps.rvs_insert_info_M(
            crate::symbols::CapsMapKey::from("dep::migrated_empty"),
            crate::capability::CapabilityInfo::rvs_new(
                crate::capability::CapabilitySet::rvs_new(),
                crate::capability::CapabilityBasis::Inferred,
                crate::capability::CapabilityCompleteness::Unknown,
            ),
        );
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &caps,
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let resolver = analysis.rvs_resolver(&graph, &caps);
        let cached_empty_path = DefPath::from("dep::cached_empty");
        let queried_caps = rvs_format_why_path_caps(&cached_empty_path, &analysis, &resolver);
        let migrated_empty_path = DefPath::from("dep::migrated_empty");
        let migrated_caps = rvs_format_why_path_caps(&migrated_empty_path, &analysis, &resolver);
        let behavior = graph
            .rvs_get(caller_path.rvs_as_str())
            .expect("never: test caller exists");
        let callee_output = rvs_format_why_callees(behavior, &analysis, &resolver).join("\n");
        let output = format!(
            "query=dep::cached_empty{queried_caps}\nmigrated=dep::migrated_empty{migrated_caps}\n{callee_output}\n"
        );
        rvs_snapshot_BIS("test_20260721_why_marks_incomplete_direct_callees", &output);

        assert_eq!(queried_caps, " (incomplete; no known caps)");
        assert_eq!(migrated_caps, " (no known caps; completeness unknown)");
        assert_eq!(
            rvs_format_why_callee_caps(
                Some(&crate::capability::CapabilitySet::rvs_new()),
                crate::capability::CapabilityCompleteness::Incomplete,
            ),
            "(no known caps) [incomplete]"
        );
        assert!(!output.contains("(pure) [incomplete]"));
        assert!(output.contains("dep::cached_empty: (no known caps) [incomplete]"));
        assert!(output.contains("dep::cached_blocking: B (Blocking) [incomplete]"));
        assert!(output.contains("dep::migrated_blocking: B (Blocking) [completeness unknown]"));
        assert!(output.contains("dep::migrated_empty: (no known caps) [completeness unknown]"));
        assert!(output.contains("demo::rvs_partial_S: S (SideEffect) [incomplete]"));
        assert!(output.contains("dep::opaque: (unknown)"));
    }

    #[test]
    fn test_20260715_why_uses_persisted_votes_and_outlier_eligibility() {
        let mut caps = crate::capsmap::CapsMap::rvs_new();
        caps.rvs_insert_info_M(
            crate::symbols::CapsMapKey::from("demo::Parser::rvs_parse"),
            crate::capability::CapabilityInfo::rvs_trait_vote(
                crate::capability::CapabilitySet::rvs_from_validated("B"),
                3,
                2,
                std::collections::BTreeMap::from([
                    (crate::capability::Capability::B, 2),
                    (crate::capability::Capability::S, 1),
                ]),
                crate::capability::CapabilityCompleteness::Complete,
            ),
        );
        let persisted =
            rvs_format_capsmap_knowledge(&caps, &DefPath::from("demo::Parser::rvs_parse"));
        caps.rvs_insert_info_M(
            crate::symbols::CapsMapKey::from("demo::Marker::rvs_mark"),
            crate::capability::CapabilityInfo::rvs_trait_vote(
                crate::capability::CapabilitySet::rvs_new(),
                2,
                1,
                std::collections::BTreeMap::new(),
                crate::capability::CapabilityCompleteness::Complete,
            ),
        );
        let persisted_empty =
            rvs_format_capsmap_knowledge(&caps, &DefPath::from("demo::Marker::rvs_mark"));

        let node = || FnNode {
            sources: BTreeSet::from([FnSource::rvs_new("src/lib.rs".into(), 1, 2)]),
            ..FnNode::default()
        };
        let mut graph = FnGraph::rvs_new();
        let mut declaration = node();
        declaration.has_body = false;
        graph.rvs_insert_M(DefPath::from("demo::Parser::rvs_parse"), declaration);
        for implementation in ["demo::Alpha", "demo::Beta"] {
            let mut implementation_node = node();
            implementation_node.is_trait_impl = true;
            graph.rvs_insert_M(
                DefPath::from(format!("{implementation}::rvs_parse@demo::Parser")),
                implementation_node,
            );
        }
        let mut generated = FnNode {
            is_trait_impl: true,
            ..FnNode::default()
        };
        generated.calls.insert(
            FunctionIdentity {
                crate_id: 1,
                def_path: DefPath::from("dep::effect"),
            },
            CallEdgeType::Strong,
        );
        let generated_path = DefPath::from("demo::Generated::rvs_parse@demo::Parser");
        graph.rvs_insert_M(generated_path.clone(), generated);
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &rvs_make_capsmap(&[("dep::effect", "S")]),
            &BTreeSet::from([CrateName::from("demo")]),
        );
        let generated_lines = rvs_format_trait_vote_summary(&analysis, &generated_path);
        let output = format!(
            "persisted:\n{}\npersisted empty:\n{}\ngenerated:\n{}\n",
            persisted.join("\n"),
            persisted_empty.join("\n"),
            generated_lines.join("\n")
        );
        rvs_snapshot_BIS(
            "test_20260715_why_uses_persisted_votes_and_outlier_eligibility",
            &output,
        );

        assert!(output.contains("persisted trait vote: selected=B, threshold=2/3"));
        assert!(output.contains("persisted trait vote: selected=none, threshold=1/2"));
        assert!(!output.contains("selected=(pure)"));
        assert!(output.contains("trait impl contribution: S"));
        assert!(!output.contains("trait impl outlier caps"));
        assert!(analysis.trait_impl_outliers.is_empty());
    }

    #[test]
    fn test_20260706_normalize_source_for_project() {
        let dir = rvs_make_temp_dir_BIS("normalize-source");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "fn parse() {}\n").unwrap();
        let source = FnSource::rvs_new(std::path::PathBuf::from("src/lib.rs"), 3, 8);

        let normalized = rename::rvs_normalize_source_for_project_BIS(&source, &dir).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_normalize_source_for_project",
            &format!(
                "abs={}\nstart={}\nend={}\n",
                normalized.file.is_absolute(),
                normalized.name_start,
                normalized.name_end
            ),
        );

        assert!(normalized.file.is_absolute());
        assert_eq!(normalized.name_start, 3);
        assert_eq!(normalized.name_end, 8);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_normalize_source_reports_missing_file() {
        let dir = rvs_make_temp_dir_BIS("normalize-source-missing");
        let source = FnSource::rvs_new(std::path::PathBuf::from("src/missing.rs"), 3, 8);

        let result = rename::rvs_normalize_source_for_project_BIS(&source, &dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260706_normalize_source_reports_missing_file",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260710_normalize_legacy_relative_source_rejects_ambiguous_bases() {
        let workspace = rvs_make_temp_dir_BIS("normalize-source-ambiguous");
        let member = workspace.join("member");
        std::fs::create_dir_all(member.join("src")).unwrap();
        std::fs::create_dir_all(member.join("member/src")).unwrap();
        std::fs::write(member.join("src/lib.rs"), "fn member_parse() {}\n").unwrap();
        std::fs::write(
            member.join("member/src/lib.rs"),
            "fn nested_member_parse() {}\n",
        )
        .unwrap();
        let source = FnSource::rvs_new(std::path::PathBuf::from("member/src/lib.rs"), 3, 8);

        let result = rename::rvs_normalize_source_for_project_BIS(&source, &member);
        let output = format!("{result:?}\n")
            .replace(&workspace.to_string_lossy().into_owned(), "$WORKSPACE");
        rvs_snapshot_BIS(
            "test_20260710_normalize_legacy_relative_source_rejects_ambiguous_bases",
            &output,
        );

        assert!(result.is_err(), "legacy source base must not be guessed");
        assert!(output.contains("ambiguous"));
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn test_20260710_normalize_relative_source_uses_recorded_exact_base() {
        let workspace = rvs_make_temp_dir_BIS("normalize-source-exact-base");
        let member = workspace.join("member");
        std::fs::create_dir_all(member.join("src")).unwrap();
        std::fs::create_dir_all(member.join("member/src")).unwrap();
        std::fs::write(member.join("src/lib.rs"), "fn member_parse() {}\n").unwrap();
        std::fs::write(
            member.join("member/src/lib.rs"),
            "fn nested_member_parse() {}\n",
        )
        .unwrap();
        let source = FnSource::rvs_new_relative(
            std::path::PathBuf::from("member/src/lib.rs"),
            workspace.clone(),
            3,
            8,
        );

        let normalized = rename::rvs_normalize_source_for_project_BIS(&source, &member).unwrap();
        let output = format!(
            "resolved={}\nbase_after_normalize={:?}\nrange={}..{}\n",
            normalized.file.display(),
            normalized.base,
            normalized.name_start,
            normalized.name_end,
        )
        .replace(&workspace.to_string_lossy().into_owned(), "$WORKSPACE");
        rvs_snapshot_BIS(
            "test_20260710_normalize_relative_source_uses_recorded_exact_base",
            &output,
        );

        assert_eq!(
            normalized.file,
            member.join("src/lib.rs").canonicalize().unwrap()
        );
        assert!(normalized.base.is_none());
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn test_20260710_recorded_source_base_does_not_fall_back() {
        let workspace = rvs_make_temp_dir_BIS("normalize-source-no-fallback");
        let member = workspace.join("member");
        std::fs::create_dir_all(member.join("src")).unwrap();
        std::fs::write(member.join("src/lib.rs"), "fn member_parse() {}\n").unwrap();
        let recorded_base = workspace.join("compiler-working-dir");
        let source =
            FnSource::rvs_new_relative(std::path::PathBuf::from("src/lib.rs"), recorded_base, 3, 8);

        let result = rename::rvs_normalize_source_for_project_BIS(&source, &member);
        let output = format!("{result:?}\n")
            .replace(&workspace.to_string_lossy().into_owned(), "$WORKSPACE");
        rvs_snapshot_BIS(
            "test_20260710_recorded_source_base_does_not_fall_back",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("recorded base"));
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn test_20260703_why_contract_summary_skips_executable_entry() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::main"),
            crate::artifacts::FnNode {
                is_entrypoint: true,
                sources: std::collections::BTreeSet::from([FnSource::rvs_new(
                    std::path::PathBuf::from("src/main.rs"),
                    3,
                    7,
                )]),
                ..crate::artifacts::FnNode::default()
            },
        );
        let analysis = PreparedLocalAnalysis::rvs_prepare_M(
            &mut graph,
            &crate::capsmap::CapsMap::rvs_new(),
            &std::collections::BTreeSet::from([CrateName::from("demo")]),
        );
        let lines = rvs_format_enforced_contract_diff_summary(
            &analysis.diffs,
            "demo::main",
            crate::capability::CapabilityCompleteness::Complete,
        );
        rvs_snapshot_BIS(
            "test_20260703_why_contract_summary_skips_executable_entry",
            &format!("lines={lines:?}\n"),
        );

        assert!(lines.is_empty());
        assert!(analysis.diffs.is_empty());
    }

    #[test]
    fn test_20260703_callee_absence_message_distinguishes_synthetic_nodes() {
        let output = format!(
            "collected={}\nbodyless={}\nsynthetic={}\n",
            rvs_callee_absence_message(true, false),
            rvs_callee_absence_message(false, false),
            rvs_callee_absence_message(false, true),
        );
        rvs_snapshot_BIS(
            "test_20260703_callee_absence_message_distinguishes_synthetic_nodes",
            &output,
        );

        assert_eq!(rvs_callee_absence_message(true, false), "(no callees)");
        assert_eq!(
            rvs_callee_absence_message(false, false),
            "(function has no body; callees intentionally absent)"
        );
        assert_eq!(
            rvs_callee_absence_message(false, true),
            "(function body not collected; callees unknown)"
        );
    }

    #[test]
    fn test_20260705_why_std_like_works_in_workspace_root() {
        let dir = rvs_make_temp_dir_BIS("why-std-workspace-root");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nmembers = []\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph-std")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph-std/callgraph.json"),
            r#"{
  "std::fs::rvs_read_BI": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("caps")).unwrap();
        std::fs::write(
            dir.join("caps/std"),
            rvs_caps_v2(&[("std::fs::rvs_read_BI", "BI")]),
        )
        .unwrap();

        let result = rvs_run_why_BIPST("std::fs::rvs_read_BI", &dir);
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS(
            "test_20260705_why_std_like_works_in_workspace_root",
            &output,
        );

        assert!(rvs_is_std_like_def_path("std::fs::rvs_read_BI"));
        assert!(!rvs_is_std_like_def_path("demo::rvs_read_BI"));
        assert!(result.is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_why_inexact_match_returns_error() {
        let dir = rvs_make_cargo_project_BIS(
            "why-inexact-match",
            "why-inexact-demo",
            &[("src/lib.rs", "pub fn rvs_parse() -> i32 { 1 }\n")],
        );

        let result = rvs_run_why_BIPST("parse", &dir);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260706_why_inexact_match_returns_error", &output);

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_why_accepts_unique_readable_specialized_path() {
        let dir = rvs_make_cargo_project_BIS(
            "why-readable-specialized-path",
            "why-readable-specialized",
            &[(
                "src/lib.rs",
                "#![allow(non_snake_case)]\n\npub struct Worker<T>(pub T);\n\nimpl Worker<u8> {\n    pub fn rvs_run(&self) {}\n}\n",
            )],
        );

        let result = rvs_run_why_BIPST("why_readable_specialized::Worker::rvs_run", &dir);
        let output = format!("{result:?}\n");
        rvs_snapshot_BIS(
            "test_20260715_why_accepts_unique_readable_specialized_path",
            &output,
        );

        assert!(result.is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260715_why_readable_specialized_path_detects_ambiguity() {
        let mut graph = FnGraph::rvs_new();
        graph.rvs_insert_M(
            DefPath::from("demo::Worker{impl#7538}::rvs_run"),
            crate::artifacts::FnNode::default(),
        );
        graph.rvs_insert_M(
            DefPath::from("demo::Worker{impl#753136}::rvs_run"),
            crate::artifacts::FnNode::default(),
        );

        let matches = rvs_why_function_matches(
            &graph,
            &std::collections::BTreeSet::new(),
            "demo::Worker::rvs_run",
        );
        let output = format!(
            "matches={}\nreadable_equal={}\n",
            matches.len(),
            matches
                .iter()
                .all(|path| path.rvs_user_path() == "demo::Worker::rvs_run")
        );
        rvs_snapshot_BIS(
            "test_20260715_why_readable_specialized_path_detects_ambiguity",
            &output,
        );

        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_20260715_why_marks_incomplete_capabilities() {
        let pure = crate::capability::CapabilitySet::rvs_new();
        let side_effect = crate::capability::CapabilitySet::rvs_from_validated("S");
        let output = format!(
            "pure={}\nincomplete_pure={}\nincomplete_known={}\nunknown={}\nunknown_incomplete={}\n",
            rvs_format_why_caps(
                Some(&pure),
                crate::capability::CapabilityCompleteness::Complete,
            ),
            rvs_format_why_caps(
                Some(&pure),
                crate::capability::CapabilityCompleteness::Incomplete,
            ),
            rvs_format_why_caps(
                Some(&side_effect),
                crate::capability::CapabilityCompleteness::Incomplete,
            ),
            rvs_format_why_caps(None, crate::capability::CapabilityCompleteness::Complete),
            rvs_format_why_caps(None, crate::capability::CapabilityCompleteness::Incomplete,),
        );
        rvs_snapshot_BIS("test_20260715_why_marks_incomplete_capabilities", &output);

        assert!(output.contains("inference incomplete"));
        assert!(output.contains("unknown"));
        assert!(output.contains("unknown; inference incomplete"));
    }

    #[test]
    fn test_20260702_annotate_uses_bin_crate_prefix() {
        let dir = rvs_make_temp_dir_BIS("annotate-bin-prefix");
        let cargo_toml = r#"[package]
name = "annotate-prefix-demo"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "cargo-rivus"
path = "src/main.rs"
"#;
        std::fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "fn main() { parse(); }\n\nfn parse() {}\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "cargo_rivus::main": {
    "calls": ["cargo_rivus::parse"],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  },
  "cargo_rivus::parse": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
        rvs_snapshot_BIS("test_20260702_annotate_uses_bin_crate_prefix", &source);

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("fn rvs_parse()"));
        assert!(source.contains("rvs_parse();"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260709_annotate_skips_integration_test_targets() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-skip-integration-tests",
            "annotate-skip-integration-demo",
            &[
                (
                    "src/lib.rs",
                    "pub fn parse(values: &mut Vec<u8>) { values.push(1); }\n",
                ),
                (
                    "tests/fixtures/mod.rs",
                    "pub struct TestServer;\n\nimpl TestServer {\n    pub fn url(&self, values: &mut Vec<u8>) { values.push(1); }\n}\n",
                ),
                (
                    "tests/upload_files.rs",
                    "mod fixtures;\n\n#[test]\nfn integration_fixture_keeps_plain_name() {\n    let server = fixtures::TestServer;\n    let mut values = Vec::new();\n    server.url(&mut values);\n}\n",
                ),
            ],
        );

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        let fixture = std::fs::read_to_string(dir.join("tests/fixtures/mod.rs")).unwrap();
        let output = format!("-- src/lib.rs --\n{source}\n-- tests/fixtures/mod.rs --\n{fixture}");
        rvs_snapshot_BIS(
            "test_20260709_annotate_skips_integration_test_targets",
            &output,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_parse_M"));
        assert!(fixture.contains("pub fn url"));
        assert!(!fixture.contains("rvs_url"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_annotate_renames_nested_main_helper() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-nested-main",
            "annotate-main-demo",
            &[(
                "src/main.rs",
                "mod cli { pub fn main() {} }\n\nfn main() { cli::main(); }\n",
            )],
        );
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_main_demo::main": {
    "calls": ["annotate_main_demo::cli::main"],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  },
  "annotate_main_demo::cli::main": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
        rvs_snapshot_BIS("test_20260702_annotate_renames_nested_main_helper", &source);

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_main()"));
        assert!(source.contains("cli::rvs_main();"));
        assert!(source.contains("fn main() {"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_annotate_renames_conflicting_duplicate_names() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-duplicate-name",
            "annotate-duplicate-demo",
            &[(
                "src/lib.rs",
                "pub mod a { pub fn parse() {} }\npub mod b { pub fn parse(_x: &mut u8) {} }\n",
            )],
        );
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_duplicate_demo::a::parse": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  },
  "annotate_duplicate_demo::b::parse": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": true,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260702_annotate_renames_conflicting_duplicate_names",
            &source,
        );

        assert!(
            result.is_ok(),
            "annotate should rename duplicate names by relative path: {result:?}"
        );
        assert!(source.contains("pub fn rvs_parse() {}"));
        assert!(source.contains("pub fn rvs_parse_M(_x: &mut u8) {}"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260703_annotate_renames_existing_rvs_wrong_suffix() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-existing-rvs-suffix",
            "annotate-rvs-demo",
            &[(
                "src/lib.rs",
                "pub trait FetchApi { type World; fn rvs_fetch_ABI(world: &Self::World) -> i32 { 1 } }\npub fn rvs_use<E: FetchApi>(world: &E::World) -> i32 { E::rvs_fetch_ABI(world) }\n",
            )],
        );

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260703_annotate_renames_existing_rvs_wrong_suffix",
            &source,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("fn rvs_fetch_P(world: &Self::World)"));
        assert!(source.contains("E::rvs_fetch_P(world)"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_renames_uppercase_function() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-uppercase-function",
            "annotate-uppercase-demo",
            &[("src/lib.rs", "pub fn Foo() -> i32 { 1 }\n")],
        );
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_uppercase_demo::Foo": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS("test_20260704_annotate_renames_uppercase_function", &source);

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_Foo()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_errors_when_candidates_match_no_symbols() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-unmatched-candidate",
            "annotate-unmatched-demo",
            &[("src/lib.rs", "pub fn existing() -> i32 { 1 }\n")],
        );
        let source_path = dir.join("src/lib.rs");
        let canonical_source = source_path.canonicalize().unwrap();
        let rename_map = HashMap::from([(
            FnSource::rvs_new(canonical_source, 0, 1),
            FnName::from("rvs_missing"),
        )]);
        let result = rename::rvs_apply_ra_source_renames_BIST(&dir, &rename_map);
        let output = format!("{result:?}\n").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260704_annotate_errors_when_candidates_match_no_symbols",
            &output,
        );

        assert!(result.is_err());
        assert!(output.contains("did not match any rust-analyzer symbol"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_errors_on_partial_rename() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-partial-rename",
            "annotate-partial-demo",
            &[("src/lib.rs", "pub fn parse() -> i32 { 1 }\n")],
        );
        let source_path = dir.join("src/lib.rs");
        let canonical_source = source_path.canonicalize().unwrap();
        let rename_map = HashMap::from([
            (
                FnSource::rvs_new(canonical_source.clone(), 7, 12),
                FnName::from("rvs_parse"),
            ),
            (
                FnSource::rvs_new(canonical_source, 0, 1),
                FnName::from("rvs_missing"),
            ),
        ]);
        let result = rename::rvs_apply_ra_source_renames_BIST(&dir, &rename_map);
        let source = std::fs::read_to_string(&source_path).unwrap();
        let output =
            format!("{result:?}\n{source}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS("test_20260704_annotate_errors_on_partial_rename", &output);

        assert!(result.is_err());
        assert!(output.contains("did not match any rust-analyzer symbol"));
        assert!(source.contains("pub fn parse()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_renames_out_of_line_module_function() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-out-of-line-module",
            "annotate-module-demo",
            &[
                ("src/lib.rs", "pub mod api;\n"),
                ("src/api.rs", "pub fn parse() -> i32 { 1 }\n"),
            ],
        );
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_module_demo::api::parse": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/api.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260704_annotate_renames_out_of_line_module_function",
            &source,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_parse()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_annotate_renames_path_attribute_module_function() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-path-attribute-module",
            "annotate-path-attr-demo",
            &[
                ("src/lib.rs", "#[path = \"wire.rs\"]\npub mod api;\n"),
                ("src/wire.rs", "pub fn parse() -> i32 { 1 }\n"),
            ],
        );

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/wire.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_annotate_renames_path_attribute_module_function",
            &source,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_parse()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_annotate_renames_lib_and_main_same_name_functions() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-lib-main-same-name",
            "annotate-samename-demo",
            &[
                ("src/lib.rs", "pub fn parse() -> i32 { 1 }\n"),
                (
                    "src/main.rs",
                    "fn parse() -> i32 { 2 }\n\nfn main() { let _ = parse(); }\n",
                ),
            ],
        );

        let result = rvs_run_annotate_BIPST(&dir);
        let lib_source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        let main_source = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_annotate_renames_lib_and_main_same_name_functions",
            &format!("lib:\n{lib_source}\nmain:\n{main_source}"),
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(lib_source.contains("pub fn rvs_parse()"));
        assert!(main_source.contains("fn rvs_parse()"));
        assert!(main_source.contains("rvs_parse();"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260713_annotate_renames_library_main_but_preserves_binary_entry() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-library-and-binary-main",
            "annotate-main-identity-demo",
            &[
                ("src/lib.rs", "pub fn main() -> i32 { 1 }\n"),
                ("src/main.rs", "fn main() {}\n"),
            ],
        );

        let result = rvs_run_annotate_BIPST(&dir);
        let lib_source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        let bin_source = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260713_annotate_renames_library_main_but_preserves_binary_entry",
            &format!("lib:\n{lib_source}\nbin:\n{bin_source}"),
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(lib_source.contains("pub fn main()"));
        assert_eq!(bin_source, "fn main() {}\n");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_annotate_skips_macro_generated_function_without_source() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-macro-generated-function",
            "annotate-macro-demo",
            &[(
                "src/lib.rs",
                "macro_rules! make_parse { () => { pub fn parse() -> i32 { 1 } }; }\nmake_parse!();\n",
            )],
        );

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260706_annotate_skips_macro_generated_function_without_source",
            &source,
        );

        assert!(
            result.is_ok(),
            "annotate should skip source-less macro function: {result:?}"
        );
        assert!(source.contains("pub fn parse()"));
        assert!(!source.contains("pub fn rvs_parse()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_renames_inherent_method() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-inherent-method",
            "annotate-method-demo",
            &[(
                "src/lib.rs",
                "pub struct User;\nimpl User { pub fn new() -> Self { Self } }\n",
            )],
        );
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_method_demo::User::new": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS("test_20260704_annotate_renames_inherent_method", &source);

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_new()"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260704_annotate_renames_generic_inherent_method() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-generic-inherent-method",
            "annotate-generic-method-demo",
            &[(
                "src/lib.rs",
                "pub struct User<T>(T);\nimpl<T> User<T> { pub fn new(value: T) -> Self { Self(value) } }\n",
            )],
        );
        std::fs::create_dir_all(dir.join("target/rivus-callgraph")).unwrap();
        std::fs::write(
            dir.join("target/rivus-callgraph/callgraph.json"),
            r#"{
  "annotate_generic_method_demo::User::new": {
    "calls": [],
    "has_body": true,
    "has_async": false,
    "is_unsafe_fn": false,
    "has_mut_param": false,
    "has_static_ref": false,
    "has_static_mut_ref": false,
    "has_thread_local_ref": false,
    "is_trait_impl": false,
    "is_test": false
  }
}
"#,
        )
        .unwrap();

        let result = rvs_run_annotate_BIPST(&dir);
        let source = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        rvs_snapshot_BIS(
            "test_20260704_annotate_renames_generic_inherent_method",
            &source,
        );

        assert!(result.is_ok(), "annotate should succeed: {result:?}");
        assert!(source.contains("pub fn rvs_new(value: T)"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260702_annotate_surfaces_callgraph_collection_error() {
        let dir = rvs_make_cargo_project_BIS(
            "annotate-callgraph-error",
            "annotate-callgraph-demo",
            &[("src/lib.rs", "pub fn parse( {\n")],
        );

        let result = rvs_run_annotate_BIPST(&dir);
        let output = format!("{result:?}").replace(&dir.to_string_lossy().into_owned(), "$TMP");
        rvs_snapshot_BIS(
            "test_20260702_annotate_surfaces_callgraph_collection_error",
            &output,
        );

        assert!(
            result.is_err(),
            "annotate should return the fresh collection failure"
        );
        assert!(output.contains("cargo check failed"));

        std::fs::remove_dir_all(dir).unwrap();
    }
}
