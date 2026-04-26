#![allow(non_snake_case)]

use std::collections::BTreeSet;

use rivus_linter::capability::{
    Capability, CapabilityParseError, CapabilitySet, rvs_parse_function,
};
use rivus_linter::capsmap::CapsMap;
use rivus_linter::check::{InferenceKind, rvs_check_path_BI, rvs_check_source};
use rivus_linter::extract::rvs_extract_functions;
use rivus_linter::report::rvs_build_report;

fn rvs_write_snapshot_BI(name: &str, content: &str) {
    std::fs::create_dir_all("test_out").unwrap();
    std::fs::write(format!("test_out/{name}.out"), content).unwrap();
}

fn rvs_snapshot_BI(name: &str, content: impl std::fmt::Display) {
    let content = content.to_string();
    rvs_write_snapshot_BI(name, &content);
}

fn rvs_format_caps(set: &BTreeSet<Capability>) -> String {
    set.iter()
        .map(|c| c.rvs_as_char().to_string())
        .collect::<Vec<_>>()
        .join("")
}

// ─── 能力萃取 ─────────────────────────────────────────────

#[test]
fn test_20260418_parse_no_suffix() {
    let (base, caps) = rvs_parse_function("rvs_add").unwrap();
    assert_eq!(base, "add");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BI(
        "20260418_parse_no_suffix",
        format!("input: rvs_add\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_single_cap() {
    let (base, caps) = rvs_parse_function("rvs_validate_M").unwrap();
    assert_eq!(base, "validate");
    assert_eq!(caps.rvs_len(), 1);
    assert!(caps.rvs_contains(Capability::M));

    rvs_snapshot_BI(
        "20260418_parse_single_cap",
        format!("input: rvs_validate_M\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_multi_cap() {
    let (base, caps) = rvs_parse_function("rvs_write_db_ABI").unwrap();
    assert_eq!(base, "write_db");
    assert!(caps.rvs_contains(Capability::A));
    assert!(caps.rvs_contains(Capability::B));
    assert!(caps.rvs_contains(Capability::I));
    assert_eq!(caps.rvs_len(), 3);

    rvs_snapshot_BI(
        "20260418_parse_multi_cap",
        format!("input: rvs_write_db_ABI\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_no_cap_tricky_name() {
    let (base, caps) = rvs_parse_function("rvs_cache_lookup").unwrap();
    assert_eq!(base, "cache_lookup");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BI(
        "20260418_parse_no_cap_tricky_name",
        format!("input: rvs_cache_lookup\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_two_caps() {
    let (base, caps) = rvs_parse_function("rvs_random_uuid_ST").unwrap();
    assert_eq!(base, "random_uuid");
    assert!(caps.rvs_contains(Capability::S));
    assert!(caps.rvs_contains(Capability::T));
    assert_eq!(caps.rvs_len(), 2);

    rvs_snapshot_BI(
        "20260418_parse_two_caps",
        format!("input: rvs_random_uuid_ST\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_non_rvs() {
    let result = rvs_parse_function("not_rvs_function");
    assert!(result.is_none());

    rvs_snapshot_BI(
        "20260418_parse_non_rvs",
        "input: not_rvs_function\nresult: None\n",
    );
}

#[test]
fn test_20260418_parse_bare_rvs() {
    let (base, caps) = rvs_parse_function("rvs_").unwrap();
    assert_eq!(base, "");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BI(
        "20260418_parse_bare_rvs",
        format!("input: rvs_\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_no_underscore_after_rvs() {
    let (base, caps) = rvs_parse_function("rvs_P").unwrap();
    assert_eq!(base, "P");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BI(
        "20260418_parse_no_underscore_after_rvs",
        format!("input: rvs_P\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_short_base_with_cap() {
    let (base, caps) = rvs_parse_function("rvs_a_B").unwrap();
    assert_eq!(base, "a");
    assert!(caps.rvs_contains(Capability::B));
    assert_eq!(caps.rvs_len(), 1);

    rvs_snapshot_BI(
        "20260418_parse_short_base_with_cap",
        format!("input: rvs_a_B\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_lowercase_suffix() {
    let (base, caps) = rvs_parse_function("rvs_foo_e").unwrap();
    assert_eq!(base, "foo_e");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BI(
        "20260418_parse_lowercase_suffix",
        format!("input: rvs_foo_e\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_all_eight_caps() {
    let (base, caps) = rvs_parse_function("rvs_nuclear_ABIMPSTU").unwrap();
    assert_eq!(base, "nuclear");
    assert_eq!(caps.rvs_len(), 8);

    rvs_snapshot_BI(
        "20260418_parse_all_eight_caps",
        format!("input: rvs_nuclear_ABIMPSTU\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

// ─── 合规检查 ─────────────────────────────────────────────

#[test]
fn test_20260418_compliance_superset_can_call_subset() {
    let caller = CapabilitySet::rvs_from_str("ABI").unwrap();
    let callee = CapabilitySet::rvs_from_str("I").unwrap();
    assert!(caller.rvs_can_call(&callee));

    let missing = caller.rvs_missing_for(&callee);
    assert!(missing.is_empty());

    rvs_snapshot_BI(
        "20260418_compliance_superset_can_call_subset",
        format!("caller: {caller}\ncallee: {callee}\ncan_call: true\nmissing: {{}}\n",),
    );
}

#[test]
fn test_20260418_compliance_subset_cannot_call_superset() {
    let caller = CapabilitySet::rvs_from_str("M").unwrap();
    let callee = CapabilitySet::rvs_from_str("ABI").unwrap();
    assert!(!caller.rvs_can_call(&callee));

    let missing = caller.rvs_missing_for(&callee);
    assert_eq!(missing.len(), 3);
    assert!(missing.contains(&Capability::A));
    assert!(missing.contains(&Capability::B));
    assert!(missing.contains(&Capability::I));

    rvs_snapshot_BI(
        "20260418_compliance_subset_cannot_call_superset",
        format!(
            "caller: {caller}\ncallee: {callee}\ncan_call: false\nmissing: {{{}}}\n",
            rvs_format_caps(&missing),
        ),
    );
}

#[test]
fn test_20260418_compliance_empty_cannot_call_cap() {
    let caller = CapabilitySet::rvs_new();
    let callee = CapabilitySet::rvs_from_str("M").unwrap();
    assert!(!caller.rvs_can_call(&callee));

    let missing = caller.rvs_missing_for(&callee);
    assert_eq!(missing.len(), 1);

    rvs_snapshot_BI(
        "20260418_compliance_empty_cannot_call_cap",
        format!(
            "caller: {caller}\ncallee: {callee}\ncan_call: false\nmissing: {{{}}}\n",
            rvs_format_caps(&missing),
        ),
    );
}

#[test]
fn test_20260418_compliance_cap_can_call_empty() {
    let caller = CapabilitySet::rvs_from_str("M").unwrap();
    let callee = CapabilitySet::rvs_new();
    assert!(caller.rvs_can_call(&callee));

    rvs_snapshot_BI(
        "20260418_compliance_cap_can_call_empty",
        format!("caller: {caller}\ncallee: {callee}\ncan_call: true\n",),
    );
}

#[test]
fn test_20260418_compliance_empty_can_call_empty() {
    let caller = CapabilitySet::rvs_new();
    let callee = CapabilitySet::rvs_new();
    assert!(caller.rvs_can_call(&callee));

    rvs_snapshot_BI(
        "20260418_compliance_empty_can_call_empty",
        format!("caller: {caller}\ncallee: {callee}\ncan_call: true\n",),
    );
}

#[test]
fn test_20260418_compliance_same_set_can_call() {
    let caller = CapabilitySet::rvs_from_str("ABIMPSTU").unwrap();
    let callee = CapabilitySet::rvs_from_str("ABIMPSTU").unwrap();
    assert!(caller.rvs_can_call(&callee));

    rvs_snapshot_BI(
        "20260418_compliance_same_set_can_call",
        format!("caller: {caller}\ncallee: {callee}\ncan_call: true\n",),
    );
}

// ─── syn 集成：萃取函数 ───────────────────────────────────

#[test]
fn test_20260418_syn_parse_single_fn() {
    let source = r#"
fn rvs_add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_add");
    assert!(fns[0].capabilities.rvs_is_empty());
    assert!(fns[0].calls.is_empty());

    rvs_snapshot_BI(
        "20260418_syn_parse_single_fn",
        format!(
            "functions: 1\nname: {}\ncaps: {}\ncalls: {}\n",
            fns[0].name,
            fns[0].capabilities,
            fns[0].calls.len()
        ),
    );
}

#[test]
fn test_20260418_syn_parse_fn_with_calls() {
    let source = r#"
fn rvs_write_db_ABI() {
    rvs_validate_M("42");
    rvs_sort_M(data);
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 1);
    let func = &fns[0];
    assert_eq!(func.name, "rvs_write_db_ABI");
    assert_eq!(func.calls.len(), 2);
    assert_eq!(func.calls[0].name, "rvs_validate_M");
    assert_eq!(func.calls[1].name, "rvs_sort_M");

    rvs_snapshot_BI(
        "20260418_syn_parse_fn_with_calls",
        format!(
            "name: {}\ncaps: {}\ncalls: {}\n  - {}\n  - {}\n",
            func.name,
            func.capabilities,
            func.calls.len(),
            func.calls[0].name,
            func.calls[1].name,
        ),
    );
}

#[test]
fn test_20260418_syn_parse_method_call() {
    let source = r#"
fn rvs_create_order_ABM(cmd: &str) {
    self.repo.rvs_find_by_id_ABI(42);
    self.publisher.rvs_publish_AIS(event);
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    let func = &fns[0];
    assert_eq!(func.calls.len(), 2);
    assert_eq!(func.calls[0].name, "rvs_find_by_id_ABI");
    assert_eq!(func.calls[1].name, "rvs_publish_AIS");

    rvs_snapshot_BI(
        "20260418_syn_parse_method_call",
        format!(
            "name: {}\ncalls: {}\n  - {}\n  - {}\n",
            func.name,
            func.calls.len(),
            func.calls[0].name,
            func.calls[1].name,
        ),
    );
}

#[test]
fn test_20260418_syn_skip_non_rvs() {
    let source = r#"
fn regular_function() {
    other_function();
}

fn rvs_check_M() {
    regular_function();
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_check_M");
    assert_eq!(fns[0].calls.len(), 1);
    assert_eq!(fns[0].calls[0].name, "regular_function");

    rvs_snapshot_BI(
        "20260418_syn_skip_non_rvs",
        format!(
            "functions: {}\nname: {}\ncalls: {}\n  - {}\n",
            fns.len(),
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls[0].name,
        ),
    );
}

#[test]
fn test_20260418_syn_parse_impl_method() {
    let source = r#"
struct Service;

impl Service {
    fn rvs_process_ABI(&self, data: &str) {
        self.rvs_validate(data);
    }

    fn rvs_validate(&self, data: &str) {
        // validation logic
    }

    fn helper(&self) {
        // not an rvs_ function
    }
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 2);

    rvs_snapshot_BI(
        "20260418_syn_parse_impl_method",
        format!(
            "functions: {}\n1: {} caps={} calls={}\n2: {} caps={} calls={}\n",
            fns.len(),
            fns[0].name,
            fns[0].capabilities,
            fns[0].calls.len(),
            fns[1].name,
            fns[1].capabilities,
            fns[1].calls.len(),
        ),
    );
}

#[test]
fn test_20260418_syn_parse_trait_method() {
    let source = r#"
trait Repository {
    fn rvs_find_by_id_ABI(&self, id: u64);
    fn rvs_save_ABI(&self, data: &str);
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 2);
    assert!(fns[0].calls.is_empty());
    assert!(fns[1].calls.is_empty());

    rvs_snapshot_BI(
        "20260418_syn_parse_trait_method",
        format!(
            "functions: {}\n1: {} caps={}\n2: {} caps={}\n",
            fns.len(),
            fns[0].name,
            fns[0].capabilities,
            fns[1].name,
            fns[1].capabilities,
        ),
    );
}

#[test]
fn test_20260418_syn_trait_default_impl() {
    let source = r#"
trait Handler {
    fn rvs_handle_ABM(&self) {
        self.rvs_validate();
    }
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].calls.len(), 1);
    assert_eq!(fns[0].calls[0].name, "rvs_validate");

    rvs_snapshot_BI(
        "20260418_syn_trait_default_impl",
        format!(
            "name: {}\ncalls: {}\n  - {}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls[0].name,
        ),
    );
}

#[test]
fn test_20260418_syn_nested_calls_in_closure() {
    let source = r#"
fn rvs_outer_ABI() {
    let closure = || {
        rvs_inner();
    };
    closure();
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns[0].calls.len(), 2);
    assert_eq!(fns[0].calls[0].name, "rvs_inner");
    assert_eq!(fns[0].calls[1].name, "closure");

    rvs_snapshot_BI(
        "20260418_syn_nested_calls_in_closure",
        format!(
            "name: {}\ncalls: {}\n  - {}\n  - {}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls[0].name,
            fns[0].calls[1].name,
        ),
    );
}

// ─── 端到端：完整 linter ──────────────────────────────────

#[test]
fn test_20260418_linter_compliant_code() {
    let source = r#"
fn rvs_outer_ABI() {
    rvs_inner();
    rvs_pure();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BI("20260418_linter_compliant_code", "violations: 0\n");
}

#[test]
fn test_20260418_linter_single_violation() {
    let source = r#"
fn rvs_inner() {
    rvs_outer_ABI();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);

    let v = &output.violations[0];
    assert_eq!(v.caller, "rvs_inner");
    assert_eq!(v.target, "rvs_outer_ABI");
    assert!(v.missing.contains(&Capability::A));
    assert!(v.missing.contains(&Capability::B));
    assert!(v.missing.contains(&Capability::I));

    rvs_snapshot_BI(
        "20260418_linter_single_violation",
        format!("violations: {}\n{}", output.violations.len(), v),
    );
}

#[test]
fn test_20260418_linter_pure_calls_mutable() {
    let source = r#"
fn rvs_add(a: i32, b: i32) -> i32 {
    rvs_sort_inplace_M(data);
    a + b
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::M));

    rvs_snapshot_BI(
        "20260418_linter_pure_calls_mutable",
        format!(
            "violations: {}\n{}",
            output.violations.len(),
            &output.violations[0]
        ),
    );
}

#[test]
fn test_20260418_linter_mutable_calls_pure_ok() {
    let source = r#"
fn rvs_sort_inplace_M(data: &mut [i32]) {
    rvs_add(1, 2);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BI("20260418_linter_mutable_calls_pure_ok", "violations: 0\n");
}

#[test]
fn test_20260418_linter_multiple_functions() {
    let source = r#"
fn rvs_good_ABI() {
    rvs_helper_B();
}

fn rvs_bad_B() {
    rvs_good_ABI();
}

fn rvs_pure() {
    rvs_bad_B();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 2);

    let violation_text = output
        .violations
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n---\n");

    rvs_snapshot_BI(
        "20260418_linter_multiple_functions",
        format!(
            "violations: {}\n{violation_text}\n",
            output.violations.len()
        ),
    );
}

#[test]
fn test_20260418_linter_method_call_violation() {
    let source = r#"
struct Foo;

impl Foo {
    fn rvs_simple(&self) {
        self.rvs_complex_ABI();
    }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);

    rvs_snapshot_BI(
        "20260418_linter_method_call_violation",
        format!(
            "violations: {}\n{}",
            output.violations.len(),
            &output.violations[0]
        ),
    );
}

#[test]
fn test_20260418_linter_all_caps_compliant() {
    let source = r#"
fn rvs_nuclear_ABIMPSTU() {
    rvs_async_A();
    rvs_block_B();
    rvs_panic_P();
    rvs_io_I();
    rvs_mut_M();
    rvs_sideeffect_S();
    rvs_thread_T();
    rvs_unsafe_U();
    rvs_pure();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BI("20260418_linter_all_caps_compliant", "violations: 0\n");
}

// ─── CapabilitySet::rvs_from_str ──────────────────────────

#[test]
fn test_20260418_capset_from_str_valid() {
    let caps = CapabilitySet::rvs_from_str("ABI").unwrap();
    assert_eq!(caps.rvs_len(), 3);
    assert!(caps.rvs_contains(Capability::A));
    assert!(caps.rvs_contains(Capability::B));
    assert!(caps.rvs_contains(Capability::I));

    rvs_snapshot_BI(
        "20260418_capset_from_str_valid",
        format!("input: ABI\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_capset_from_str_invalid() {
    let result = CapabilitySet::rvs_from_str("ABX");
    assert!(matches!(
        result,
        Err(CapabilityParseError::InvalidLetter('X'))
    ));

    rvs_snapshot_BI(
        "20260418_capset_from_str_invalid",
        "input: ABX\nresult: Err(InvalidLetter('X'))\n",
    );
}

// ─── proc_macro2 span 行号测试 ────────────────────────────

#[test]
fn test_20260418_span_line_numbers() {
    let source = r#"fn rvs_top() {
    rvs_sub();
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns[0].line, 1);
    assert_eq!(fns[0].calls[0].line, 2);

    rvs_snapshot_BI(
        "20260418_span_line_numbers",
        format!(
            "fn line: {}\ncall line: {}\n",
            fns[0].line, fns[0].calls[0].line,
        ),
    );
}

// ─── 汇报功能 ────────────────────────────────────────────

#[test]
fn test_20260418_report_basic() {
    let source = r#"
fn rvs_add(a: i32, b: i32) -> i32 {
    a + b
}

fn rvs_validate_M(s: &str) {
    // validation
}

fn rvs_write_file_BI(path: &str) {
    rvs_validate_M(path);
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    let report = rvs_build_report(&fns);

    assert_eq!(report.total_fn_count, 3);
    assert_eq!(report.pure_fn_count, 1);

    rvs_snapshot_BI("20260418_report_basic", format!("{report}"));
}

#[test]
fn test_20260418_report_empty() {
    let source = r#"fn main() {}"#;
    let fns = rvs_extract_functions(source).unwrap();
    let report = rvs_build_report(&fns);

    assert_eq!(report.total_fn_count, 0);
    assert_eq!(report.total_line_count, 0);

    rvs_snapshot_BI("20260418_report_empty", format!("{report}"));
}

#[test]
fn test_20260418_report_overlapping_caps() {
    let source = r#"
fn rvs_mega_ABIMPSTU() {
    // this function has all 8 capabilities
    let x = 1 + 2;
    let y = x * 3;
    let z = y + x;
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    let report = rvs_build_report(&fns);

    assert_eq!(report.total_fn_count, 1);
    assert_eq!(report.by_capability.len(), 8);

    for cap in report.by_capability.values() {
        assert_eq!(cap.fn_count, 1);
        assert_eq!(cap.line_count, report.total_line_count);
    }

    rvs_snapshot_BI("20260418_report_overlapping_caps", format!("{report}"));
}

// ─── CapsMap 解析与查找 ─────────────────────────────────

#[test]
fn test_20260419_capsmap_parse_basic() {
    let content = "std::fs::read_to_string=BI\nVec::new=\n";
    let cm = CapsMap::rvs_parse(content).unwrap();

    let caps = cm.rvs_lookup("std::fs::read_to_string").unwrap();
    assert!(caps.rvs_contains(Capability::B));
    assert!(caps.rvs_contains(Capability::I));
    assert_eq!(caps.rvs_len(), 2);

    let caps = cm.rvs_lookup("Vec::new").unwrap();
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BI(
        "20260419_capsmap_parse_basic",
        format!(
            "entries: 2\nstd::fs::read_to_string: {}\nVec::new: {}\n",
            cm.rvs_lookup("std::fs::read_to_string").unwrap(),
            cm.rvs_lookup("Vec::new").unwrap(),
        ),
    );
}

#[test]
fn test_20260419_capsmap_parse_comments() {
    let content = "# 这是一个注释\nstd::process::exit=S # 终止进程\n\n";
    let cm = CapsMap::rvs_parse(content).unwrap();
    let caps = cm.rvs_lookup("std::process::exit").unwrap();
    assert!(caps.rvs_contains(Capability::S));

    rvs_snapshot_BI(
        "20260419_capsmap_parse_comments",
        format!("std::process::exit: {}\n", caps),
    );
}

#[test]
fn test_20260419_capsmap_suffix_match() {
    let content = "alloc::vec::Vec::new=\nstd::process::exit=S\n";
    let cm = CapsMap::rvs_parse(content).unwrap();

    let caps = cm.rvs_lookup("Vec::new").unwrap();
    assert!(caps.rvs_is_empty());

    let caps = cm.rvs_lookup("exit").unwrap();
    assert!(caps.rvs_contains(Capability::S));

    assert!(cm.rvs_lookup("nonexistent").is_none());

    rvs_snapshot_BI(
        "20260419_capsmap_suffix_match",
        format!(
            "Vec::new: {}\nexit: {}\nnonexistent: None\n",
            cm.rvs_lookup("Vec::new").unwrap(),
            cm.rvs_lookup("exit").unwrap(),
        ),
    );
}

#[test]
fn test_20260419_capsmap_unknown_non_rvs_warning() {
    let source = r#"
fn rvs_good() {
    unknown_function();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(output.warnings[0].callee, "unknown_function");
    assert_eq!(output.warnings[0].caller, "rvs_good");

    rvs_snapshot_BI(
        "20260419_capsmap_unknown_non_rvs_warning",
        format!(
            "violations: {}\nwarnings: {}\n{}\n",
            output.violations.len(),
            output.warnings.len(),
            output.warnings[0],
        ),
    );
}

#[test]
fn test_20260419_capsmap_known_non_rvs_compliance() {
    let content = "heavy_io=BI\npure_thing=\n";
    let cm = CapsMap::rvs_parse(content).unwrap();

    let source = r#"
fn rvs_simple() {
    pure_thing();
}
"#;
    let output = rvs_check_source(source, "test.rs", &cm).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.warnings.is_empty());

    rvs_snapshot_BI(
        "20260419_capsmap_known_non_rvs_compliance",
        "violations: 0\nwarnings: 0\n",
    );
}

#[test]
fn test_20260419_capsmap_known_non_rvs_violation() {
    let content = "heavy_io=BI\n";
    let cm = CapsMap::rvs_parse(content).unwrap();

    let source = r#"
fn rvs_simple() {
    heavy_io();
}
"#;
    let output = rvs_check_source(source, "test.rs", &cm).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::B));
    assert!(output.violations[0].missing.contains(&Capability::I));

    rvs_snapshot_BI(
        "20260419_capsmap_known_non_rvs_violation",
        format!(
            "violations: {}\n{}\n",
            output.violations.len(),
            output.violations[0]
        ),
    );
}

// ─── 静态变量与 thread_local! 检查 ────────────────────────

#[test]
fn test_20260418_static_ref_requires_S() {
    let source = r#"
static COUNTER: i32 = 0;

fn rvs_read_counter() -> i32 {
    COUNTER
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert_eq!(
        output.violations[0].kind,
        rivus_linter::check::ViolationKind::StaticRef
    );
    assert!(output.violations[0].missing.contains(&Capability::S));
    assert_eq!(output.violations[0].target, "COUNTER");

    rvs_snapshot_BI(
        "20260418_static_ref_requires_S",
        format!(
            "violations: {}\n{}\n",
            output.violations.len(),
            output.violations[0]
        ),
    );
}

#[test]
fn test_20260418_static_ref_with_S_ok() {
    let source = r#"
static COUNTER: i32 = 0;

fn rvs_read_counter_S() -> i32 {
    COUNTER
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BI("20260418_static_ref_with_S_ok", "violations: 0\n");
}

#[test]
fn test_20260418_static_mut_ref_requires_SU() {
    let source = r#"
static mut STATE: i32 = 0;

fn rvs_read_state_U() -> i32 {
    unsafe { STATE }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::S));

    rvs_snapshot_BI(
        "20260418_static_mut_ref_requires_SU",
        format!(
            "violations: {}\n{}\n",
            output.violations.len(),
            output.violations[0]
        ),
    );
}

#[test]
fn test_20260418_static_mut_ref_with_SU_ok() {
    let source = r#"
static mut STATE: i32 = 0;

fn rvs_read_state_SU() -> i32 {
    unsafe { STATE }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BI("20260418_static_mut_ref_with_SU_ok", "violations: 0\n");
}

#[test]
fn test_20260418_thread_local_ref_requires_ST() {
    let source = r#"
thread_local! {
    static TLS: i32 = 42;
}

fn rvs_read_tls() -> i32 {
    TLS.with(|v| *v)
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.len() >= 1);
    let tls_violation = output
        .violations
        .iter()
        .find(|v| v.target == "TLS")
        .unwrap();
    assert!(tls_violation.missing.contains(&Capability::S));
    assert!(tls_violation.missing.contains(&Capability::T));

    rvs_snapshot_BI(
        "20260418_thread_local_ref_requires_ST",
        format!(
            "violations: {}\n{}\n",
            output.violations.len(),
            tls_violation
        ),
    );
}

#[test]
fn test_20260418_thread_local_ref_with_ST_ok() {
    let source = r#"
thread_local! {
    static TLS: i32 = 42;
}

fn rvs_read_tls_ST() -> i32 {
    TLS.with(|v| *v)
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BI("20260418_thread_local_ref_with_ST_ok", "violations: 0\n");
}

#[test]
fn test_20260418_static_in_method_usage() {
    let source = r#"
static CACHE: i32 = 0;

struct Service;

impl Service {
    fn rvs_check_cache(&self) -> i32 {
        CACHE
    }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::S));
    assert_eq!(output.violations[0].target, "CACHE");

    rvs_snapshot_BI(
        "20260418_static_in_method_usage",
        format!(
            "violations: {}\n{}\n",
            output.violations.len(),
            output.violations[0]
        ),
    );
}

// ─── MIR 解析测试 ─────────────────────────────────────────

#[test]
fn test_20260419_mir_extract_rvs_functions() {
    let mir = r#"
fn rvs_add(_1: i32, _2: i32) -> i32 {
    debug x => _1;
    debug y => _2;
    let mut _0: i32;
    let mut _3: (i32, bool);

    bb0: {
        _3 = AddWithOverflow(copy _1, copy _2);
        assert(!move (_3.1: bool), "attempt to compute", copy _1, copy _2) -> [success: bb1, unwind continue];
    }

    bb1: {
        _0 = move (_3.0: i32);
        return;
    }
}

fn rvs_read_BI(_1: &str) -> Result<String, std::io::Error> {
    debug path => _1;
    let mut _0: std::result::Result<std::string::String, std::io::Error>;

    bb0: {
        _0 = std::fs::read_to_string::<&str>(copy _1) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}

fn main() -> () {
    bb0: {
        _1 = rvs_add(const 1_i32, const 2_i32) -> [return: bb1, unwind continue];
    }
}
"#;

    let fns = rivus_linter::mir::rvs_extract_from_mir(mir).unwrap();
    assert_eq!(fns.len(), 2);
    assert_eq!(fns[0].name, "rvs_add");
    assert!(fns[0].capabilities.rvs_is_empty());
    assert!(fns[0].calls.is_empty());

    assert_eq!(fns[1].name, "rvs_read_BI");
    assert!(fns[1].capabilities.rvs_contains(Capability::B));
    assert!(fns[1].capabilities.rvs_contains(Capability::I));
    assert_eq!(fns[1].calls.len(), 1);
    assert_eq!(fns[1].calls[0].name, "std::fs::read_to_string");

    rvs_snapshot_BI(
        "20260419_mir_extract_rvs_functions",
        format!(
            "functions: {}\n1: {} caps={} calls={}\n2: {} caps={} calls={}\n  - {}\n",
            fns.len(),
            fns[0].name,
            fns[0].capabilities,
            fns[0].calls.len(),
            fns[1].name,
            fns[1].capabilities,
            fns[1].calls.len(),
            fns[1].calls[0].name,
        ),
    );
}

#[test]
fn test_20260419_mir_trait_dispatch() {
    let mir = r#"
fn rvs_process_BI(_1: &str) -> Result<Vec<i32>, std::io::Error> {
    let mut _0: std::result::Result<std::vec::Vec<i32>, std::io::Error>;
    let mut _5: std::str::Lines<'_>;
    let mut _4: std::iter::Map<std::str::Lines<'_>, {closure@src/main.rs:13:43: 13:46}>;

    bb0: {
        _5 = core::str::<impl str>::lines(copy _1) -> [return: bb1, unwind: bb6];
    }

    bb1: {
        _4 = <std::str::Lines<'_> as Iterator>::map::<i32, {closure@src/main.rs:13:43: 13:46}>(move _5, const ZeroSized) -> [return: bb2, unwind: bb6];
    }

    bb2: {
        _3 = <Map<std::str::Lines<'_>, {closure@src/main.rs:13:43: 13:46}> as Iterator>::collect::<Vec<i32>>(move _4) -> [return: bb3, unwind: bb6];
    }

    bb3: {
        return;
    }
}
"#;

    let fns = rivus_linter::mir::rvs_extract_from_mir(mir).unwrap();
    assert_eq!(fns.len(), 1);
    let func = &fns[0];
    assert_eq!(func.name, "rvs_process_BI");

    let call_names: Vec<&str> = func.calls.iter().map(|c| c.name.as_str()).collect();
    assert!(
        call_names
            .iter()
            .any(|n| n.contains("Iterator") && n.contains("map"))
    );
    assert!(
        call_names
            .iter()
            .any(|n| n.contains("Iterator") && n.contains("collect"))
    );
    assert!(
        call_names
            .iter()
            .any(|n| n.contains("core::str") && n.contains("lines"))
    );

    rvs_snapshot_BI(
        "20260419_mir_trait_dispatch",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            func.name,
            func.calls.len(),
            func.calls
                .iter()
                .map(|c| format!("  - {}", c.name))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    );
}

#[test]
fn test_20260419_mir_inherent_method() {
    let mir = r#"
fn rvs_init() -> HashMap<String, i32> {
    let mut _0: HashMap<String, i32>;

    bb0: {
        _0 = HashMap::<String, i32>::new() -> [return: bb1, unwind continue];
    }

    bb1: {
        _2 = <HashMap<String, i32> as Clone>::clone(copy _0) -> [return: bb2, unwind continue];
    }

    bb2: {
        return;
    }
}
"#;

    let fns = rivus_linter::mir::rvs_extract_from_mir(mir).unwrap();
    assert_eq!(fns.len(), 1);
    let func = &fns[0];

    let call_names: Vec<&str> = func.calls.iter().map(|c| c.name.as_str()).collect();
    assert!(
        call_names
            .iter()
            .any(|n| n.contains("HashMap") && n.contains("new"))
    );
    assert!(
        call_names
            .iter()
            .any(|n| n.contains("Clone") && n.contains("clone"))
    );

    rvs_snapshot_BI(
        "20260419_mir_inherent_method",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            func.name,
            func.calls.len(),
            func.calls
                .iter()
                .map(|c| format!("  - {}", c.name))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    );
}

#[test]
fn test_20260419_mir_closures_skipped() {
    let mir = r#"
fn rvs_outer_ABI(_1: &str) -> Vec<i32> {
    bb0: {
        _3 = rvs_inner(copy _1) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}

fn rvs_outer_ABI::{closure#0}(_1: &mut {closure@src/main.rs:5:20: 5:23}, _2: &str) -> i32 {
    bb0: {
        _3 = core::str::<impl str>::len(copy _2) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;

    let fns = rivus_linter::mir::rvs_extract_from_mir(mir).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_outer_ABI");
    assert_eq!(fns[0].calls.len(), 2);

    let call_names: Vec<&str> = fns[0].calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.contains(&"rvs_inner"));
    assert!(
        call_names
            .iter()
            .any(|n| n.contains("core::str") && n.contains("len"))
    );

    rvs_snapshot_BI(
        "20260419_mir_closures_skipped",
        format!(
            "functions: {}\nname: {}\ncalls: {}\n{}\n",
            fns.len(),
            fns[0].name,
            fns[0].calls.len(),
            fns[0]
                .calls
                .iter()
                .map(|c| format!("  - {}", c.name))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    );
}

#[test]
fn test_20260419_mir_bare_path_calls() {
    let mir = r#"
fn rvs_process(_1: &str) -> Result<i32, ()> {
    let mut _0: std::result::Result<i32, ()>;

    bb0: {
        _3 = parse_file(copy _1) -> [return: bb1, unwind continue];
    }

    bb1: {
        _4 = format(copy _3) -> [return: bb2, unwind continue];
    }

    bb2: {
        return;
    }
}
"#;

    let fns = rivus_linter::mir::rvs_extract_from_mir(mir).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_process");
    assert_eq!(fns[0].calls.len(), 2);

    let call_names: Vec<&str> = fns[0].calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.contains(&"parse_file"));
    assert!(call_names.contains(&"format"));

    rvs_snapshot_BI(
        "20260419_mir_bare_path_calls",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0]
                .calls
                .iter()
                .map(|c| format!("  - {}", c.name))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    );
}

#[test]
fn test_20260419_mir_unwrap_or_else_fn_ptr() {
    let mir = r#"
fn rvs_harvest_from_expr(_1: &str) -> Harvest {
    let mut _0: Harvest;

    bb0: {
        _23 = Option::<Harvest>::unwrap_or_else::<fn() -> Harvest {Harvest::empty}>(move _24, Harvest::empty) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;

    let fns = rivus_linter::mir::rvs_extract_from_mir(mir).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_harvest_from_expr");
    assert_eq!(fns[0].calls.len(), 1);

    let call_names: Vec<&str> = fns[0].calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.iter().any(|n| n.contains("unwrap_or_else")));

    rvs_snapshot_BI(
        "20260419_mir_unwrap_or_else_fn_ptr",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0]
                .calls
                .iter()
                .map(|c| format!("  - {}", c.name))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    );
}

// ─── MIR 目录级检查测试 ──────────────────────────────────

#[test]
fn test_20260419_mir_check_dir_compliant() {
    let mir = r#"
fn rvs_outer_ABI(_1: &str) -> () {
    bb0: {
        _0 = rvs_inner(copy _1) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}

fn rvs_inner(_1: &str) -> () {
    bb0: {
        return;
    }
}
"#;

    let dir = std::env::temp_dir().join("rivus_test_mir_compliant");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260419_mir_check_dir_compliant",
        "violations: 0\nwarnings: 0\n",
    );
}

#[test]
fn test_20260419_mir_check_dir_violation() {
    let mir = r#"
fn rvs_pure() -> () {
    bb0: {
        _0 = rvs_io_BI() -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}

fn rvs_io_BI() -> () {
    bb0: {
        return;
    }
}
"#;

    let dir = std::env::temp_dir().join("rivus_test_mir_violation");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::B));
    assert!(output.violations[0].missing.contains(&Capability::I));

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260419_mir_check_dir_violation",
        format!(
            "violations: {}\n{}\n",
            output.violations.len(),
            output.violations[0]
        ),
    );
}

#[test]
fn test_20260419_mir_check_dir_with_capsmap() {
    let mir = r#"
fn rvs_simple() -> () {
    bb0: {
        _0 = heavy_io() -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;

    let dir = std::env::temp_dir().join("rivus_test_mir_capsmap");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_parse("heavy_io=BI\n").unwrap();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::B));

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260419_mir_check_dir_with_capsmap",
        format!(
            "violations: {}\n{}\n",
            output.violations.len(),
            output.violations[0]
        ),
    );
}

#[test]
fn test_20260419_mir_check_dir_merge_multiple_files() {
    let mir1 = r#"
fn rvs_outer() -> () {
    bb0: {
        _0 = rvs_helper() -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;

    let mir2 = r#"
fn rvs_outer(_1: i32) -> () {
    bb0: {
        _0 = rvs_other() -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;

    let dir = std::env::temp_dir().join("rivus_test_mir_merge");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("part1.mir"), mir1).unwrap();
    std::fs::write(dir.join("part2.mir"), mir2).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());

    let fns = {
        let sources = rivus_linter::source::rvs_read_mir_sources_BI(&dir).unwrap();
        let mut all = Vec::new();
        for sf in &sources {
            if let Ok(fns) = rivus_linter::mir::rvs_extract_from_mir(&sf.source) {
                all.extend(fns);
            }
        }
        let mut map: std::collections::HashMap<String, rivus_linter::FnDef> =
            std::collections::HashMap::new();
        for f in all {
            match map.entry(f.name.clone()) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    e.get_mut().calls.extend(f.calls);
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(f);
                }
            }
        }
        map.into_values().collect::<Vec<_>>()
    };

    let outer = fns.iter().find(|f| f.name == "rvs_outer").unwrap();
    assert_eq!(outer.calls.len(), 2);

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260419_mir_check_dir_merge",
        format!(
            "violations: {}\nmerged calls for rvs_outer: {}\n  - {}\n  - {}\n",
            output.violations.len(),
            outer.calls.len(),
            outer.calls[0].name,
            outer.calls[1].name,
        ),
    );
}

#[test]
fn test_20260419_mir_check_dir_empty() {
    let dir = std::env::temp_dir().join("rivus_test_mir_empty");
    std::fs::create_dir_all(&dir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.warnings.is_empty());

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260419_mir_check_dir_empty",
        "violations: 0\nwarnings: 0\n",
    );
}

#[test]
fn test_20260419_mir_check_dir_empty_dir() {
    let dir = std::env::temp_dir().join("rivus_test_mir_empty_real");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.warnings.is_empty());

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260419_mir_check_dir_empty_dir",
        "violations: 0\nwarnings: 0\n",
    );
}

#[test]
fn test_20260419_mir_check_dir_unknown_non_rvs_warning() {
    let mir = r#"
fn rvs_do_thing() -> () {
    bb0: {
        _0 = mystery_function() -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;

    let dir = std::env::temp_dir().join("rivus_test_mir_warning");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(output.warnings[0].callee, "mystery_function");

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260419_mir_check_dir_unknown_non_rvs_warning",
        format!(
            "violations: 0\nwarnings: {}\n{}\n",
            output.warnings.len(),
            output.warnings[0]
        ),
    );
}

// ─── debug_assert 参数检查 ─────────────────────────────────

#[test]
fn test_20260419_assert_warning_missing_all() {
    let source = r#"
fn rvs_add(a: i32, b: i32) -> i32 {
    a + b
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert_eq!(output.assert_warnings.len(), 1);
    assert_eq!(output.assert_warnings[0].function, "rvs_add");
    assert_eq!(
        output.assert_warnings[0].missing_params,
        vec!["a".to_string(), "b".to_string()]
    );

    rvs_snapshot_BI(
        "20260419_assert_warning_missing_all",
        format!(
            "assert_warnings: {}\n{}\n",
            output.assert_warnings.len(),
            output.assert_warnings[0],
        ),
    );
}

#[test]
fn test_20260419_assert_warning_partial() {
    let source = r#"
fn rvs_div(a: i32, b: i32) -> i32 {
    debug_assert!(b != 0, "divisor must be non-zero");
    a / b
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert_eq!(output.assert_warnings.len(), 1);
    assert_eq!(
        output.assert_warnings[0].missing_params,
        vec!["a".to_string()]
    );

    rvs_snapshot_BI(
        "20260419_assert_warning_partial",
        format!(
            "assert_warnings: {}\n{}\n",
            output.assert_warnings.len(),
            output.assert_warnings[0],
        ),
    );
}

#[test]
fn test_20260419_assert_warning_all_covered() {
    let source = r#"
fn rvs_div(a: i32, b: i32) -> i32 {
    debug_assert!(a >= 0);
    debug_assert!(b != 0);
    a / b
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.assert_warnings.is_empty());

    rvs_snapshot_BI(
        "20260419_assert_warning_all_covered",
        "assert_warnings: 0\n",
    );
}

#[test]
fn test_20260419_assert_warning_no_params() {
    let source = r#"
fn rvs_pure() -> i32 {
    42
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.assert_warnings.is_empty());

    rvs_snapshot_BI("20260419_assert_warning_no_params", "assert_warnings: 0\n");
}

#[test]
fn test_20260419_assert_warning_self_excluded() {
    let source = r#"
struct Foo;

impl Foo {
    fn rvs_compute(&self, x: i32) -> i32 {
        x * 2
    }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert_eq!(output.assert_warnings.len(), 1);
    assert_eq!(
        output.assert_warnings[0].missing_params,
        vec!["x".to_string()]
    );

    rvs_snapshot_BI(
        "20260419_assert_warning_self_excluded",
        format!(
            "assert_warnings: {}\n{}\n",
            output.assert_warnings.len(),
            output.assert_warnings[0],
        ),
    );
}

#[test]
fn test_20260419_assert_warning_trait_no_default() {
    let source = r#"
trait Repository {
    fn rvs_find_by_id_ABI(&self, id: u64);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.assert_warnings.is_empty());

    rvs_snapshot_BI(
        "20260419_assert_warning_trait_no_default",
        "assert_warnings: 0\n",
    );
}

#[test]
fn test_20260419_assert_warning_debug_assert_eq() {
    let source = r#"
fn rvs_process(data: &str, count: usize) -> bool {
    debug_assert_eq!(count, 0);
    data.is_empty()
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.assert_warnings.is_empty());

    rvs_snapshot_BI(
        "20260419_assert_warning_debug_assert_eq",
        "assert_warnings: 0\n",
    );
}

#[test]
fn test_20260419_assert_warning_non_numeric_exempt() {
    let source = r#"
fn rvs_greet(name: &str) -> String {
    format!("hello {}", name)
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.assert_warnings.is_empty());

    rvs_snapshot_BI(
        "20260419_assert_warning_non_numeric_exempt",
        "assert_warnings: 0\n",
    );
}

#[test]
fn test_20260419_assert_warning_numeric_unasserted() {
    let source = r#"
fn rvs_compute(x: i32, name: &str) -> i32 {
    x + 1
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert_eq!(output.assert_warnings.len(), 1);
    assert_eq!(
        output.assert_warnings[0].missing_params,
        vec!["x".to_string()]
    );

    rvs_snapshot_BI(
        "20260419_assert_warning_numeric_unasserted",
        format!(
            "assert_warnings: {}\n{}\n",
            output.assert_warnings.len(),
            output.assert_warnings[0],
        ),
    );
}

#[test]
fn test_20260419_assert_warning_nested_block() {
    let source = r#"
fn rvs_foo(x: i32) -> i32 {
    if x > 0 {
        debug_assert!(x > 0);
    }
    x
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.assert_warnings.is_empty());

    rvs_snapshot_BI(
        "20260419_assert_warning_nested_block",
        "assert_warnings: 0\n",
    );
}

// ─── A1: mod 块递归提取 ────────────────────────────────────

#[test]
fn test_20260420_mod_recursive_extract() {
    let source = r#"
mod inner {
    fn rvs_helper() {
        rvs_sub();
    }
}

fn rvs_outer_ABI() {
    rvs_helper();
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 2);
    let helper = fns.iter().find(|f| f.name == "rvs_helper").unwrap();
    let outer = fns.iter().find(|f| f.name == "rvs_outer_ABI").unwrap();
    assert_eq!(helper.calls.len(), 1);
    assert_eq!(outer.calls.len(), 1);

    rvs_snapshot_BI(
        "20260420_mod_recursive_extract",
        format!(
            "functions: {}\n1: {} calls={}\n2: {} calls={}\n",
            fns.len(),
            helper.name,
            helper.calls.len(),
            outer.name,
            outer.calls.len(),
        ),
    );
}

#[test]
fn test_20260420_mod_nested_static_ref() {
    let source = r#"
static COUNTER: i32 = 0;

mod inner {
    fn rvs_read_counter() -> i32 {
        COUNTER
    }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::S));

    rvs_snapshot_BI(
        "20260420_mod_nested_static_ref",
        format!(
            "violations: {}\n{}\n",
            output.violations.len(),
            output.violations[0]
        ),
    );
}

// ─── A2: 遍历缺失的 Expr 变体 ──────────────────────────────

#[test]
fn test_20260420_async_block_calls() {
    let source = r#"
fn rvs_outer() {
    let _ = async { rvs_inner(); };
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns[0].calls.len(), 1);
    assert_eq!(fns[0].calls[0].name, "rvs_inner");

    rvs_snapshot_BI(
        "20260420_async_block_calls",
        format!(
            "name: {}\ncalls: {}\n  - {}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls[0].name,
        ),
    );
}

#[test]
fn test_20260420_cast_expr_calls() {
    let source = r#"
fn rvs_outer() {
    let _ = rvs_inner() as i32;
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns[0].calls.len(), 1);
    assert_eq!(fns[0].calls[0].name, "rvs_inner");

    rvs_snapshot_BI(
        "20260420_cast_expr_calls",
        format!(
            "name: {}\ncalls: {}\n  - {}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls[0].name,
        ),
    );
}

#[test]
fn test_20260420_try_block_calls() {
    let source = r#"
fn rvs_outer() {
    let _ = try { rvs_inner(); };
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns[0].calls.len(), 1);
    assert_eq!(fns[0].calls[0].name, "rvs_inner");

    rvs_snapshot_BI(
        "20260420_try_block_calls",
        format!(
            "name: {}\ncalls: {}\n  - {}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls[0].name,
        ),
    );
}

// ─── A3: match arm guard 和 let else ────────────────────────

#[test]
fn test_20260420_match_guard_calls() {
    let source = r#"
fn rvs_outer(x: i32) {
    match x {
        n if rvs_check(n) => {}
        _ => {}
    }
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    let call_names: Vec<&str> = fns[0].calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.contains(&"rvs_check"));

    rvs_snapshot_BI(
        "20260420_match_guard_calls",
        format!(
            "name: {}\ncalls: {}\n  - {}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls[0].name,
        ),
    );
}

#[test]
fn test_20260420_let_else_calls() {
    let source = r#"
fn rvs_outer(x: Option<i32>) {
    let Some(v) = x else { rvs_handle(); return; };
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    let call_names: Vec<&str> = fns[0].calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.contains(&"rvs_handle"));

    rvs_snapshot_BI(
        "20260420_let_else_calls",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0]
                .calls
                .iter()
                .map(|c| format!("  - {}", c.name))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    );
}

#[test]
fn test_20260420_struct_rest_calls() {
    let source = r#"
fn rvs_outer() {
    let _ = Foo { x: rvs_a(), ..rvs_b() };
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    let call_names: Vec<&str> = fns[0].calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.contains(&"rvs_a"));
    assert!(call_names.contains(&"rvs_b"));

    rvs_snapshot_BI(
        "20260420_struct_rest_calls",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0]
                .calls
                .iter()
                .map(|c| format!("  - {}", c.name))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    );
}

// ─── B1: unsafe 块/函数 → 应有 U 检测 ──────────────────────

#[test]
fn test_20260420_infer_unsafe_block_missing_U() {
    let source = r#"
fn rvs_read_raw() -> i32 {
    unsafe { std::ptr::read(std::ptr::null()) }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert_eq!(output.inference_warnings.len(), 1);
    assert_eq!(
        output.inference_warnings[0].kind,
        InferenceKind::MissingUnsafe
    );

    rvs_snapshot_BI(
        "20260420_infer_unsafe_block_missing_U",
        format!(
            "inference_warnings: {}\n{}\n",
            output.inference_warnings.len(),
            output.inference_warnings[0]
        ),
    );
}

#[test]
fn test_20260420_infer_unsafe_block_with_U_ok() {
    let source = r#"
fn rvs_read_raw_U() -> i32 {
    unsafe { std::ptr::read(std::ptr::null()) }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.inference_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_infer_unsafe_block_with_U_ok",
        "inference_warnings: 0\n",
    );
}

#[test]
fn test_20260420_infer_unsafe_fn_missing_U() {
    let source = r#"
unsafe fn rvs_dangerous() {
    std::ptr::read(std::ptr::null());
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingUnsafe)
    );

    rvs_snapshot_BI(
        "20260420_infer_unsafe_fn_missing_U",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

// ─── B2: async fn → 应有 A 检测 ─────────────────────────────

#[test]
fn test_20260420_infer_async_fn_missing_A() {
    let source = r#"
async fn rvs_fetch() {
    rvs_inner();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingAsync)
    );

    rvs_snapshot_BI(
        "20260420_infer_async_fn_missing_A",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_infer_async_fn_with_A_ok() {
    let source = r#"
async fn rvs_fetch_A() {
    rvs_inner();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingAsync)
    );

    rvs_snapshot_BI(
        "20260420_infer_async_fn_with_A_ok",
        "inference_warnings: 0\n",
    );
}

// ─── B3/B4: &mut 参数/self → 应有 M 检测 ────────────────────

#[test]
fn test_20260420_infer_mut_param_missing_M() {
    let source = r#"
fn rvs_update(data: &mut i32) {
    *data = 42;
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingMutable)
    );

    rvs_snapshot_BI(
        "20260420_infer_mut_param_missing_M",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_infer_mut_self_missing_M() {
    let source = r#"
struct Foo;
impl Foo {
    fn rvs_modify(&mut self) {
        self.value = 42;
    }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingMutable)
    );

    rvs_snapshot_BI(
        "20260420_infer_mut_self_missing_M",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_infer_mut_with_M_ok() {
    let source = r#"
fn rvs_update_M(data: &mut i32) {
    *data = 42;
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingMutable)
    );

    rvs_snapshot_BI("20260420_infer_mut_with_M_ok", "inference_warnings: 0\n");
}

// ─── B6: static/thread_local 读取 → 应有 S 检测 ──────────

#[test]
fn test_20260420_infer_static_read_missing_S() {
    let source = r#"
static COUNTER: i32 = 0;

fn rvs_read_counter() -> i32 {
    COUNTER
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingSideEffect)
    );

    rvs_snapshot_BI(
        "20260420_infer_static_read_missing_S",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_infer_thread_local_read_missing_S() {
    let source = r#"
thread_local! {
    static TLS: i32 = 42;
}

fn rvs_read_tls_T() -> i32 {
    TLS.with(|v| *v)
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingSideEffect)
    );

    rvs_snapshot_BI(
        "20260420_infer_thread_local_read_missing_S",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

// ─── B8: panic 宏 → 应有 P 检测 ─────────────────────────────

#[test]
fn test_20260420_infer_panic_macro_missing_P() {
    let source = r#"
fn rvs_bail(msg: &str) {
    panic!("{}", msg);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingPanic)
    );

    rvs_snapshot_BI(
        "20260420_infer_panic_macro_missing_P",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_infer_assert_macro_missing_P() {
    let source = r#"
fn rvs_check_valid(x: i32) {
    assert!(x > 0);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingPanic)
    );

    rvs_snapshot_BI(
        "20260420_infer_assert_macro_missing_P",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_infer_debug_assert_no_P() {
    let source = r#"
fn rvs_check_valid(x: i32) {
    debug_assert!(x > 0);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingPanic)
    );

    rvs_snapshot_BI(
        "20260420_infer_debug_assert_no_P",
        "inference_warnings: 0\n",
    );
}

#[test]
fn test_20260420_infer_panic_with_P_ok() {
    let source = r#"
fn rvs_bail_P(msg: &str) {
    panic!("{}", msg);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingPanic)
    );

    rvs_snapshot_BI("20260420_infer_panic_with_P_ok", "inference_warnings: 0\n");
}

#[test]
fn test_20260420_infer_unwrap_missing_P() {
    let source = r#"
fn rvs_get_value(x: Option<i32>) -> i32 {
    x.unwrap()
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingPanic)
    );

    rvs_snapshot_BI(
        "20260420_infer_unwrap_missing_P",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_infer_expect_missing_P() {
    let source = r#"
fn rvs_get_value(x: Result<i32, String>) -> i32 {
    x.expect("must succeed")
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingPanic)
    );

    rvs_snapshot_BI(
        "20260420_infer_expect_missing_P",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_infer_unwrap_with_P_ok() {
    let source = r#"
fn rvs_get_value_P(x: Option<i32>) -> i32 {
    x.unwrap()
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingPanic)
    );

    rvs_snapshot_BI("20260420_infer_unwrap_with_P_ok", "inference_warnings: 0\n");
}

#[test]
fn test_20260420_infer_expect_with_P_ok() {
    let source = r#"
fn rvs_get_value_P(x: Result<i32, String>) -> i32 {
    x.expect("must succeed")
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingPanic)
    );

    rvs_snapshot_BI("20260420_infer_expect_with_P_ok", "inference_warnings: 0\n");
}

// ─── C4: 能力字母非字母序检查 ────────────────────────────────

#[test]
fn test_20260420_suffix_non_alphabetical() {
    let source = r#"
fn rvs_foo_BA() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::NonAlphabeticalSuffix)
    );

    rvs_snapshot_BI(
        "20260420_suffix_non_alphabetical",
        format!(
            "inference_warnings: {}\n{}\n",
            output.inference_warnings.len(),
            output.inference_warnings[0]
        ),
    );
}

#[test]
fn test_20260420_suffix_alphabetical_ok() {
    let source = r#"
fn rvs_foo_AB() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::NonAlphabeticalSuffix)
    );

    rvs_snapshot_BI("20260420_suffix_alphabetical_ok", "inference_warnings: 0\n");
}

// ─── C5: 重复能力字母检查 ────────────────────────────────────

#[test]
fn test_20260420_suffix_duplicate_letter() {
    let source = r#"
fn rvs_foo_PP() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::DuplicateSuffixLetter)
    );

    rvs_snapshot_BI(
        "20260420_suffix_duplicate_letter",
        format!(
            "inference_warnings: {}\n{}\n",
            output.inference_warnings.len(),
            output.inference_warnings[0]
        ),
    );
}

#[test]
fn test_20260420_suffix_no_duplicate_ok() {
    let source = r#"
fn rvs_foo_P() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::DuplicateSuffixLetter)
    );

    rvs_snapshot_BI("20260420_suffix_no_duplicate_ok", "inference_warnings: 0\n");
}

// ─── rvs_extract_raw_suffix ──────────────────────────────────

#[test]
fn test_20260420_extract_raw_suffix() {
    let cases = vec![
        ("rvs_foo_AB", "AB"),
        ("rvs_foo", ""),
        ("rvs_foo_BA", "BA"),
        ("rvs_foo_PP", "PP"),
        ("rvs_nuclear_ABIMPSTU", "ABIMPSTU"),
        ("not_rvs", ""),
    ];
    let mut result = String::new();
    for (name, expected) in &cases {
        let suffix = rivus_linter::capability::rvs_extract_raw_suffix(name);
        assert_eq!(&suffix, expected, "for {name}");
        result.push_str(&format!("{name} -> \"{suffix}\"\n"));
    }

    rvs_snapshot_BI("20260420_extract_raw_suffix", result);
}

// ─── D1: thread_local 引用缺 T 推断 ─────────────────────────

#[test]
fn test_20260420_infer_thread_local_has_S_missing_T() {
    let source = r#"
thread_local! {
    static TLS: i32 = 42;
}

fn rvs_read_tls_S() -> i32 {
    TLS.with(|v| *v)
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingThreadLocal),
        "should produce MissingThreadLocal hint for thread_local ref without T"
    );

    rvs_snapshot_BI(
        "20260420_infer_thread_local_has_S_missing_T",
        format!(
            "violations: {}\ninference_warnings: {}\n",
            output.violations.len(),
            output.inference_warnings.len()
        ),
    );
}

#[test]
fn test_20260420_infer_thread_local_with_ST_ok() {
    let source = r#"
thread_local! {
    static TLS: i32 = 42;
}

fn rvs_read_tls_ST() -> i32 {
    TLS.with(|v| *v)
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingThreadLocal)
    );

    rvs_snapshot_BI(
        "20260420_infer_thread_local_with_ST_ok",
        "inference_warnings: 0\n",
    );
}

// ─── D2: MIR &mut 参数推断 ──────────────────────────────────

#[test]
fn test_20260420_mir_infer_mut_param_missing_M() {
    let mir = r#"
fn rvs_update(_1: &mut i32) -> () {
    bb0: {
        return;
    }
}
"#;
    let dir = std::env::temp_dir().join("rivus_test_mir_infer_mut");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingMutable),
        "MIR should produce MissingMutable hint for &mut param without M"
    );

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260420_mir_infer_mut_param_missing_M",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_mir_infer_mut_param_with_M_ok() {
    let mir = r#"
fn rvs_update_M(_1: &mut i32) -> () {
    bb0: {
        return;
    }
}
"#;
    let dir = std::env::temp_dir().join("rivus_test_mir_infer_mut_ok");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingMutable)
    );

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260420_mir_infer_mut_param_with_M_ok",
        "inference_warnings: 0\n",
    );
}

// ─── D3: MIR panic 调用推断 ──────────────────────────────────

#[test]
fn test_20260420_mir_infer_panic_missing_P() {
    let mir = r#"
fn rvs_bail(_1: &str) -> () {
    let mut _0: ();

    bb0: {
        _0 = core::panicking::panic_fmt(move _2, move _3) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;
    let dir = std::env::temp_dir().join("rivus_test_mir_infer_panic");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .any(|w| w.kind == InferenceKind::MissingPanic),
        "MIR should produce MissingPanic hint for panic call without P"
    );

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260420_mir_infer_panic_missing_P",
        format!("inference_warnings: {}\n", output.inference_warnings.len()),
    );
}

#[test]
fn test_20260420_mir_panic_no_false_positive_on_string_arg() {
    let mir = r#"
fn rvs_scan_mir_has_panic(_1: &MirFnDef) -> bool {
    let mut _0: bool;

    bb0: {
        _1 = core::str::<impl str>::contains::<&str>(copy _0, const "panicking::panic") -> [return: bb1, unwind continue];
    }

    bb1: {
        _0 = const false;
        return;
    }
}
"#;
    let dir = std::env::temp_dir().join("rivus_test_mir_panic_string_arg");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingPanic),
        "MIR should NOT produce MissingPanic for panicking::panic in string constant args"
    );

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260420_mir_panic_no_false_positive_on_string_arg",
        "inference_warnings: 0\n",
    );
}

#[test]
fn test_20260420_mir_infer_panic_with_P_ok() {
    let mir = r#"
fn rvs_bail_P(_1: &str) -> () {
    let mut _0: ();

    bb0: {
        _0 = core::panicking::panic_fmt(move _2, move _3) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;
    let dir = std::env::temp_dir().join("rivus_test_mir_infer_panic_ok");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingPanic)
    );

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260420_mir_infer_panic_with_P_ok",
        "inference_warnings: 0\n",
    );
}

#[test]
fn test_20260420_mir_closure_mut_self_no_false_positive() {
    let mir = r#"
fn rvs_is_subset_of(_1: &CapabilitySet, _2: &CapabilitySet) -> bool {
    let mut _0: bool;

    bb0: {
        _0 = const true;
        return;
    }
}

fn rvs_is_subset_of::{closure#0}(_1: &mut {closure@rvs_is_subset_of}, _2: &Capability) -> bool {
    let mut _0: bool;

    bb0: {
        _0 = const true;
        return;
    }
}
"#;
    let dir = std::env::temp_dir().join("rivus_test_mir_closure_mut_self");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BIM(&dir, &cm).unwrap();
    assert!(
        output
            .inference_warnings
            .iter()
            .all(|w| w.kind != InferenceKind::MissingMutable),
        "MIR should NOT produce MissingMutable for FnMut closure &mut self"
    );

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260420_mir_closure_mut_self_no_false_positive",
        "inference_warnings: 0\n",
    );
}

// ─── E1: #[cfg(test)] 和 #[test] 过滤 ──────────────────────

#[test]
fn test_20260420_extract_skips_cfg_test_mod() {
    let source = r#"
#[cfg(test)]
mod tests {
    fn rvs_helper() {
        rvs_inner();
    }
}

fn rvs_outer() {
    rvs_inner();
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_outer");

    rvs_snapshot_BI(
        "20260420_extract_skips_cfg_test_mod",
        format!("functions: {}\n0: {}\n", fns.len(), fns[0].name),
    );
}

#[test]
fn test_20260420_extract_skips_test_fn() {
    let source = r#"
#[test]
fn rvs_check_something() {
    rvs_helper();
}

fn rvs_helper() {}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_helper");

    rvs_snapshot_BI(
        "20260420_extract_skips_test_fn",
        format!("functions: {}\n0: {}\n", fns.len(), fns[0].name),
    );
}

#[test]
fn test_20260420_extract_skips_test_method_in_impl() {
    let source = r#"
struct Foo;

impl Foo {
    #[test]
    fn rvs_check_behavior(&self) {
        rvs_inner();
    }

    fn rvs_inner(&self) {}
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_inner");

    rvs_snapshot_BI(
        "20260420_extract_skips_test_method_in_impl",
        format!("functions: {}\n0: {}\n", fns.len(), fns[0].name),
    );
}

// ─── E2: #[allow(dead_code)] / #[allow(unused)] 过滤 ─────

#[test]
fn test_20260420_extract_flags_allow_dead_code() {
    let source = r#"
#[allow(dead_code)]
fn rvs_never_called() {
    rvs_helper();
}

fn rvs_helper() {}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 2);
    let never_called = fns.iter().find(|f| f.name == "rvs_never_called").unwrap();
    let helper = fns.iter().find(|f| f.name == "rvs_helper").unwrap();
    assert!(never_called.allows_dead_code);
    assert!(!helper.allows_dead_code);

    rvs_snapshot_BI(
        "20260420_extract_flags_allow_dead_code",
        format!(
            "functions: {}\n0: {} allows_dead_code={}\n1: {} allows_dead_code={}\n",
            fns.len(),
            never_called.name,
            never_called.allows_dead_code,
            helper.name,
            helper.allows_dead_code,
        ),
    );
}

#[test]
fn test_20260420_extract_flags_allow_unused() {
    let source = r#"
#[allow(unused)]
fn rvs_dead() {
    rvs_real();
}

fn rvs_real() {}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 2);
    let dead = fns.iter().find(|f| f.name == "rvs_dead").unwrap();
    let real = fns.iter().find(|f| f.name == "rvs_real").unwrap();
    assert!(dead.allows_dead_code);
    assert!(!real.allows_dead_code);

    rvs_snapshot_BI(
        "20260420_extract_flags_allow_unused",
        format!(
            "functions: {}\n0: {} allows_dead_code={}\n1: {} allows_dead_code={}\n",
            fns.len(),
            dead.name,
            dead.allows_dead_code,
            real.name,
            real.allows_dead_code,
        ),
    );
}

#[test]
fn test_20260420_extract_flags_allow_dead_code_method() {
    let source = r#"
struct Foo;

impl Foo {
    #[allow(dead_code)]
    fn rvs_orphan(&self) {}

    fn rvs_used(&self) {}
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    assert_eq!(fns.len(), 2);
    let orphan = fns.iter().find(|f| f.name == "rvs_orphan").unwrap();
    let used = fns.iter().find(|f| f.name == "rvs_used").unwrap();
    assert!(orphan.allows_dead_code);
    assert!(!used.allows_dead_code);

    rvs_snapshot_BI(
        "20260420_extract_flags_allow_dead_code_method",
        format!(
            "functions: {}\n0: {} allows_dead_code={}\n1: {} allows_dead_code={}\n",
            fns.len(),
            orphan.name,
            orphan.allows_dead_code,
            used.name,
            used.allows_dead_code,
        ),
    );
}

// ─── E3: DeadCodeWarning 检查输出 ────────────────────────

#[test]
fn test_20260420_dead_code_warning_emitted() {
    let source = r#"
#[allow(dead_code)]
fn rvs_never_called(x: i32) -> i32 {
    x + 1
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.dead_code_warnings.len(), 1);
    assert_eq!(output.dead_code_warnings[0].function, "rvs_never_called");

    rvs_snapshot_BI(
        "20260420_dead_code_warning_emitted",
        format!(
            "dead_code_warnings: {}\n{}\n",
            output.dead_code_warnings.len(),
            output.dead_code_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_dead_code_warning_unused() {
    let source = r#"
#[allow(unused)]
fn rvs_dead_fn() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.dead_code_warnings.len(), 1);
    assert_eq!(output.dead_code_warnings[0].function, "rvs_dead_fn");

    rvs_snapshot_BI(
        "20260420_dead_code_warning_unused",
        format!(
            "dead_code_warnings: {}\n{}\n",
            output.dead_code_warnings.len(),
            output.dead_code_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_no_dead_code_warning_without_attr() {
    let source = r#"
fn rvs_normal() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.dead_code_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_no_dead_code_warning_without_attr",
        "dead_code_warnings: 0\n",
    );
}

// ─── E4: Report excludes allows_dead_code functions ──────

#[test]
fn test_20260420_report_excludes_dead_code() {
    let source = r#"
fn rvs_add(a: i32, b: i32) -> i32 {
    a + b
}

#[allow(dead_code)]
fn rvs_dead_M(s: &str) {
    // dead code
}

fn rvs_write_file_BI(path: &str) {
    rvs_add(1, 2);
}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    let report = rvs_build_report(&fns);

    assert_eq!(report.total_fn_count, 2);
    assert_eq!(report.pure_fn_count, 1);

    rvs_snapshot_BI("20260420_report_excludes_dead_code", format!("{report}"));
}

#[test]
fn test_20260420_report_excludes_unused() {
    let source = r#"
#[allow(unused)]
fn rvs_dead_pure() -> i32 {
    42
}

fn rvs_real_M() {}
"#;
    let fns = rvs_extract_functions(source).unwrap();
    let report = rvs_build_report(&fns);

    assert_eq!(report.total_fn_count, 1);
    assert_eq!(report.pure_fn_count, 0);

    rvs_snapshot_BI("20260420_report_excludes_unused", format!("{report}"));
}

// ─── F1: #[allow(non_snake_case)] 标注检查 ───────────────────

#[test]
fn test_20260420_allow_missing_for_cap_fn() {
    let source = r#"
fn rvs_write_db_ABI() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.missing_allow_warnings.len(), 1);
    assert_eq!(
        output.missing_allow_warnings[0].function,
        "rvs_write_db_ABI"
    );

    rvs_snapshot_BI(
        "20260420_allow_missing_for_cap_fn",
        format!(
            "missing_allow_warnings: {}\n{}\n",
            output.missing_allow_warnings.len(),
            output.missing_allow_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_allow_present_on_fn_ok() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_write_db_ABI() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_allow_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_allow_present_on_fn_ok",
        "missing_allow_warnings: 0\n",
    );
}

#[test]
fn test_20260420_allow_no_caps_no_warning() {
    let source = r#"
fn rvs_pure() {}
fn rvs_helper() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_allow_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_allow_no_caps_no_warning",
        "missing_allow_warnings: 0\n",
    );
}

#[test]
fn test_20260420_allow_file_level_ok() {
    let source = r#"#![allow(non_snake_case)]

fn rvs_write_db_ABI() {}
fn rvs_fetch_AI() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_allow_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_allow_file_level_ok",
        "missing_allow_warnings: 0\n",
    );
}

#[test]
fn test_20260420_allow_on_impl_block_ok() {
    let source = r#"
struct Svc;

#[allow(non_snake_case)]
impl Svc {
    fn rvs_run_AI(&self) {}
    fn rvs_stop_M(&mut self) {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_allow_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_allow_on_impl_block_ok",
        "missing_allow_warnings: 0\n",
    );
}

#[test]
fn test_20260420_allow_missing_on_impl_method() {
    let source = r#"
struct Svc;

impl Svc {
    fn rvs_run_AI(&self) {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.missing_allow_warnings.len(), 1);
    assert_eq!(output.missing_allow_warnings[0].function, "rvs_run_AI");

    rvs_snapshot_BI(
        "20260420_allow_missing_on_impl_method",
        format!(
            "missing_allow_warnings: {}\n{}\n",
            output.missing_allow_warnings.len(),
            output.missing_allow_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_allow_mod_level_ok() {
    let source = r#"
#[allow(non_snake_case)]
mod inner {
    fn rvs_deep_BI() {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_allow_warnings.is_empty());

    rvs_snapshot_BI("20260420_allow_mod_level_ok", "missing_allow_warnings: 0\n");
}

// ─── F2: #[test] 命名格式检查 ───────────────────────────────

#[test]
fn test_20260420_testname_invalid_no_date() {
    let source = r#"
#[test]
fn test_something_obvious() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.test_name_warnings.len(), 1);
    assert_eq!(
        output.test_name_warnings[0].function,
        "test_something_obvious"
    );

    rvs_snapshot_BI(
        "20260420_testname_invalid_no_date",
        format!(
            "test_name_warnings: {}\n{}\n",
            output.test_name_warnings.len(),
            output.test_name_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_testname_valid_ok() {
    let source = r#"
#[test]
fn test_20260420_valid_shape() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.test_name_warnings.is_empty());

    rvs_snapshot_BI("20260420_testname_valid_ok", "test_name_warnings: 0\n");
}

#[test]
fn test_20260420_testname_wrong_date_length() {
    let source = r#"
#[test]
fn test_202604_short_date() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.test_name_warnings.len(), 1);
    assert_eq!(
        output.test_name_warnings[0].function,
        "test_202604_short_date"
    );

    rvs_snapshot_BI(
        "20260420_testname_wrong_date_length",
        format!(
            "test_name_warnings: {}\n{}\n",
            output.test_name_warnings.len(),
            output.test_name_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_testname_no_test_prefix() {
    let source = r#"
#[test]
fn check_20260420_something() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.test_name_warnings.len(), 1);
    assert_eq!(
        output.test_name_warnings[0].function,
        "check_20260420_something"
    );

    rvs_snapshot_BI(
        "20260420_testname_no_test_prefix",
        format!(
            "test_name_warnings: {}\n{}\n",
            output.test_name_warnings.len(),
            output.test_name_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_testname_inside_cfg_test_mod() {
    let source = r#"
#[cfg(test)]
mod tests {
    #[test]
    fn stale_name() {}

    #[test]
    fn test_20260420_good_name() {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.test_name_warnings.len(), 1);
    assert_eq!(output.test_name_warnings[0].function, "stale_name");

    rvs_snapshot_BI(
        "20260420_testname_inside_cfg_test_mod",
        format!(
            "test_name_warnings: {}\n{}\n",
            output.test_name_warnings.len(),
            output.test_name_warnings[0],
        ),
    );
}

// ─── F3: #[test] 命名唯一性检查 ──────────────────────────────

#[test]
fn test_20260420_testname_duplicate_same_file() {
    let source = r#"
#[test]
fn test_20260420_dup_case() {}

mod inner {
    #[test]
    fn test_20260420_dup_case() {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.duplicate_test_warnings.len(), 1);
    assert_eq!(
        output.duplicate_test_warnings[0].name,
        "test_20260420_dup_case"
    );
    assert_eq!(output.duplicate_test_warnings[0].occurrences.len(), 2);

    rvs_snapshot_BI(
        "20260420_testname_duplicate_same_file",
        format!(
            "duplicate_test_warnings: {}\n{}\n",
            output.duplicate_test_warnings.len(),
            output.duplicate_test_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_testname_no_duplicate_ok() {
    let source = r#"
#[test]
fn test_20260420_first_case() {}

#[test]
fn test_20260420_second_case() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.duplicate_test_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_testname_no_duplicate_ok",
        "duplicate_test_warnings: 0\n",
    );
}

#[test]
fn test_20260420_testname_duplicate_cross_file() {
    let dir = std::env::temp_dir().join("rivus_test_dup_cross");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.rs"),
        "#[test]\nfn test_20260420_shared_name() {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("b.rs"),
        "#[test]\nfn test_20260420_shared_name() {}\n",
    )
    .unwrap();

    let output = rivus_linter::rvs_check_path_BI(&dir, &CapsMap::rvs_new()).unwrap();
    let dup = output
        .duplicate_test_warnings
        .iter()
        .find(|d| d.name == "test_20260420_shared_name")
        .unwrap();
    assert_eq!(dup.occurrences.len(), 2);

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BI(
        "20260420_testname_duplicate_cross_file",
        format!(
            "duplicate name: {}\noccurrences: {}\n",
            dup.name,
            dup.occurrences.len()
        ),
    );
}

#[test]
fn test_20260420_testname_triple_dup_same_file() {
    let source = r#"
#[test]
fn test_20260420_triplicate() {}

mod a {
    #[test]
    fn test_20260420_triplicate() {}
}

mod b {
    #[test]
    fn test_20260420_triplicate() {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.duplicate_test_warnings.len(), 1);
    assert_eq!(output.duplicate_test_warnings[0].occurrences.len(), 3);

    rvs_snapshot_BI(
        "20260420_testname_triple_dup_same_file",
        format!(
            "duplicate name: {}\noccurrences: {}\n",
            output.duplicate_test_warnings[0].name,
            output.duplicate_test_warnings[0].occurrences.len(),
        ),
    );
}

// ─── G1: 被禁导入检查 ─────────────────────────────────────────

#[test]
fn test_20260420_banned_import_anyhow() {
    let source = r#"
use anyhow::Result;
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.banned_import_warnings.len(), 1);
    assert_eq!(output.banned_import_warnings[0].crate_name, "anyhow");

    rvs_snapshot_BI(
        "20260420_banned_import_anyhow",
        format!(
            "banned_import_warnings: {}\n{}\n",
            output.banned_import_warnings.len(),
            output.banned_import_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_banned_import_eyre() {
    let source = r#"
use eyre::Report;
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.banned_import_warnings.len(), 1);
    assert_eq!(output.banned_import_warnings[0].crate_name, "eyre");

    rvs_snapshot_BI(
        "20260420_banned_import_eyre",
        format!(
            "banned_import_warnings: {}\n{}\n",
            output.banned_import_warnings.len(),
            output.banned_import_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_banned_import_color_eyre() {
    let source = r#"
use color_eyre::eyre::Result;
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.banned_import_warnings.len(), 1);
    assert_eq!(output.banned_import_warnings[0].crate_name, "color_eyre");

    rvs_snapshot_BI(
        "20260420_banned_import_color_eyre",
        format!(
            "banned_import_warnings: {}\n{}\n",
            output.banned_import_warnings.len(),
            output.banned_import_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_banned_import_allowed_ok() {
    let source = r#"
use std::collections::HashMap;
use thiserror::Error;
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.banned_import_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_banned_import_allowed_ok",
        "banned_import_warnings: 0\n",
    );
}

#[test]
fn test_20260420_banned_import_in_mod() {
    let source = r#"
mod inner {
    use anyhow::Result;
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.banned_import_warnings.len(), 1);

    rvs_snapshot_BI(
        "20260420_banned_import_in_mod",
        format!(
            "banned_import_warnings: {}\n{}\n",
            output.banned_import_warnings.len(),
            output.banned_import_warnings[0],
        ),
    );
}

// ─── 函数命名检查（缺少 rvs_ 前缀）──────────────────────────

#[test]
fn test_20260421_non_rvs_fn_missing_prefix() {
    let source = r#"
fn bad_fn() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.non_rvs_fn_warnings.len(), 1);
    assert_eq!(output.non_rvs_fn_warnings[0].function, "bad_fn");

    rvs_snapshot_BI(
        "20260421_non_rvs_fn_missing_prefix",
        format!(
            "non_rvs_fn_warnings: {}\n{}\n",
            output.non_rvs_fn_warnings.len(),
            output.non_rvs_fn_warnings[0],
        ),
    );
}

#[test]
fn test_20260421_fn_with_rvs_ok() {
    let source = r#"
fn rvs_good_fn_P() { panic!("test"); }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.non_rvs_fn_warnings.is_empty());

    rvs_snapshot_BI("20260421_fn_with_rvs_ok", "non_rvs_fn_warnings: 0\n");
}

#[test]
fn test_20260421_non_rvs_fn_in_impl() {
    let source = r#"
struct Svc;

impl Svc {
    fn bad_method(&self) {}
    fn rvs_good_method_M(&mut self) {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.non_rvs_fn_warnings.len(), 1);
    assert_eq!(output.non_rvs_fn_warnings[0].function, "bad_method");

    rvs_snapshot_BI(
        "20260421_non_rvs_fn_in_impl",
        format!(
            "non_rvs_fn_warnings: {}\n{}\n",
            output.non_rvs_fn_warnings.len(),
            output.non_rvs_fn_warnings[0],
        ),
    );
}

#[test]
fn test_20260421_non_rvs_fn_in_mod() {
    let source = r#"
mod inner {
    fn bad_fn() {}
    fn rvs_good_fn() {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.non_rvs_fn_warnings.len(), 1);
    assert_eq!(output.non_rvs_fn_warnings[0].function, "bad_fn");

    rvs_snapshot_BI(
        "20260421_non_rvs_fn_in_mod",
        format!(
            "non_rvs_fn_warnings: {}\n{}\n",
            output.non_rvs_fn_warnings.len(),
            output.non_rvs_fn_warnings[0],
        ),
    );
}

#[test]
fn test_20260421_pub_fn_missing_rvs_prefix() {
    let source = r#"
pub fn bad_pub_fn() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.non_rvs_fn_warnings.len(), 1);
    assert_eq!(output.non_rvs_fn_warnings[0].function, "bad_pub_fn");

    rvs_snapshot_BI(
        "20260421_pub_fn_missing_rvs_prefix",
        format!(
            "non_rvs_fn_warnings: {}\n{}\n",
            output.non_rvs_fn_warnings.len(),
            output.non_rvs_fn_warnings[0],
        ),
    );
}

// ─── 公开 API 文档注释检查 ────────────────────────────────

#[test]
fn test_20260420_pub_fn_missing_doc() {
    let source = r#"
pub fn rvs_foo_bar() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.missing_doc_warnings.len(), 1);
    assert_eq!(output.missing_doc_warnings[0].item, "rvs_foo_bar");

    rvs_snapshot_BI(
        "20260420_pub_fn_missing_doc",
        format!(
            "missing_doc_warnings: {}\n{}\n",
            output.missing_doc_warnings.len(),
            output.missing_doc_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_pub_fn_with_doc_ok() {
    let source = r#"
/// 一个带文档的 pub 函数。
pub fn rvs_foo_bar() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_doc_warnings.is_empty());

    rvs_snapshot_BI("20260420_pub_fn_with_doc_ok", "missing_doc_warnings: 0\n");
}

#[test]
fn test_20260420_pub_method_missing_doc() {
    let source = r#"
pub struct Svc;
impl Svc {
    pub fn rvs_method() {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.missing_doc_warnings.len(), 1);
    assert_eq!(output.missing_doc_warnings[0].item, "rvs_method");

    rvs_snapshot_BI(
        "20260420_pub_method_missing_doc",
        format!(
            "missing_doc_warnings: {}\n{}\n",
            output.missing_doc_warnings.len(),
            output.missing_doc_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_pub_method_with_doc_ok() {
    let source = r#"
pub struct Svc;
impl Svc {
    /// 方法的文档。
    pub fn rvs_method() {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_doc_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_pub_method_with_doc_ok",
        "missing_doc_warnings: 0\n",
    );
}

#[test]
fn test_20260420_pub_trait_impl_exempt() {
    // trait 实现方法（如 Display::fmt）不需要独立文档
    let source = r#"
use std::fmt;
pub struct X;
impl fmt::Display for X {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { Ok(()) }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_doc_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_pub_trait_impl_exempt",
        "missing_doc_warnings: 0\n",
    );
}

#[test]
fn test_20260420_pub_fn_in_mod_missing_doc() {
    let source = r#"
pub mod inner {
    pub fn rvs_foo() {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.missing_doc_warnings.len(), 1);
    assert_eq!(output.missing_doc_warnings[0].item, "rvs_foo");

    rvs_snapshot_BI(
        "20260420_pub_fn_in_mod_missing_doc",
        format!(
            "missing_doc_warnings: {}\n{}\n",
            output.missing_doc_warnings.len(),
            output.missing_doc_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_non_pub_fn_doc_optional() {
    let source = r#"
fn rvs_internal() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_doc_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_non_pub_fn_doc_optional",
        "missing_doc_warnings: 0\n",
    );
}

// ─── D: 禁用 #![deny(warnings)] 反模式 ────────────────────

#[test]
fn test_20260420_deny_warnings_detected() {
    let source = r#"
#![deny(warnings)]
pub fn rvs_foo() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.deny_warnings_warnings.len(), 1);

    rvs_snapshot_BI(
        "20260420_deny_warnings_detected",
        format!(
            "deny_warnings_warnings: {}\n{}\n",
            output.deny_warnings_warnings.len(),
            output.deny_warnings_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_deny_warnings_in_group() {
    // 在 deny(...) 里作为多个 lint 之一也要抓
    let source = r#"
#![deny(unused, warnings, dead_code)]
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.deny_warnings_warnings.len(), 1);

    rvs_snapshot_BI(
        "20260420_deny_warnings_in_group",
        format!(
            "deny_warnings_warnings: {}\n",
            output.deny_warnings_warnings.len()
        ),
    );
}

#[test]
fn test_20260420_deny_specific_lints_ok() {
    // 具名 lint 允许
    let source = r#"
#![deny(dead_code, unused_imports)]
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.deny_warnings_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_deny_specific_lints_ok",
        "deny_warnings_warnings: 0\n",
    );
}

// ─── A: 禁 wildcard import (use foo::*) ────────────────────

#[test]
fn test_20260420_wildcard_import_external() {
    let source = r#"
use bytes::*;
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.wildcard_import_warnings.len(), 1);
    assert_eq!(output.wildcard_import_warnings[0].use_path, "bytes::*");

    rvs_snapshot_BI(
        "20260420_wildcard_import_external",
        format!(
            "wildcard_import_warnings: {}\n{}\n",
            output.wildcard_import_warnings.len(),
            output.wildcard_import_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_wildcard_import_super_ok() {
    // 测试内部 use super::*; 是允许的
    let source = r#"
#[cfg(test)]
mod tests {
    use super::*;
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.wildcard_import_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_wildcard_import_super_ok",
        "wildcard_import_warnings: 0\n",
    );
}

#[test]
fn test_20260420_wildcard_import_prelude_ok() {
    // *::prelude::* 是允许的（crate 作者刻意暴露）
    let source = r#"
use tokio::prelude::*;
use std::io::prelude::*;
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.wildcard_import_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_wildcard_import_prelude_ok",
        "wildcard_import_warnings: 0\n",
    );
}

#[test]
fn test_20260420_wildcard_import_regular_use_ok() {
    let source = r#"
use std::collections::HashMap;
use std::fs::File;
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.wildcard_import_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_wildcard_import_regular_use_ok",
        "wildcard_import_warnings: 0\n",
    );
}

// ─── C: unsafe fn 必须有 /// # Safety 文档 ────────────────────

#[test]
fn test_20260420_unsafe_fn_missing_safety_doc() {
    let source = r#"
/// 一个未记录 safety 的 unsafe 函数。
pub unsafe fn rvs_dangerous_U() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.missing_safety_doc_warnings.len(), 1);
    assert_eq!(
        output.missing_safety_doc_warnings[0].function,
        "rvs_dangerous_U"
    );

    rvs_snapshot_BI(
        "20260420_unsafe_fn_missing_safety_doc",
        format!(
            "missing_safety_doc_warnings: {}\n{}\n",
            output.missing_safety_doc_warnings.len(),
            output.missing_safety_doc_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_unsafe_fn_with_safety_doc_ok() {
    let source = r#"
/// 做一些危险的事情。
///
/// # Safety
///
/// 调用者必须保证 ptr 有效。
pub unsafe fn rvs_dangerous_U() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_safety_doc_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_unsafe_fn_with_safety_doc_ok",
        "missing_safety_doc_warnings: 0\n",
    );
}

#[test]
fn test_20260420_safe_fn_doesnt_need_safety() {
    let source = r#"
/// 纯的、安全的函数。
pub fn rvs_safe() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_safety_doc_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_safe_fn_doesnt_need_safety",
        "missing_safety_doc_warnings: 0\n",
    );
}

// ─── B: 借用类型参数建议 ────────────────────────────────

#[test]
fn test_20260420_borrowed_param_ref_string() {
    // &String 应改 &str
    let source = r#"
pub fn rvs_foo(s: &String) -> usize { s.len() }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.borrowed_param_warnings.len(), 1);
    assert!(
        output.borrowed_param_warnings[0]
            .suggestion
            .contains("&str")
    );

    rvs_snapshot_BI(
        "20260420_borrowed_param_ref_string",
        format!(
            "borrowed_param_warnings: {}\n{}\n",
            output.borrowed_param_warnings.len(),
            output.borrowed_param_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_borrowed_param_ref_vec() {
    // &Vec<T> 应改 &[T]
    let source = r#"
pub fn rvs_sum(xs: &Vec<i32>) -> i32 { xs.iter().sum() }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.borrowed_param_warnings.len(), 1);
    assert!(
        output.borrowed_param_warnings[0]
            .suggestion
            .contains("&[T]")
    );

    rvs_snapshot_BI(
        "20260420_borrowed_param_ref_vec",
        format!(
            "borrowed_param_warnings: {}\n{}\n",
            output.borrowed_param_warnings.len(),
            output.borrowed_param_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_borrowed_param_ref_box() {
    // &Box<T> 应改 &T
    let source = r#"
pub fn rvs_foo(b: &Box<i32>) -> i32 { **b }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.borrowed_param_warnings.len(), 1);
    assert!(output.borrowed_param_warnings[0].suggestion.contains("&T"));

    rvs_snapshot_BI(
        "20260420_borrowed_param_ref_box",
        format!(
            "borrowed_param_warnings: {}\n{}\n",
            output.borrowed_param_warnings.len(),
            output.borrowed_param_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_borrowed_param_str_ok() {
    // &str 是对的
    let source = r#"
pub fn rvs_foo(s: &str) -> usize { s.len() }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.borrowed_param_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_borrowed_param_str_ok",
        "borrowed_param_warnings: 0\n",
    );
}

#[test]
fn test_20260420_borrowed_param_owned_string_ok() {
    // 拥有所有权的 String 参数是合理的（要 move/consume）
    let source = r#"
pub fn rvs_take(s: String) {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.borrowed_param_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_borrowed_param_owned_string_ok",
        "borrowed_param_warnings: 0\n",
    );
}

#[test]
fn test_20260420_borrowed_param_mut_ref_string_ok() {
    // &mut String 是合理的（可能要做 push_str 等）
    let source = r#"
pub fn rvs_append_M(s: &mut String) { s.push_str("x"); }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.borrowed_param_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_borrowed_param_mut_ref_string_ok",
        "borrowed_param_warnings: 0\n",
    );
}

#[test]
fn test_20260420_missing_debug_pub_struct() {
    let source = r#"
pub struct Foo { x: i32 }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.missing_debug_warnings.len(), 1);
    assert!(output.missing_debug_warnings[0].name.contains("Foo"));

    rvs_snapshot_BI(
        "20260420_missing_debug_pub_struct",
        format!(
            "missing_debug_warnings: {}\n{}\n",
            output.missing_debug_warnings.len(),
            output.missing_debug_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_missing_debug_pub_enum() {
    let source = r#"
pub enum Color { Red, Green, Blue }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.missing_debug_warnings.len(), 1);

    rvs_snapshot_BI(
        "20260420_missing_debug_pub_enum",
        format!(
            "missing_debug_warnings: {}\n{}\n",
            output.missing_debug_warnings.len(),
            output.missing_debug_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_missing_debug_with_derive_ok() {
    let source = r#"
#[derive(Debug)]
pub struct Foo { x: i32 }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_debug_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_missing_debug_with_derive_ok",
        "missing_debug_warnings: 0\n",
    );
}

#[test]
fn test_20260420_missing_debug_private_ok() {
    let source = r#"
struct Foo { x: i32 }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_debug_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_missing_debug_private_ok",
        "missing_debug_warnings: 0\n",
    );
}

#[test]
fn test_20260420_missing_panics_doc_P_marker() {
    let source = r#"
#[allow(non_snake_case)]
pub fn rvs_divide_P(a: i32, b: i32) -> i32 { a / b }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.missing_panics_doc_warnings.len(), 1);
    assert!(
        output.missing_panics_doc_warnings[0]
            .function
            .contains("rvs_divide_P")
    );

    rvs_snapshot_BI(
        "20260420_missing_panics_doc_P_marker",
        format!(
            "missing_panics_doc_warnings: {}\n{}\n",
            output.missing_panics_doc_warnings.len(),
            output.missing_panics_doc_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_missing_panics_doc_with_doc_ok() {
    let source = r#"
/// Does a division.
///
/// # Panics
///
/// Panics if b is zero.
#[allow(non_snake_case)]
pub fn rvs_divide_P(a: i32, b: i32) -> i32 { a / b }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_panics_doc_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_missing_panics_doc_with_doc_ok",
        "missing_panics_doc_warnings: 0\n",
    );
}

#[test]
fn test_20260420_missing_panics_doc_no_P_ok() {
    let source = r#"
#[allow(non_snake_case)]
pub fn rvs_add(a: i32, b: i32) -> i32 { a + b }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.missing_panics_doc_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_missing_panics_doc_no_P_ok",
        "missing_panics_doc_warnings: 0\n",
    );
}

#[test]
fn test_20260420_into_impl_detected() {
    let source = r#"
struct Celsius(f64);
impl Into<f64> for Celsius {
    fn into(self) -> f64 { self.0 }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.into_impl_warnings.len(), 1);
    assert!(output.into_impl_warnings[0].impl_type.contains("Celsius"));

    rvs_snapshot_BI(
        "20260420_into_impl_detected",
        format!(
            "into_impl_warnings: {}\n{}\n",
            output.into_impl_warnings.len(),
            output.into_impl_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_from_impl_ok() {
    let source = r#"
struct Celsius(f64);
impl From<Celsius> for f64 {
    fn from(c: Celsius) -> f64 { c.0 }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.into_impl_warnings.is_empty());

    rvs_snapshot_BI("20260420_from_impl_ok", "into_impl_warnings: 0\n");
}

#[test]
fn test_20260420_consumed_arg_on_error() {
    let source = r#"
#[allow(non_snake_case)]
pub fn rvs_parse_P(data: String) -> Result<(), ParseError> { Err(ParseError) }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.consumed_arg_on_error_warnings.len(), 1);
    assert!(
        output.consumed_arg_on_error_warnings[0]
            .param_type
            .contains("String")
    );

    rvs_snapshot_BI(
        "20260420_consumed_arg_on_error",
        format!(
            "consumed_arg_on_error_warnings: {}\n{}\n",
            output.consumed_arg_on_error_warnings.len(),
            output.consumed_arg_on_error_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_consumed_arg_preserved_in_error_ok() {
    let source = r#"
#[allow(non_snake_case)]
pub fn rvs_parse_P(data: String) -> Result<(), ParseError<String>> { Err(ParseError(data)) }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.consumed_arg_on_error_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_consumed_arg_preserved_in_error_ok",
        "consumed_arg_on_error_warnings: 0\n",
    );
}

#[test]
fn test_20260420_consumed_arg_ref_ok() {
    let source = r#"
#[allow(non_snake_case)]
pub fn rvs_parse(data: &str) -> Result<(), ParseError> { Err(ParseError) }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.consumed_arg_on_error_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_consumed_arg_ref_ok",
        "consumed_arg_on_error_warnings: 0\n",
    );
}

#[test]
fn test_20260420_deref_polymorphism_detected() {
    let source = r#"
struct MyVec(Vec<i32>);
impl std::ops::Deref for MyVec {
    type Target = Vec<i32>;
    fn deref(&self) -> &Self::Target { &self.0 }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.deref_polymorphism_warnings.len(), 1);
    assert!(
        output.deref_polymorphism_warnings[0]
            .impl_type
            .contains("MyVec")
    );

    rvs_snapshot_BI(
        "20260420_deref_polymorphism_detected",
        format!(
            "deref_polymorphism_warnings: {}\n{}\n",
            output.deref_polymorphism_warnings.len(),
            output.deref_polymorphism_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_deref_smart_pointer_ok() {
    let source = r#"
struct MyBox<T>(T);
impl<T> std::ops::Deref for MyBox<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.0 }
}
"#;
    // This WILL be flagged — Deref polymorphism is always flagged.
    // Smart pointers are a legitimate use, but the linter warns by default.
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.deref_polymorphism_warnings.len(), 1);

    rvs_snapshot_BI(
        "20260420_deref_smart_pointer_warned",
        format!(
            "deref_polymorphism_warnings: {}\n{}\n",
            output.deref_polymorphism_warnings.len(),
            output.deref_polymorphism_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_reflection_usage_any() {
    let source = r#"
#[allow(non_snake_case)]
pub fn rvs_process_P() -> String { std::any::type_name::<i32>().to_string() }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.reflection_usage_warnings.len(), 1);
    assert!(output.reflection_usage_warnings[0].path.contains("any"));

    rvs_snapshot_BI(
        "20260420_reflection_usage_any",
        format!(
            "reflection_usage_warnings: {}\n{}\n",
            output.reflection_usage_warnings.len(),
            output.reflection_usage_warnings[0],
        ),
    );
}

#[test]
fn test_20260420_reflection_usage_ok() {
    let source = r#"
#[allow(non_snake_case)]
pub fn rvs_process(value: &dyn std::fmt::Debug) -> String { format!("{value:?}") }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.reflection_usage_warnings.is_empty());

    rvs_snapshot_BI(
        "20260420_reflection_usage_ok",
        "reflection_usage_warnings: 0\n",
    );
}

// ─── Stub 检测：todo!() / unimplemented!() ─────────────────

#[test]
fn test_20260421_stub_todo_detected() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_fetch_AI() { todo!() }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let stub_violations: Vec<_> = output
        .violations
        .iter()
        .filter(|v| matches!(v.kind, rivus_linter::ViolationKind::StubMacro { .. }))
        .collect();
    assert_eq!(stub_violations.len(), 1);
    assert_eq!(stub_violations[0].caller, "rvs_fetch_AI");

    rvs_snapshot_BI(
        "20260421_stub_todo_detected",
        format!(
            "stub_violations: {}\n{}\n",
            stub_violations.len(),
            stub_violations[0],
        ),
    );
}

#[test]
fn test_20260421_stub_unimplemented_detected() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_parse_P() -> i32 { unimplemented!() }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let stub_violations: Vec<_> = output
        .violations
        .iter()
        .filter(|v| matches!(v.kind, rivus_linter::ViolationKind::StubMacro { .. }))
        .collect();
    assert_eq!(stub_violations.len(), 1);

    rvs_snapshot_BI(
        "20260421_stub_unimplemented_detected",
        format!(
            "stub_violations: {}\n{}\n",
            stub_violations.len(),
            stub_violations[0],
        ),
    );
}

#[test]
fn test_20260421_stub_no_stub_ok() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_add(a: i32, b: i32) -> i32 { a + b }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let stub_violations: Vec<_> = output
        .violations
        .iter()
        .filter(|v| matches!(v.kind, rivus_linter::ViolationKind::StubMacro { .. }))
        .collect();
    assert!(stub_violations.is_empty());

    rvs_snapshot_BI("20260421_stub_no_stub_ok", "stub_violations: 0\n");
}

#[test]
fn test_20260421_stub_in_impl_method() {
    let source = r#"
struct Svc;
#[allow(non_snake_case)]
impl Svc {
    fn rvs_run_AI(&self) { todo!("implement async run") }
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let stub_violations: Vec<_> = output
        .violations
        .iter()
        .filter(|v| matches!(v.kind, rivus_linter::ViolationKind::StubMacro { .. }))
        .collect();
    assert_eq!(stub_violations.len(), 1);
    assert_eq!(stub_violations[0].caller, "rvs_run_AI");

    rvs_snapshot_BI(
        "20260421_stub_in_impl_method",
        format!(
            "stub_violations: {}\n{}\n",
            stub_violations.len(),
            stub_violations[0],
        ),
    );
}

// ─── 空函数体检测 ────────────────────────────────────────

#[test]
fn test_20260421_empty_fn_detected() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_placeholder() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let empty_violations: Vec<_> = output
        .violations
        .iter()
        .filter(|v| matches!(v.kind, rivus_linter::ViolationKind::EmptyFn))
        .collect();
    assert_eq!(empty_violations.len(), 1);
    assert_eq!(empty_violations[0].caller, "rvs_placeholder");

    rvs_snapshot_BI(
        "20260421_empty_fn_detected",
        format!(
            "empty_fn_violations: {}\n{}\n",
            empty_violations.len(),
            empty_violations[0],
        ),
    );
}

#[test]
fn test_20260421_empty_fn_only_debug_assert_warned() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_check_M(n: i32) {
    debug_assert!(n > 0);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let empty_violations: Vec<_> = output
        .violations
        .iter()
        .filter(|v| matches!(v.kind, rivus_linter::ViolationKind::EmptyFn))
        .collect();
    assert_eq!(empty_violations.len(), 1);
    assert_eq!(empty_violations[0].caller, "rvs_check_M");

    rvs_snapshot_BI(
        "20260421_empty_fn_only_debug_assert_warned",
        format!(
            "empty_fn_violations: {}\n{}\n",
            empty_violations.len(),
            empty_violations[0],
        ),
    );
}

#[test]
fn test_20260421_empty_fn_with_logic_ok() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_add(a: i32, b: i32) -> i32 { a + b }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let empty_violations: Vec<_> = output
        .violations
        .iter()
        .filter(|v| matches!(v.kind, rivus_linter::ViolationKind::EmptyFn))
        .collect();
    assert!(empty_violations.is_empty());

    rvs_snapshot_BI(
        "20260421_empty_fn_with_logic_ok",
        "empty_fn_violations: 0\n",
    );
}

#[test]
fn test_20260421_empty_fn_in_impl() {
    let source = r#"
struct Svc;
impl Svc {
    fn rvs_empty_method(&self) {}
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let empty_violations: Vec<_> = output
        .violations
        .iter()
        .filter(|v| matches!(v.kind, rivus_linter::ViolationKind::EmptyFn))
        .collect();
    assert_eq!(empty_violations.len(), 1);
    assert_eq!(empty_violations[0].caller, "rvs_empty_method");

    rvs_snapshot_BI(
        "20260421_empty_fn_in_impl",
        format!(
            "empty_fn_violations: {}\n{}\n",
            empty_violations.len(),
            empty_violations[0],
        ),
    );
}

// ─── TODO/FIXME 注释检测 ─────────────────────────────────

#[test]
fn test_20260421_todo_comment_detected() {
    let source = r#"
// TODO implement this later
#[allow(non_snake_case)]
fn rvs_stub() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.todo_comment_warnings.len(), 1);
    assert_eq!(output.todo_comment_warnings[0].kind, "TODO");

    rvs_snapshot_BI(
        "20260421_todo_comment_detected",
        format!(
            "todo_comment_warnings: {}\n{}\n",
            output.todo_comment_warnings.len(),
            output.todo_comment_warnings[0],
        ),
    );
}

#[test]
fn test_20260421_fixme_comment_detected() {
    let source = r#"
// FIXME this is broken
#[allow(non_snake_case)]
fn rvs_broken_P() { panic!() }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.todo_comment_warnings.len(), 1);
    assert_eq!(output.todo_comment_warnings[0].kind, "FIXME");

    rvs_snapshot_BI(
        "20260421_fixme_comment_detected",
        format!(
            "todo_comment_warnings: {}\n{}\n",
            output.todo_comment_warnings.len(),
            output.todo_comment_warnings[0],
        ),
    );
}

#[test]
fn test_20260421_no_todo_comment_ok() {
    let source = r#"
// This is a normal comment
#[allow(non_snake_case)]
fn rvs_done() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.todo_comment_warnings.is_empty());

    rvs_snapshot_BI("20260421_no_todo_comment_ok", "todo_comment_warnings: 0\n");
}

#[test]
fn test_20260421_todo_in_block_comment() {
    let source = r#"
/* TODO: revisit this */
#[allow(non_snake_case)]
fn rvs_legacy() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.todo_comment_warnings.len(), 1);
    assert_eq!(output.todo_comment_warnings[0].kind, "TODO");

    rvs_snapshot_BI(
        "20260421_todo_in_block_comment",
        format!(
            "todo_comment_warnings: {}\n{}\n",
            output.todo_comment_warnings.len(),
            output.todo_comment_warnings[0],
        ),
    );
}

// ─── 好函数未被测试覆盖检测 ──────────────────────────────

#[test]
fn test_20260421_untested_good_fn_detected() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_add(a: i32, b: i32) -> i32 { a + b }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.untested_good_fn_warnings.len(), 1);
    assert_eq!(output.untested_good_fn_warnings[0].function, "rvs_add");

    rvs_snapshot_BI(
        "20260421_untested_good_fn_detected",
        format!(
            "untested_good_fn_warnings: {}\n{}\n",
            output.untested_good_fn_warnings.len(),
            output.untested_good_fn_warnings[0],
        ),
    );
}

#[test]
fn test_20260421_good_fn_with_test_ok() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_add(a: i32, b: i32) -> i32 { a + b }

#[test]
fn test_20260421_add_works() {
    rvs_add(1, 2);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.untested_good_fn_warnings.is_empty());

    rvs_snapshot_BI(
        "20260421_good_fn_with_test_ok",
        "untested_good_fn_warnings: 0\n",
    );
}

#[test]
fn test_20260421_non_good_fn_untested_ok() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_fetch_ABI() { todo!() }
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.untested_good_fn_warnings.is_empty());

    rvs_snapshot_BI(
        "20260421_non_good_fn_untested_ok",
        "untested_good_fn_warnings: 0\n",
    );
}

#[test]
fn test_20260421_good_fn_dead_code_exempt() {
    let source = r#"
#[allow(dead_code)]
#[allow(non_snake_case)]
fn rvs_helper() {}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.untested_good_fn_warnings.is_empty());

    rvs_snapshot_BI(
        "20260421_good_fn_dead_code_exempt",
        "untested_good_fn_warnings: 0\n",
    );
}

#[test]
fn test_20260421_good_fn_mutable_with_test_ok() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_sort_M(arr: &mut [i32]) { arr.sort(); }

#[test]
fn test_20260421_sort_works() {
    let mut v = [3, 1, 2];
    rvs_sort_M(&mut v);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.untested_good_fn_warnings.is_empty());

    rvs_snapshot_BI(
        "20260421_good_fn_mutable_with_test_ok",
        "untested_good_fn_warnings: 0\n",
    );
}

// ─── 宏内调用提取 ──────────────────────────────────────

#[test]
fn test_20260421_assert_eq_macro_call_detected() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_add(a: i32, b: i32) -> i32 { a + b }

#[test]
fn test_20260421_add_works() {
    assert_eq!(rvs_add(1, 2), 3);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.untested_good_fn_warnings.is_empty());

    rvs_snapshot_BI(
        "20260421_assert_eq_macro_call_detected",
        "untested_good_fn_warnings: 0\n",
    );
}

#[test]
fn test_20260421_format_macro_call_detected() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_add(a: i32, b: i32) -> i32 { a + b }

#[test]
fn test_20260421_add_works() {
    let _ = format!("result: {}", rvs_add(1, 2));
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.untested_good_fn_warnings.is_empty());

    rvs_snapshot_BI(
        "20260421_format_macro_call_detected",
        "untested_good_fn_warnings: 0\n",
    );
}

#[test]
fn test_20260421_method_call_in_macro_detected() {
    let source = r#"
struct S;
#[allow(non_snake_case)]
impl S {
    fn rvs_compute(&self) -> i32 { 42 }
}

#[test]
fn test_20260421_compute_works() {
    let s = S;
    println!("got {}", s.rvs_compute());
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.untested_good_fn_warnings.is_empty());

    rvs_snapshot_BI(
        "20260421_method_call_in_macro_detected",
        "untested_good_fn_warnings: 0\n",
    );
}

#[test]
fn test_20260421_violation_in_macro_detected() {
    let source = r#"
#[allow(non_snake_case)]
fn rvs_pure_fn() {
    let _ = format!("calling {}", std::fs::read_to_string("x").unwrap());
}
"#;
    let output = rvs_check_source(
        source,
        "test.rs",
        &CapsMap::rvs_parse("std::fs::read_to_string=BI\n").unwrap(),
    )
    .unwrap();
    assert!(!output.violations.is_empty());

    rvs_snapshot_BI(
        "20260421_violation_in_macro_detected",
        format!("violations: {}\n", output.violations.len()),
    );
}

#[test]
fn test_20260422_missing_panic_shows_warning_severity() {
    let source = r#"
fn rvs_bail(msg: &str) {
    panic!("{}", msg);
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let missing_panic = output
        .inference_warnings
        .iter()
        .find(|w| w.kind == InferenceKind::MissingPanic)
        .unwrap();

    let displayed = format!("{missing_panic}");
    assert!(
        displayed.starts_with("warning:"),
        "MissingPanic should display as warning, got: {displayed}"
    );

    rvs_snapshot_BI(
        "20260422_missing_panic_shows_warning_severity",
        format!("display: {displayed}\n"),
    );
}

#[test]
fn test_20260422_non_panic_inference_still_hint() {
    let source = r#"
async fn rvs_fetch(url: &str) {
    rvs_do_nothing();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    let non_panic: Vec<_> = output
        .inference_warnings
        .iter()
        .filter(|w| w.kind != InferenceKind::MissingPanic)
        .collect();

    for inf in &non_panic {
        let displayed = format!("{inf}");
        assert!(
            displayed.starts_with("hint:"),
            "non-MissingPanic inference should still be hint, got: {displayed}",
        );
    }

    rvs_snapshot_BI(
        "20260422_non_panic_inference_still_hint",
        format!("non_panic_inferences: {}\n", non_panic.len()),
    );
}

#[test]
fn test_20260425_error_swallow_ok_detected() {
    let source = r#"
#![allow(non_snake_case)]
fn rvs_foo() {
    let _ = bar().ok();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(!output.error_swallow_warnings.is_empty());
    let w = &output.error_swallow_warnings[0];
    assert_eq!(w.method, "ok");
    assert_eq!(w.function, "rvs_foo");

    rvs_snapshot_BI(
        "20260425_error_swallow_ok_detected",
        format!("warnings: {}\n", output.error_swallow_warnings.len()),
    );
}

#[test]
fn test_20260425_error_swallow_unwrap_or_default_detected() {
    let source = r#"
#![allow(non_snake_case)]
fn rvs_bar() {
    let x = baz().unwrap_or_default();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(!output.error_swallow_warnings.is_empty());
    let w = &output.error_swallow_warnings[0];
    assert_eq!(w.method, "unwrap_or_default");

    rvs_snapshot_BI(
        "20260425_error_swallow_unwrap_or_default_detected",
        format!("warnings: {}\n", output.error_swallow_warnings.len()),
    );
}

#[test]
fn test_20260425_error_swallow_none_ok() {
    let source = r#"
#![allow(non_snake_case)]
fn rvs_baz() {
    let _ = bar().unwrap();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.error_swallow_warnings.is_empty());

    rvs_snapshot_BI(
        "20260425_error_swallow_none_ok",
        "error_swallow_warnings: 0\n".to_string(),
    );
}

#[test]
fn test_20260425_catch_unwind_detected() {
    let source = r#"
#![allow(non_snake_case)]
fn rvs_safe_call() {
    let _ = std::panic::catch_unwind(|| dangerous());
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(!output.catch_unwind_warnings.is_empty());
    assert_eq!(output.catch_unwind_warnings[0].function, "rvs_safe_call");

    rvs_snapshot_BI(
        "20260425_catch_unwind_detected",
        format!("warnings: {}\n", output.catch_unwind_warnings.len()),
    );
}

#[test]
fn test_20260425_catch_unwind_none_ok() {
    let source = r#"
#![allow(non_snake_case)]
fn rvs_normal() {
    let _ = foo();
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.catch_unwind_warnings.is_empty());

    rvs_snapshot_BI(
        "20260425_catch_unwind_none_ok",
        "catch_unwind_warnings: 0\n".to_string(),
    );
}

#[test]
fn test_20260425_catch_all_error_variant_unknown() {
    let source = r#"
#![allow(non_snake_case)]
#[derive(Debug, thiserror::Error)]
enum MyError {
    #[error("not found")]
    NotFound,
    #[error("unknown")]
    Unknown,
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(!output.catch_all_error_variant_warnings.is_empty());
    let w = &output.catch_all_error_variant_warnings[0];
    assert_eq!(w.enum_name, "MyError");
    assert_eq!(w.variant, "Unknown");

    rvs_snapshot_BI(
        "20260425_catch_all_error_variant_unknown",
        format!(
            "warnings: {}\n",
            output.catch_all_error_variant_warnings.len()
        ),
    );
}

#[test]
fn test_20260425_catch_all_error_variant_other() {
    let source = r#"
#![allow(non_snake_case)]
#[derive(Debug, thiserror::Error)]
enum RepoError {
    #[error("io")]
    Io(#[from] std::io::Error),
    #[error("other")]
    Other(String),
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(!output.catch_all_error_variant_warnings.is_empty());
    assert_eq!(output.catch_all_error_variant_warnings[0].variant, "Other");

    rvs_snapshot_BI(
        "20260425_catch_all_error_variant_other",
        format!(
            "warnings: {}\n",
            output.catch_all_error_variant_warnings.len()
        ),
    );
}

#[test]
fn test_20260425_catch_all_error_variant_none_ok() {
    let source = r#"
#![allow(non_snake_case)]
#[derive(Debug, thiserror::Error)]
enum GoodError {
    #[error("not found")]
    NotFound,
    #[error("permission denied")]
    PermissionDenied,
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.catch_all_error_variant_warnings.is_empty());

    rvs_snapshot_BI(
        "20260425_catch_all_error_variant_none_ok",
        "catch_all_error_variant_warnings: 0\n".to_string(),
    );
}

#[test]
fn test_20260425_catch_all_error_variant_non_error_enum_ok() {
    let source = r#"
#![allow(non_snake_case)]
enum Color {
    Unknown,
    Other,
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.catch_all_error_variant_warnings.is_empty());

    rvs_snapshot_BI(
        "20260425_catch_all_error_variant_non_error_enum_ok",
        "catch_all_error_variant_warnings: 0\n".to_string(),
    );
}

#[test]
fn test_20260425_missing_test_output_self_check() {
    let output = rvs_check_path_BI(std::path::Path::new("src/"), &CapsMap::rvs_new()).unwrap();

    rvs_snapshot_BI(
        "20260425_missing_test_output_self_check",
        format!(
            "src has {} tests with missing snapshots\n",
            output.missing_test_output_warnings.len()
        ),
    );
}

#[test]
fn test_20260426_validate_returns_unit_detected() {
    let source = r#"
#![allow(non_snake_case)]
fn rvs_validate_email(raw: &str) -> Result<(), ParseError> {
    Ok(())
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(!output.validate_returns_unit_warnings.is_empty());
    assert_eq!(
        output.validate_returns_unit_warnings[0].function,
        "rvs_validate_email"
    );

    rvs_snapshot_BI(
        "20260426_validate_returns_unit_detected",
        format!(
            "warnings: {}\n",
            output.validate_returns_unit_warnings.len()
        ),
    );
}

#[test]
fn test_20260426_validate_returns_unit_none_ok() {
    let source = r#"
#![allow(non_snake_case)]
fn rvs_parse_email(raw: &str) -> Result<Email, ParseError> {
    Ok(Email {})
}
"#;
    let output = rvs_check_source(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.validate_returns_unit_warnings.is_empty());

    rvs_snapshot_BI(
        "20260426_validate_returns_unit_none_ok",
        "validate_returns_unit_warnings: 0\n".to_string(),
    );
}
