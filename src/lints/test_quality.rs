use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use rustc_lint::{LateContext, LintContext};

use super::callgraph::{FnBehavior, FnReportEntry};
use super::msg::Msg;
use super::{
    RVS_DUPLICATE_TEST, RVS_MISSING_TEST_OUTPUT, RVS_UNTESTED_GOOD_FN, RVS_UNTESTED_OK_FN,
};

/// `check_crate_post` — cross-cutting test quality checks and output writing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rvs_check_crate_post_MS<'tcx>(
    cx: &LateContext<'tcx>,
    test_names: &BTreeMap<String, Vec<rustc_span::Span>>,
    good_fns: &[(String, rustc_span::Span)],
    ok_fns: &[(String, rustc_span::Span)],
    test_call_names: &HashSet<String>,
    fn_report: &[FnReportEntry],
    callgraph: &BTreeMap<String, FnBehavior>,
    emit_report: bool,
    collect_callgraph: bool,
) {
    rvs_check_duplicate_tests_S(cx, test_names);
    rvs_check_missing_test_output_S(cx, test_names);
    rvs_check_untested_good_fns_S(cx, good_fns, test_call_names);
    rvs_check_untested_ok_fns_S(cx, ok_fns, test_call_names);
    rvs_write_report_S(cx, fn_report, emit_report);
    rvs_write_callgraph_S(cx, callgraph, collect_callgraph);
}

fn rvs_check_duplicate_tests_S<'tcx>(
    cx: &LateContext<'tcx>,
    test_names: &BTreeMap<String, Vec<rustc_span::Span>>,
) {
    for (name, spans) in test_names {
        if spans.len() > 1 {
            for sp in spans {
                cx.emit_span_lint(
                    RVS_DUPLICATE_TEST,
                    *sp,
                    Msg::new(*sp, format!("duplicate test '{name}'")),
                );
            }
        }
    }
}

fn rvs_check_missing_test_output_S<'tcx>(
    cx: &LateContext<'tcx>,
    test_names: &BTreeMap<String, Vec<rustc_span::Span>>,
) {
    if Path::new("test_out").is_dir() {
        for (name, spans) in test_names {
            let out_file = format!("test_out/{name}.out");
            if !Path::new(&out_file).exists() {
                if let Some(sp) = spans.first() {
                    cx.emit_span_lint(
                        RVS_MISSING_TEST_OUTPUT,
                        *sp,
                        Msg::new(*sp, format!("test '{name}' missing {out_file}")),
                    );
                }
            }
        }
    }
}

fn rvs_check_untested_good_fns_S<'tcx>(
    cx: &LateContext<'tcx>,
    good_fns: &[(String, rustc_span::Span)],
    test_call_names: &HashSet<String>,
) {
    for (name, span) in good_fns {
        if !test_call_names.contains(name)
            && !test_call_names
                .iter()
                .any(|tc| tc.rsplit("::").next().unwrap_or(tc) == name.as_str())
        {
            cx.emit_span_lint(
                RVS_UNTESTED_GOOD_FN,
                *span,
                Msg::new(*span, format!("good fn '{name}' not called by any test")),
            );
        }
    }
}

fn rvs_check_untested_ok_fns_S<'tcx>(
    cx: &LateContext<'tcx>,
    ok_fns: &[(String, rustc_span::Span)],
    test_call_names: &HashSet<String>,
) {
    for (name, span) in ok_fns {
        if !test_call_names.contains(name)
            && !test_call_names
                .iter()
                .any(|tc| tc.rsplit("::").next().unwrap_or(tc) == name.as_str())
        {
            cx.emit_span_lint(
                RVS_UNTESTED_OK_FN,
                *span,
                Msg::new(*span, format!("ok fn '{name}' not called by any test")),
            );
        }
    }
}

fn rvs_write_report_S<'tcx>(
    cx: &LateContext<'tcx>,
    fn_report: &[FnReportEntry],
    emit_report: bool,
) {
    if emit_report {
        if !fn_report.is_empty() {
            if let Ok(json) = serde_json::to_string(fn_report) {
                let report_dir = std::env::var("RIVUS_REPORT_DIR")
                    .unwrap_or_else(|_| "target/rivus-report".into());
                let crate_name = cx
                    .tcx
                    .crate_name(rustc_span::def_id::LOCAL_CRATE)
                    .as_str()
                    .to_string();
                let report_path = Path::new(&report_dir).join(format!("{crate_name}.json"));
                if let Some(parent) = report_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(mut f) = std::fs::File::create(&report_path) {
                    use std::io::Write;
                    let _ = f.write_all(json.as_bytes());
                    let _ = f.sync_all();
                }
            }
        }
    }
}

fn rvs_write_callgraph_S<'tcx>(
    cx: &LateContext<'tcx>,
    callgraph: &BTreeMap<String, FnBehavior>,
    collect_callgraph: bool,
) {
    if collect_callgraph {
        if !callgraph.is_empty() {
            if let Ok(json) = serde_json::to_string(callgraph) {
                let cg_dir = std::env::var("RIVUS_CALLGRAPH_DIR")
                    .unwrap_or_else(|_| "target/rivus-callgraph".into());
                let crate_name = cx
                    .tcx
                    .crate_name(rustc_span::def_id::LOCAL_CRATE)
                    .as_str()
                    .to_string();
                let cg_path = Path::new(&cg_dir).join(format!("{crate_name}.json"));
                if let Some(parent) = cg_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(mut f) = std::fs::File::create(&cg_path) {
                    use std::io::Write;
                    let _ = f.write_all(json.as_bytes());
                    let _ = f.sync_all();
                }
            }
        }
    }
}
