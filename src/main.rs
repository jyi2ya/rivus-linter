#![feature(rustc_private)]
#![feature(rustc_attrs)]
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(
    internal_features,
    reason = "rustc driver integration and the typed test-coverage diagnostic item require internal features"
)]
#![allow(
    non_snake_case,
    reason = "rvs_ functions use uppercase capability suffixes"
)]
#![allow(
    rivus::rvs_unsupported_implicit_execution,
    reason = "RAII guard Drop impls must clean up generation directories during panic unwinding"
)]
#![allow(
    rivus::rvs_unsupported_indirect_call,
    reason = "rustc-driver, salsa, and serde callbacks are dynamic by design; the linter is a single analysis engine and these edges are out of HIR reach"
)]
#![allow(
    rivus::rvs_untested_good_fn,
    reason = "rustc-driver internal helpers, tested via full lint pass"
)]
#![allow(
    rivus::rvs_missing_debug_derive,
    reason = "rustc TyCtxt and LateContext do not implement Debug"
)]

extern crate rustc_ast;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_hir_analysis;
extern crate rustc_hir_id;
extern crate rustc_interface;
extern crate rustc_lexer;
extern crate rustc_lint;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;

use std::env;
use std::path::PathBuf;
use std::process::{self, ExitCode};

use clap::{Parser, Subcommand};
use rustc_driver::Callbacks;
use rustc_interface::interface;
use rustc_session::EarlyDiagCtxt;
use rustc_session::config::ErrorOutputType;
mod artifacts;
mod callgraph;
mod capability;
mod capsmap;
mod diagnostic_source;
mod environment;
mod function_classification;
mod inference;
mod lints;
mod offline_caps;
mod symbols;
#[cfg(test)]
mod test_support;

use environment::{
    analysis_commands, infer_commands, lint_driver, rename, report_commands, setup, workspace,
};

#[cfg(test)]
const RIVUS_CONTRIBUTOR_POLICY: &str = include_str!("../rivus.md");
const RIVUS_PROJECT_TEMPLATE: &str = include_str!("rivus-project-template.md");
const RIVUS_MANUAL: &str = include_str!("rivus-manual.md");

// ─── Driver mode ─────────────────────────────────────────────────────────

#[derive(Debug)]
struct RivusCallbacks {
    driver_config: workspace::RivusDriverConfig,
}

impl Callbacks for RivusCallbacks {
    fn config(&mut self, config: &mut interface::Config) {
        let previous = config.register_lints.take();
        let driver_config = self.driver_config.clone();
        config.register_lints = Some(Box::new(move |_sess, lint_store| {
            if let Some(previous) = &previous {
                previous(_sess, lint_store);
            }
            lint_store.register_lints(lints::RIVUS_LINTS);
            let driver_config = driver_config.clone();
            lint_store.register_late_pass(move |_| {
                let lint_config = lint_driver::rvs_prepare_lint_config_BIS(driver_config.clone());
                Box::new(lints::RivusLintPass::rvs_new(lint_config))
            });
        }));
    }
}

#[derive(Debug)]
struct DefaultCallbacks;

impl Callbacks for DefaultCallbacks {}

/// # Panics
///
/// Panics if the current executable path is invalid or cargo cannot be spawned.
fn rvs_run_driver_BIST() -> ExitCode {
    let raw_args: Vec<String> = env::args().collect();
    let direct_rustc = raw_args.get(1).is_some_and(|arg| arg == "--rustc");
    let driver_config = if direct_rustc {
        None
    } else {
        match workspace::rvs_load_driver_protocol_BIS() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("invalid Rivus driver protocol: {error}");
                return ExitCode::from(2u8);
            }
        }
    };
    let early_dcx = EarlyDiagCtxt::new(ErrorOutputType::default());
    rustc_driver::init_rustc_env_logger(&early_dcx);

    rustc_driver::catch_with_exit_code(move || {
        let mut args = raw_args;

        if args.get(1).is_some_and(|arg| arg == "--rustc") {
            args.remove(1);
            if let Some(arg0) = args.first_mut() {
                *arg0 = "rustc".to_string();
            }
            return rustc_driver::run_compiler(&args, &mut DefaultCallbacks);
        }

        let wrapper_mode =
            args.get(1).is_some_and(|arg| rvs_is_rustc_arg(arg)) || driver_config.is_some();
        if wrapper_mode {
            args.remove(1);
        }

        // In wrapper mode, replace --cap-lints allow with --cap-lints warn
        // so our lint pass runs on std/core/alloc (cargo passes --cap-lints
        // allow for them, which causes rustc to skip the lint pass entirely).
        // Using --cap-lints warn (not removing entirely) prevents compilation
        // failures from std's #[deny(...)] attributes.
        let rivus_enabled = wrapper_mode && driver_config.is_some();
        if rivus_enabled {
            rvs_rewrite_cap_lints_M(&mut args, CapLintsRewrite::AllowToWarn);
        }
        let collection_lints = driver_config.as_ref().and_then(|config| match config.mode {
            workspace::RivusDriverMode::Callgraph { lints, .. } => Some(lints),
            _ => None,
        });
        let callgraph_lint_mode =
            rvs_callgraph_lint_mode(wrapper_mode, rivus_enabled, collection_lints);
        rvs_add_callgraph_lint_args_M(&mut args, callgraph_lint_mode);

        if rivus_enabled {
            rustc_driver::run_compiler(
                &args,
                &mut RivusCallbacks {
                    driver_config: driver_config
                        .expect("never: enabled Rivus driver has validated configuration"),
                },
            )
        } else {
            rustc_driver::run_compiler(&args, &mut DefaultCallbacks)
        }
    })
}

fn rvs_env_flag_is_one_BS(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| value == "1")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallgraphLintMode {
    Normal,
    Collect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapLintsRewrite {
    AllowToWarn,
    ForceWarn,
}

const fn rvs_callgraph_lint_mode(
    wrapper_mode: bool,
    rivus_enabled: bool,
    collection_lints: Option<workspace::CollectionLints>,
) -> CallgraphLintMode {
    // Only silent collection suppresses diagnostics; the lint-bearing
    // collection of `cargo rivus check` keeps normal lint levels.
    match (wrapper_mode && rivus_enabled, collection_lints) {
        (true, Some(workspace::CollectionLints::Silent)) => CallgraphLintMode::Collect,
        _ => CallgraphLintMode::Normal,
    }
}

fn rvs_add_callgraph_lint_args_M(args: &mut Vec<String>, mode: CallgraphLintMode) {
    if mode == CallgraphLintMode::Collect {
        rvs_rewrite_cap_lints_M(args, CapLintsRewrite::ForceWarn);
        args.push("-Awarnings".to_string());
        args.push("-Aunfulfilled_lint_expectations".to_string());
    }
}

fn rvs_rewrite_cap_lints_M(args: &mut Vec<String>, rewrite: CapLintsRewrite) {
    let mut found = false;
    let mut index = 0usize;
    while index < args.len() {
        if args.get(index).is_some_and(|arg| arg == "--cap-lints") {
            if let Some(value) = args.get_mut(index + 1) {
                if rewrite == CapLintsRewrite::ForceWarn || value == "allow" {
                    *value = "warn".to_string();
                }
                found = true;
            }
            index += 2;
            continue;
        }
        if let Some(arg) = args.get_mut(index)
            && let Some(value) = arg.strip_prefix("--cap-lints=")
        {
            if rewrite == CapLintsRewrite::ForceWarn || value == "allow" {
                *arg = "--cap-lints=warn".to_string();
            }
            found = true;
        }
        index += 1;
    }
    if rewrite == CapLintsRewrite::ForceWarn && !found {
        args.push("--cap-lints=warn".to_string());
    }
}

#[cfg(test)]
fn rvs_rivus_enabled_value(value: &str) -> bool {
    value == "1"
}

fn rvs_is_rustc_arg(arg: &str) -> bool {
    std::path::Path::new(arg)
        .file_name()
        .is_some_and(|name| name == "rustc" || name == "rustc.exe")
}

fn rvs_should_enter_driver(args: &[String]) -> bool {
    args.get(1)
        .is_some_and(|arg| arg == "--rustc" || rvs_is_rustc_arg(arg))
}

fn rvs_should_enter_driver_with_wrapper_marker(args: &[String], wrapper_marker: bool) -> bool {
    rvs_should_enter_driver(args)
        || (wrapper_marker
            && args.get(1).is_some_and(|arg| !rvs_is_cli_entry_arg(arg))
            && args
                .get(2)
                .is_some_and(|arg| rvs_is_cargo_rustc_leading_arg(arg)))
}

fn rvs_is_cli_entry_arg(arg: &str) -> bool {
    matches!(
        arg,
        "rivus"
            | "check"
            | "report"
            | "setup"
            | "infer-capsmap"
            | "strip"
            | "annotate"
            | "infer-std"
            | "why"
            | "usage"
            | "help"
            | "--help"
            | "-h"
            | "--version"
            | "-V"
    )
}

fn rvs_is_cargo_rustc_leading_arg(arg: &str) -> bool {
    matches!(
        arg,
        "-" | "-vV" | "-V" | "--version" | "--crate-name" | "--print"
    ) || arg.starts_with("--print=")
}

fn rvs_parse_function_query(function: &str) -> Result<String, String> {
    if function.trim().is_empty() {
        Err("function def_path must not be blank".to_string())
    } else {
        Ok(function.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;
    use clap::CommandFactory;

    #[test]
    fn test_20260706_rewrite_cap_lints_preserves_allow_crate_name() {
        let mut args = vec![
            "rustc".to_string(),
            "--crate-name".to_string(),
            "allow".to_string(),
            "--cap-lints".to_string(),
            "allow".to_string(),
        ];

        rvs_rewrite_cap_lints_M(&mut args, CapLintsRewrite::AllowToWarn);
        let output = format!("{}\n", args.join("\n"));
        rvs_snapshot_BIS(
            "test_20260706_rewrite_cap_lints_preserves_allow_crate_name",
            &output,
        );

        assert_eq!(args[2], "allow");
        assert_eq!(args[4], "warn");
    }

    #[test]
    fn test_20260706_rewrite_cap_lints_equals_allow() {
        let mut args = vec!["rustc".to_string(), "--cap-lints=allow".to_string()];

        rvs_rewrite_cap_lints_M(&mut args, CapLintsRewrite::AllowToWarn);
        let output = format!("{}\n", args.join("\n"));
        rvs_snapshot_BIS("test_20260706_rewrite_cap_lints_equals_allow", &output);

        assert_eq!(args[1], "--cap-lints=warn");
    }

    #[test]
    fn test_20260706_rivus_enabled_requires_one() {
        let rows = [("", false), ("0", false), ("1", true), ("true", false)];
        let output = rows
            .iter()
            .map(|(value, _)| format!("{value:?}={}", rvs_rivus_enabled_value(value)))
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS("test_20260706_rivus_enabled_requires_one", &output);

        for (value, expected) in rows {
            assert_eq!(rvs_rivus_enabled_value(value), expected);
        }
    }

    #[test]
    fn test_20260706_driver_gate_detects_wrapper_args() {
        let cli = vec!["cargo-rivus".to_string(), "usage".to_string()];
        let direct = vec!["cargo-rivus".to_string(), "--rustc".to_string()];
        let wrapper = vec![
            "cargo-rivus".to_string(),
            "/toolchain/bin/rustc".to_string(),
        ];
        let windows_wrapper = vec!["cargo-rivus".to_string(), "rustc.exe".to_string()];
        let false_positive = vec!["cargo-rivus".to_string(), "rustc.json".to_string()];
        let output = format!(
            "cli={}\ndirect={}\nwrapper={}\nwindows_wrapper={}\nfalse_positive={}\n",
            rvs_should_enter_driver(&cli),
            rvs_should_enter_driver(&direct),
            rvs_should_enter_driver(&wrapper),
            rvs_should_enter_driver(&windows_wrapper),
            rvs_should_enter_driver(&false_positive),
        );
        rvs_snapshot_BIS("test_20260706_driver_gate_detects_wrapper_args", &output);

        assert!(!rvs_should_enter_driver(&cli));
        assert!(rvs_should_enter_driver(&direct));
        assert!(rvs_should_enter_driver(&wrapper));
        assert!(rvs_should_enter_driver(&windows_wrapper));
        assert!(!rvs_should_enter_driver(&false_positive));
        assert!(rvs_is_rustc_arg("/toolchain/bin/rustc"));
        assert!(rvs_is_rustc_arg("rustc.exe"));
        assert!(!rvs_is_rustc_arg("rustc.json"));
    }

    #[test]
    fn test_20260714_driver_gate_uses_wrapper_marker_for_custom_compiler() {
        let custom_wrapper = vec![
            "cargo-rivus".to_string(),
            "/toolchain/bin/rustc-custom".to_string(),
            "--crate-name".to_string(),
            "demo".to_string(),
        ];
        let cli_named_compiler = vec![
            "cargo-rivus".to_string(),
            "/toolchain/bin/check".to_string(),
            "--crate-name".to_string(),
            "demo".to_string(),
        ];
        let marked = rvs_should_enter_driver_with_wrapper_marker(&custom_wrapper, true);
        let unmarked = rvs_should_enter_driver_with_wrapper_marker(&custom_wrapper, false);
        let cli_named = rvs_should_enter_driver_with_wrapper_marker(&cli_named_compiler, true);
        let output = format!("marked={marked}\nunmarked={unmarked}\ncli_named={cli_named}\n");
        rvs_snapshot_BIS(
            "test_20260714_driver_gate_uses_wrapper_marker_for_custom_compiler",
            &output,
        );

        assert!(marked);
        assert!(!unmarked);
        assert!(cli_named);
    }

    #[test]
    fn test_20260714_driver_gate_ignores_inherited_wrapper_marker_for_cli() {
        let usage = vec!["cargo-rivus".to_string(), "usage".to_string()];
        let check = vec![
            "cargo-rivus".to_string(),
            "check".to_string(),
            "--version".to_string(),
        ];
        let output = format!(
            "usage={}\ncheck={}\n",
            rvs_should_enter_driver_with_wrapper_marker(&usage, true),
            rvs_should_enter_driver_with_wrapper_marker(&check, true),
        );
        rvs_snapshot_BIS(
            "test_20260714_driver_gate_ignores_inherited_wrapper_marker_for_cli",
            &output,
        );

        assert!(!rvs_should_enter_driver_with_wrapper_marker(&usage, true));
        assert!(!rvs_should_enter_driver_with_wrapper_marker(&check, true));
    }

    #[test]
    fn test_20260706_disabled_wrapper_does_not_rewrite_cap_lints() {
        let mut args = vec!["rustc".to_string(), "--cap-lints=allow".to_string()];
        let enabled = false;
        if enabled {
            rvs_rewrite_cap_lints_M(&mut args, CapLintsRewrite::AllowToWarn);
        }
        let output = args.join("\n") + "\n";
        rvs_snapshot_BIS(
            "test_20260706_disabled_wrapper_does_not_rewrite_cap_lints",
            &output,
        );

        assert_eq!(args[1], "--cap-lints=allow");
    }

    #[test]
    fn test_20260714_callgraph_suppresses_intermediate_expectations() {
        let mut enabled = vec!["rustc".to_string()];
        let mut existing_cap = vec![
            "rustc".to_string(),
            "--cap-lints".to_string(),
            "allow".to_string(),
        ];
        let mut disabled = enabled.clone();
        rvs_add_callgraph_lint_args_M(&mut enabled, CallgraphLintMode::Collect);
        rvs_add_callgraph_lint_args_M(&mut existing_cap, CallgraphLintMode::Collect);
        rvs_add_callgraph_lint_args_M(&mut disabled, CallgraphLintMode::Normal);
        let output = format!(
            "enabled={}\nexisting_cap={}\ndisabled={}\n",
            enabled.join(" "),
            existing_cap.join(" "),
            disabled.join(" ")
        );
        rvs_snapshot_BIS(
            "test_20260714_callgraph_suppresses_intermediate_expectations",
            &output,
        );

        assert_eq!(
            enabled
                .get(enabled.len().saturating_sub(2))
                .map(String::as_str),
            Some("-Awarnings")
        );
        assert_eq!(
            enabled.last().map(String::as_str),
            Some("-Aunfulfilled_lint_expectations")
        );
        assert_eq!(disabled, ["rustc"]);
        assert!(enabled.iter().any(|arg| arg == "--cap-lints=warn"));
        assert_eq!(existing_cap.get(2).map(String::as_str), Some("warn"));
        assert_eq!(
            rvs_callgraph_lint_mode(true, true, Some(workspace::CollectionLints::Check)),
            CallgraphLintMode::Normal
        );
        assert_eq!(
            rvs_callgraph_lint_mode(true, true, None),
            CallgraphLintMode::Normal
        );
        assert_eq!(
            rvs_callgraph_lint_mode(true, true, Some(workspace::CollectionLints::Silent)),
            CallgraphLintMode::Collect
        );
    }

    #[test]
    fn test_20260715_cli_version_is_available() {
        let error = Cli::try_parse_from(["cargo-rivus", "--version"]).unwrap_err();
        let output = format!("kind={:?}\n{error}", error.kind());
        rvs_snapshot_BIS("test_20260715_cli_version_is_available", &output);

        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn test_20260716_cli_identity_help_and_why_validation() {
        let version = Cli::try_parse_from(["cargo-rivus", "--version"]).unwrap_err();
        let root_help = Cli::command().render_long_help().to_string();
        let root_usage = root_help
            .lines()
            .find(|line| line.starts_with("Usage:"))
            .expect("never: root help has a usage line");
        let mut command = Cli::command();
        let infer_help = command
            .find_subcommand_mut("infer-capsmap")
            .expect("never: infer-capsmap subcommand exists")
            .render_long_help()
            .to_string();
        let normalized_infer_help = infer_help.split_whitespace().collect::<Vec<_>>().join(" ");
        let expected_infer_output_help =
            "-o, --output <OUTPUT> Output path for direct external deps capsmap";
        let infer_allows_arbitrary_output =
            normalized_infer_help.contains(expected_infer_output_help);
        let root_warns_about_linter_bugs = root_help.contains(
            "stop modifying the target project and report it to a human; do not add workarounds",
        );
        let blank = Cli::try_parse_from(["cargo-rivus", "why", ""]).unwrap_err();
        let whitespace = Cli::try_parse_from(["cargo-rivus", "why", "   "]).unwrap_err();
        let output = format!(
            "version_kind={:?}\nversion={}\nroot_usage={root_usage}\nroot_warns_about_linter_bugs={root_warns_about_linter_bugs}\ninfer_allows_arbitrary_output={infer_allows_arbitrary_output}\nblank_kind={:?}\nwhitespace_kind={:?}\n",
            version.kind(),
            version.to_string().trim(),
            blank.kind(),
            whitespace.kind(),
        );
        rvs_snapshot_BIS(
            "test_20260716_cli_identity_help_and_why_validation",
            &output,
        );

        assert_eq!(version.kind(), clap::error::ErrorKind::DisplayVersion);
        assert_eq!(root_usage, "Usage: cargo rivus [COMMAND]");
        assert_eq!(blank.kind(), clap::error::ErrorKind::ValueValidation);
        assert_eq!(whitespace.kind(), clap::error::ErrorKind::ValueValidation);
        assert!(root_warns_about_linter_bugs);
        assert!(infer_allows_arbitrary_output);
    }

    #[test]
    fn test_20260728_suspected_linter_bug_policy_is_documented() {
        let root_agents = include_str!("../AGENTS.md");
        let readme = include_str!("../readme.pod");
        let contributor_status = RIVUS_CONTRIBUTOR_POLICY
            .lines()
            .find(|line| line.contains("开发状态警告（给 LLM）"))
            .expect("never: contributor policy documents the development warning");
        let contributor_issue = RIVUS_CONTRIBUTOR_POLICY
            .lines()
            .find(|line| line.contains("实际使用问题记录（给 LLM）"))
            .expect("never: contributor policy documents issue recording");
        let manual_policy = RIVUS_MANUAL
            .lines()
            .find(|line| line.contains("给 LLM 的强制规则"))
            .expect("never: manual documents suspected linter bug handling");
        let readme_status = readme
            .lines()
            .find(|line| line.contains("仍在积极开发"))
            .expect("never: README documents development status");
        let readme_policy = readme
            .lines()
            .find(|line| line.contains("必须立即停止当前工作"))
            .expect("never: README documents suspected linter bug handling");
        let agents_matches_rivus = root_agents == RIVUS_CONTRIBUTOR_POLICY;
        let output = format!(
            "agents_matches_rivus={agents_matches_rivus}\ncontributor_status={contributor_status}\ncontributor_issue={contributor_issue}\nmanual_policy={manual_policy}\nreadme_status={readme_status}\nreadme_policy={readme_policy}\n"
        );
        rvs_snapshot_BIS(
            "test_20260728_suspected_linter_bug_policy_is_documented",
            &output,
        );

        assert!(agents_matches_rivus);
        assert!(contributor_status.contains("立即停止当前工作并向人类汇报"));
        assert!(contributor_status.contains("workaround"));
        assert!(contributor_issue.contains("~/var/linter-issues/"));
        for required in ["环境与版本", "复现命令", "实际结果", "预期结果", "影响"]
        {
            assert!(contributor_issue.contains(required));
        }
        assert!(manual_policy.contains("等待进一步决定"));
        assert!(manual_policy.contains("workaround"));
        assert!(readme_status.contains("已知和未知 bug"));
        assert!(readme_policy.contains("等待进一步决定"));
        assert!(readme_policy.contains("workaround"));
    }

    #[test]
    fn test_20260728_manual_documents_consumed_argument_type_structure() {
        let consumed_arg = RIVUS_MANUAL
            .lines()
            .find(|line| line.contains("ConsumedArgOnErrorWarning"))
            .expect("never: manual documents ConsumedArgOnErrorWarning");
        let output = format!("consumed={consumed_arg}\n");
        rvs_snapshot_BIS(
            "test_20260728_manual_documents_consumed_argument_type_structure",
            &output,
        );

        assert!(consumed_arg.contains("规范化 rustc 类型"));
        assert!(consumed_arg.contains("错误类型结构"));
        assert!(consumed_arg.contains("不会递归检查 ADT"));
    }

    #[test]
    fn test_20260728_persistent_caps_update_lock_is_documented() {
        let root_agents = include_str!("../AGENTS.md");
        let theory = include_str!("../docs/theory/capability-knowledge.md");
        let contributor_lock = RIVUS_CONTRIBUTOR_POLICY
            .lines()
            .find(|line| line.contains(".rivus-caps.lock"))
            .expect("never: contributor policy documents the persistent caps lock");
        let manual_lock = RIVUS_MANUAL
            .lines()
            .find(|line| line.contains(".rivus-caps.lock"))
            .expect("never: manual documents the persistent caps lock");
        let theory_lock = theory
            .lines()
            .find(|line| line.contains(".rivus-caps.lock"))
            .expect("never: capability theory documents the persistent caps lock");
        let agents_matches_rivus = root_agents == RIVUS_CONTRIBUTOR_POLICY;
        let output = format!(
            "agents_matches_rivus={agents_matches_rivus}\ncontributor_lock={contributor_lock}\nmanual_lock={manual_lock}\ntheory_lock={theory_lock}\n"
        );
        rvs_snapshot_BIS(
            "test_20260728_persistent_caps_update_lock_is_documented",
            &output,
        );

        assert!(agents_matches_rivus);
        for documented_lock in [contributor_lock, manual_lock, theory_lock] {
            assert!(documented_lock.contains("进程内 registry"));
            assert!(documented_lock.contains("POSIX record lock"));
            assert!(documented_lock.contains("fork"));
        }
        assert!(contributor_lock.contains("项目根目录下持久存在的"));
        assert!(manual_lock.contains("项目根目录下持久存在的"));
        assert!(theory_lock.contains("项目根目录下持久存在的"));
    }

    #[test]
    fn test_20260729_setup_cli_describes_non_destructive_merge() {
        let mut command = Cli::command();
        let setup_help = command
            .find_subcommand_mut("setup")
            .expect("never: setup subcommand exists")
            .render_long_help()
            .to_string();
        let normalized = setup_help.split_whitespace().collect::<Vec<_>>().join(" ");
        let managed_agents = normalized.contains("managed public Rivus section into AGENTS.md");
        let missing_clippy = normalized.contains("missing clippy lints to Cargo.toml");
        let force_option = normalized.contains("--force");
        let output = format!(
            "managed_agents={managed_agents}\nmissing_clippy={missing_clippy}\nforce_option={force_option}\n"
        );
        rvs_snapshot_BIS(
            "test_20260729_setup_cli_describes_non_destructive_merge",
            &output,
        );

        assert!(managed_agents);
        assert!(missing_clippy);
        assert!(!force_option);
    }

    #[test]
    fn test_20260729_public_docs_match_setup_capability_and_cache_semantics() {
        let agents = include_str!("../AGENTS.md");
        let rivus = include_str!("../rivus.md");
        let manual = RIVUS_MANUAL;
        let theory = include_str!("../docs/theory/function-graph.md");
        let expected_schema = format!("schema v{}", artifacts::CALLGRAPH_SCHEMA_VERSION);
        let policy_files_identical = agents == rivus;
        let schema_current = theory.contains(&expected_schema);
        let cache_current = manual.contains("target/rivus-callgraph-std.json");
        let barriers_exact = agents.contains("`B/I/P/S/T` 五个可传播能力")
            && theory.contains("`B/I/P/S/T` 从被调用方传播到调用方")
            && RIVUS_PROJECT_TEMPLATE.contains("exactly `B/I/P/S/T`");
        let setup_markers = manual.contains(setup::RIVUS_AGENTS_BEGIN_MARKER)
            && manual.contains(setup::RIVUS_AGENTS_END_MARKER);
        let no_overwrite_claim =
            !manual.contains("AGENTS.md` 每次覆盖写入") && !manual.contains("覆盖写入 `AGENTS.md`");
        let overlapping_counts = manual.contains("这些计数互相重叠，不能相加得到 Total");
        let output = format!(
            "policy_files_identical={policy_files_identical}\nschema_current={schema_current}\ncache_current={cache_current}\nbarriers_exact={barriers_exact}\nsetup_markers={setup_markers}\nno_overwrite_claim={no_overwrite_claim}\noverlapping_counts={overlapping_counts}\n"
        );
        rvs_snapshot_BIS(
            "test_20260729_public_docs_match_setup_capability_and_cache_semantics",
            &output,
        );

        assert!(policy_files_identical);
        assert!(schema_current);
        assert!(cache_current);
        assert!(barriers_exact);
        assert!(setup_markers);
        assert!(no_overwrite_claim);
        assert!(overlapping_counts);
    }

    #[test]
    fn test_20260809_parse_function_query_table() {
        let cases = [
            ("std::fs::read_to_string", true),
            ("  ", false),
            ("", false),
            ("\t\n", false),
            ("core::clone::Clone::clone", true),
        ];
        let mut output = String::new();
        for (input, should_pass) in cases {
            let result = rvs_parse_function_query(input);
            let passed = result.is_ok();
            output.push_str(&format!("{input:?} => {passed}\n"));
            assert_eq!(passed, should_pass, "{input:?}");
        }
        rvs_snapshot_BIS("test_20260809_parse_function_query_table", &output);
    }
}

// ─── CLI mode ────────────────────────────────────────────────────────────

#[derive(Debug, Parser)]
#[command(name = "cargo-rivus", bin_name = "cargo rivus")]
#[command(about = "Check function capability compliance in Rust source code")]
#[command(
    long_about = "Check function capability compliance in Rust source code.\n\nExperimental: cargo-rivus is under active development and has many known and unknown bugs. If a diagnostic or inferred capability appears to be a linter bug, stop modifying the target project and report it to a human; do not add workarounds."
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Check capability compliance via rustc plugin (cargo check)
    Check {
        /// Extra cargo check args
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Report line-count breakdown by capability
    Report {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Set up project: merge a managed public Rivus section into AGENTS.md and add missing clippy lints to Cargo.toml
    Setup {
        /// Path to target project directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Collect callgraph and infer capsmap from seed annotations
    InferCapsmap {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output path for direct external deps capsmap
        #[arg(short = 'o', long = "output", required = true)]
        output: PathBuf,
    },
    /// Strip rvs_ prefix and capability suffix from all functions
    Strip {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Infer capabilities and add rvs_ prefix and capability suffix to all functions
    Annotate {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Infer capsmap for std/core/alloc via -Zbuild-std (requires nightly)
    InferStd {
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output path for std capsmap (e.g. caps/std)
        #[arg(short = 'o', long = "output", required = true)]
        output: PathBuf,
    },
    /// Show why a function has its caps (prints callees and their caps)
    Why {
        /// Function def_path to explain (e.g. std::fs::read)
        #[arg(value_parser = rvs_parse_function_query)]
        function: String,
        /// Path to project directory (must contain Cargo.toml)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Display the detailed tool manual
    Usage,
}

fn main() -> ExitCode {
    let raw_args: Vec<String> = env::args().collect();
    if rvs_should_enter_driver_with_wrapper_marker(
        &raw_args,
        rvs_env_flag_is_one_BS("RIVUS_WRAPPER"),
    ) {
        return rvs_run_driver_BIST();
    }

    // Cargo subcommands: `cargo rivus check` invokes `cargo-rivus rivus check`.
    // Strip the leading "rivus" arg so clap sees the real subcommand.
    let filtered_args: Vec<String> = if raw_args.get(1).map(|s| s.as_str()) == Some("rivus") {
        let mut v = raw_args;
        v.remove(1);
        v
    } else {
        raw_args
    };
    let cli = Cli::parse_from(filtered_args);

    let result: Result<(), String> = match cli.command {
        None => {
            let empty_args: Vec<String> = Vec::new();
            if let Err(code) = workspace::rvs_run_cargo_check_BIST(&empty_args) {
                process::exit(code);
            }
            Ok(())
        }
        Some(Commands::Check { args }) => {
            if let Err(code) = workspace::rvs_run_cargo_check_BIST(&args) {
                process::exit(code);
            }
            Ok(())
        }
        Some(Commands::Report { path }) => report_commands::rvs_run_report_BIST(&path),
        Some(Commands::Setup { path }) => {
            setup::rvs_run_setup_BIST(&path).map_err(|error| error.to_string())
        }
        Some(Commands::InferCapsmap { path, output }) => {
            infer_commands::rvs_run_infer_capsmap_BIST(&path, &output)
        }
        Some(Commands::InferStd { path, output }) => {
            infer_commands::rvs_run_infer_std_BIST(&path, &output)
        }
        Some(Commands::Strip { path }) => rename::rvs_strip_BIST(&path),
        Some(Commands::Annotate { path }) => analysis_commands::rvs_run_annotate_BIST(&path),
        Some(Commands::Why { function, path }) => {
            analysis_commands::rvs_run_why_BIST(&function, &path)
        }
        Some(Commands::Usage) => {
            print!("{RIVUS_MANUAL}");
            Ok(())
        }
    };
    if let Err(error) = result {
        eprintln!("Error: {error}");
        return ExitCode::from(2u8);
    }
    ExitCode::SUCCESS
}
