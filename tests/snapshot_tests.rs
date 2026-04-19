#![allow(non_snake_case)]

use std::collections::BTreeSet;

use rivus_linter::capsmap::CapsMap;
use rivus_linter::capability::{
    parse_rvs_function, Capability, CapabilityParseError, CapabilitySet,
};
use rivus_linter::check::rvs_check_source_E;
use rivus_linter::extract::rvs_extract_functions_E;
use rivus_linter::report::rvs_build_report;

fn rvs_write_snapshot_BIP(name: &str, content: &str) {
    std::fs::create_dir_all("test_out").unwrap();
    std::fs::write(format!("test_out/{name}.out"), content).unwrap();
}

fn rvs_snapshot_BIP(name: &str, content: impl std::fmt::Display) {
    let content = content.to_string();
    rvs_write_snapshot_BIP(name, &content);
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
    let (base, caps) = parse_rvs_function("rvs_add").unwrap();
    assert_eq!(base, "add");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BIP(
        "20260418_parse_no_suffix",
        format!("input: rvs_add\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_single_cap() {
    let (base, caps) = parse_rvs_function("rvs_parse_int_E").unwrap();
    assert_eq!(base, "parse_int");
    assert_eq!(caps.rvs_len(), 1);
    assert!(caps.rvs_contains(Capability::E));

    rvs_snapshot_BIP(
        "20260418_parse_single_cap",
        format!("input: rvs_parse_int_E\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_multi_cap() {
    let (base, caps) = parse_rvs_function("rvs_write_db_ABEI").unwrap();
    assert_eq!(base, "write_db");
    assert!(caps.rvs_contains(Capability::A));
    assert!(caps.rvs_contains(Capability::B));
    assert!(caps.rvs_contains(Capability::E));
    assert!(caps.rvs_contains(Capability::I));
    assert_eq!(caps.rvs_len(), 4);

    rvs_snapshot_BIP(
        "20260418_parse_multi_cap",
        format!("input: rvs_write_db_ABEI\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_no_cap_tricky_name() {
    let (base, caps) = parse_rvs_function("rvs_cache_lookup").unwrap();
    assert_eq!(base, "cache_lookup");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BIP(
        "20260418_parse_no_cap_tricky_name",
        format!("input: rvs_cache_lookup\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_two_caps() {
    let (base, caps) = parse_rvs_function("rvs_random_uuid_PT").unwrap();
    assert_eq!(base, "random_uuid");
    assert!(caps.rvs_contains(Capability::P));
    assert!(caps.rvs_contains(Capability::T));
    assert_eq!(caps.rvs_len(), 2);

    rvs_snapshot_BIP(
        "20260418_parse_two_caps",
        format!("input: rvs_random_uuid_PT\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_non_rvs() {
    let result = parse_rvs_function("not_rvs_function");
    assert!(result.is_none());

    rvs_snapshot_BIP("20260418_parse_non_rvs", "input: not_rvs_function\nresult: None\n");
}

#[test]
fn test_20260418_parse_bare_rvs() {
    let (base, caps) = parse_rvs_function("rvs_").unwrap();
    assert_eq!(base, "");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BIP(
        "20260418_parse_bare_rvs",
        format!("input: rvs_\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_no_underscore_after_rvs() {
    let (base, caps) = parse_rvs_function("rvs_E").unwrap();
    assert_eq!(base, "E");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BIP(
        "20260418_parse_no_underscore_after_rvs",
        format!("input: rvs_E\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_short_base_with_cap() {
    let (base, caps) = parse_rvs_function("rvs_a_B").unwrap();
    assert_eq!(base, "a");
    assert!(caps.rvs_contains(Capability::B));
    assert_eq!(caps.rvs_len(), 1);

    rvs_snapshot_BIP(
        "20260418_parse_short_base_with_cap",
        format!("input: rvs_a_B\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_lowercase_suffix() {
    let (base, caps) = parse_rvs_function("rvs_foo_e").unwrap();
    assert_eq!(base, "foo_e");
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BIP(
        "20260418_parse_lowercase_suffix",
        format!("input: rvs_foo_e\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_parse_all_eight_caps() {
    let (base, caps) = parse_rvs_function("rvs_nuclear_ABEIMPTU").unwrap();
    assert_eq!(base, "nuclear");
    assert_eq!(caps.rvs_len(), 8);

    rvs_snapshot_BIP(
        "20260418_parse_all_eight_caps",
        format!("input: rvs_nuclear_ABEIMPTU\nbase: {base:?}\ncapabilities: {caps}\n"),
    );
}

// ─── 合规检查 ─────────────────────────────────────────────

#[test]
fn test_20260418_compliance_superset_can_call_subset() {
    let caller = CapabilitySet::rvs_from_str_E("ABEI").unwrap();
    let callee = CapabilitySet::rvs_from_str_E("E").unwrap();
    assert!(caller.rvs_can_call(&callee));

    let missing = caller.rvs_missing_for(&callee);
    assert!(missing.is_empty());

    rvs_snapshot_BIP(
        "20260418_compliance_superset_can_call_subset",
        format!(
            "caller: {caller}\ncallee: {callee}\ncan_call: true\nmissing: {{}}\n",
        ),
    );
}

#[test]
fn test_20260418_compliance_subset_cannot_call_superset() {
    let caller = CapabilitySet::rvs_from_str_E("E").unwrap();
    let callee = CapabilitySet::rvs_from_str_E("ABEI").unwrap();
    assert!(!caller.rvs_can_call(&callee));

    let missing = caller.rvs_missing_for(&callee);
    assert_eq!(missing.len(), 3);
    assert!(missing.contains(&Capability::A));
    assert!(missing.contains(&Capability::B));
    assert!(missing.contains(&Capability::I));

    rvs_snapshot_BIP(
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
    let callee = CapabilitySet::rvs_from_str_E("M").unwrap();
    assert!(!caller.rvs_can_call(&callee));

    let missing = caller.rvs_missing_for(&callee);
    assert_eq!(missing.len(), 1);

    rvs_snapshot_BIP(
        "20260418_compliance_empty_cannot_call_cap",
        format!(
            "caller: {caller}\ncallee: {callee}\ncan_call: false\nmissing: {{{}}}\n",
            rvs_format_caps(&missing),
        ),
    );
}

#[test]
fn test_20260418_compliance_cap_can_call_empty() {
    let caller = CapabilitySet::rvs_from_str_E("M").unwrap();
    let callee = CapabilitySet::rvs_new();
    assert!(caller.rvs_can_call(&callee));

    rvs_snapshot_BIP(
        "20260418_compliance_cap_can_call_empty",
        format!(
            "caller: {caller}\ncallee: {callee}\ncan_call: true\n",
        ),
    );
}

#[test]
fn test_20260418_compliance_empty_can_call_empty() {
    let caller = CapabilitySet::rvs_new();
    let callee = CapabilitySet::rvs_new();
    assert!(caller.rvs_can_call(&callee));

    rvs_snapshot_BIP(
        "20260418_compliance_empty_can_call_empty",
        format!(
            "caller: {caller}\ncallee: {callee}\ncan_call: true\n",
        ),
    );
}

#[test]
fn test_20260418_compliance_same_set_can_call() {
    let caller = CapabilitySet::rvs_from_str_E("ABEIMPTU").unwrap();
    let callee = CapabilitySet::rvs_from_str_E("ABEIMPTU").unwrap();
    assert!(caller.rvs_can_call(&callee));

    rvs_snapshot_BIP(
        "20260418_compliance_same_set_can_call",
        format!(
            "caller: {caller}\ncallee: {callee}\ncan_call: true\n",
        ),
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
    let fns = rvs_extract_functions_E(source).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_add");
    assert!(fns[0].capabilities.rvs_is_empty());
    assert!(fns[0].calls.is_empty());

    rvs_snapshot_BIP(
        "20260418_syn_parse_single_fn",
        format!("functions: 1\nname: {}\ncaps: {}\ncalls: {}\n", fns[0].name, fns[0].capabilities, fns[0].calls.len()),
    );
}

#[test]
fn test_20260418_syn_parse_fn_with_calls() {
    let source = r#"
fn rvs_write_db_ABEI() {
    rvs_parse_int_E("42");
    rvs_validate_M(data);
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    assert_eq!(fns.len(), 1);
    let func = &fns[0];
    assert_eq!(func.name, "rvs_write_db_ABEI");
    assert_eq!(func.calls.len(), 2);
    assert_eq!(func.calls[0].name, "rvs_parse_int_E");
    assert_eq!(func.calls[1].name, "rvs_validate_M");

    rvs_snapshot_BIP(
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
fn rvs_create_order_ABEIP(cmd: &str) {
    self.repo.rvs_find_by_id_ABEI(42);
    self.publisher.rvs_publish_ABEI(event);
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    let func = &fns[0];
    assert_eq!(func.calls.len(), 2);
    assert_eq!(func.calls[0].name, "rvs_find_by_id_ABEI");
    assert_eq!(func.calls[1].name, "rvs_publish_ABEI");

    rvs_snapshot_BIP(
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

fn rvs_check_E() {
    regular_function();
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_check_E");
    assert_eq!(fns[0].calls.len(), 1);
    assert_eq!(fns[0].calls[0].name, "regular_function");

    rvs_snapshot_BIP(
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
    fn rvs_process_ABEI(&self, data: &str) {
        self.rvs_validate_E(data);
    }

    fn rvs_validate_E(&self, data: &str) {
        // validation logic
    }

    fn helper(&self) {
        // not an rvs_ function
    }
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    assert_eq!(fns.len(), 2);

    rvs_snapshot_BIP(
        "20260418_syn_parse_impl_method",
        format!(
            "functions: {}\n1: {} caps={} calls={}\n2: {} caps={} calls={}\n",
            fns.len(),
            fns[0].name, fns[0].capabilities, fns[0].calls.len(),
            fns[1].name, fns[1].capabilities, fns[1].calls.len(),
        ),
    );
}

#[test]
fn test_20260418_syn_parse_trait_method() {
    let source = r#"
trait Repository {
    fn rvs_find_by_id_ABEI(&self, id: u64);
    fn rvs_save_ABEI(&self, data: &str);
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    assert_eq!(fns.len(), 2);
    assert!(fns[0].calls.is_empty());
    assert!(fns[1].calls.is_empty());

    rvs_snapshot_BIP(
        "20260418_syn_parse_trait_method",
        format!(
            "functions: {}\n1: {} caps={}\n2: {} caps={}\n",
            fns.len(),
            fns[0].name, fns[0].capabilities,
            fns[1].name, fns[1].capabilities,
        ),
    );
}

#[test]
fn test_20260418_syn_trait_default_impl() {
    let source = r#"
trait Handler {
    fn rvs_handle_ABE(&self) {
        self.rvs_validate_E();
    }
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].calls.len(), 1);
    assert_eq!(fns[0].calls[0].name, "rvs_validate_E");

    rvs_snapshot_BIP(
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
fn rvs_outer_AEI() {
    let closure = || {
        rvs_inner_E();
    };
    closure();
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    assert_eq!(fns[0].calls.len(), 2);
    assert_eq!(fns[0].calls[0].name, "rvs_inner_E");
    assert_eq!(fns[0].calls[1].name, "closure");

    rvs_snapshot_BIP(
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
fn rvs_outer_ABEI() {
    rvs_inner_E();
    rvs_inner();
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BIP(
        "20260418_linter_compliant_code",
        "violations: 0\n",
    );
}

#[test]
fn test_20260418_linter_single_violation() {
    let source = r#"
fn rvs_inner_E() {
    rvs_outer_ABEI();
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);

    let v = &output.violations[0];
    assert_eq!(v.caller, "rvs_inner_E");
    assert_eq!(v.target, "rvs_outer_ABEI");
    assert!(v.missing.contains(&Capability::A));
    assert!(v.missing.contains(&Capability::B));
    assert!(v.missing.contains(&Capability::I));

    rvs_snapshot_BIP(
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
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::M));

    rvs_snapshot_BIP(
        "20260418_linter_pure_calls_mutable",
        format!("violations: {}\n{}", output.violations.len(), &output.violations[0]),
    );
}

#[test]
fn test_20260418_linter_mutable_calls_pure_ok() {
    let source = r#"
fn rvs_sort_inplace_M(data: &mut [i32]) {
    rvs_add(1, 2);
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BIP(
        "20260418_linter_mutable_calls_pure_ok",
        "violations: 0\n",
    );
}

#[test]
fn test_20260418_linter_multiple_functions() {
    let source = r#"
fn rvs_good_ABEI() {
    rvs_helper_E();
}

fn rvs_bad_E() {
    rvs_good_ABEI();
}

fn rvs_pure() {
    rvs_bad_E();
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 2);

    let violation_text = output
        .violations
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n---\n");

    rvs_snapshot_BIP(
        "20260418_linter_multiple_functions",
        format!("violations: {}\n{violation_text}\n", output.violations.len()),
    );
}

#[test]
fn test_20260418_linter_method_call_violation() {
    let source = r#"
struct Foo;

impl Foo {
    fn rvs_simple_E(&self) {
        self.rvs_complex_ABEI();
    }
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);

    rvs_snapshot_BIP(
        "20260418_linter_method_call_violation",
        format!("violations: {}\n{}", output.violations.len(), &output.violations[0]),
    );
}

#[test]
fn test_20260418_linter_all_caps_compliant() {
    let source = r#"
fn rvs_nuclear_ABEIMPTU() {
    rvs_async_A();
    rvs_block_B();
    rvs_fail_E();
    rvs_io_I();
    rvs_mut_M();
    rvs_impure_P();
    rvs_thread_T();
    rvs_unsafe_U();
    rvs_pure();
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BIP(
        "20260418_linter_all_caps_compliant",
        "violations: 0\n",
    );
}

// ─── CapabilitySet::rvs_from_str_E ──────────────────────────

#[test]
fn test_20260418_capset_from_str_valid() {
    let caps = CapabilitySet::rvs_from_str_E("ABEI").unwrap();
    assert_eq!(caps.rvs_len(), 4);
    assert!(caps.rvs_contains(Capability::A));
    assert!(caps.rvs_contains(Capability::B));
    assert!(caps.rvs_contains(Capability::E));
    assert!(caps.rvs_contains(Capability::I));

    rvs_snapshot_BIP(
        "20260418_capset_from_str_valid",
        format!("input: ABEI\ncapabilities: {caps}\n"),
    );
}

#[test]
fn test_20260418_capset_from_str_invalid() {
    let result = CapabilitySet::rvs_from_str_E("ABX");
    assert!(matches!(result, Err(CapabilityParseError::InvalidLetter('X'))));

    rvs_snapshot_BIP(
        "20260418_capset_from_str_invalid",
        "input: ABX\nresult: Err(InvalidLetter('X'))\n",
    );
}

// ─── proc_macro2 span 行号测试 ────────────────────────────

#[test]
fn test_20260418_span_line_numbers() {
    let source = r#"fn rvs_top_E() {
    rvs_sub_E();
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    assert_eq!(fns[0].line, 1);
    assert_eq!(fns[0].calls[0].line, 2);

    rvs_snapshot_BIP(
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

fn rvs_parse_E(s: &str) -> Result<i32, ()> {
    Ok(42)
}

fn rvs_write_file_BEI(path: &str) {
    rvs_parse_E(path);
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    let report = rvs_build_report(&fns);

    assert_eq!(report.total_fn_count, 3);
    assert_eq!(report.pure_fn_count, 1);

    rvs_snapshot_BIP(
        "20260418_report_basic",
        format!("{report}"),
    );
}

#[test]
fn test_20260418_report_empty() {
    let source = r#"fn main() {}"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    let report = rvs_build_report(&fns);

    assert_eq!(report.total_fn_count, 0);
    assert_eq!(report.total_line_count, 0);

    rvs_snapshot_BIP(
        "20260418_report_empty",
        format!("{report}"),
    );
}

#[test]
fn test_20260418_report_overlapping_caps() {
    let source = r#"
fn rvs_mega_ABEIMPTU() {
    // this function has all 8 capabilities
    let x = 1 + 2;
    let y = x * 3;
    let z = y + x;
}
"#;
    let fns = rvs_extract_functions_E(source).unwrap();
    let report = rvs_build_report(&fns);

    assert_eq!(report.total_fn_count, 1);
    assert_eq!(report.by_capability.len(), 8);

    for cap in report.by_capability.values() {
        assert_eq!(cap.fn_count, 1);
        assert_eq!(cap.line_count, report.total_line_count);
    }

    rvs_snapshot_BIP(
        "20260418_report_overlapping_caps",
        format!("{report}"),
    );
}

// ─── CapsMap 解析与查找 ─────────────────────────────────

#[test]
fn test_20260419_capsmap_parse_basic() {
    let content = "std::fs::read_to_string=BEI\nVec::new=\n";
    let cm = CapsMap::rvs_parse_E(content).unwrap();

    let caps = cm.rvs_lookup("std::fs::read_to_string").unwrap();
    assert!(caps.rvs_contains(Capability::B));
    assert!(caps.rvs_contains(Capability::E));
    assert!(caps.rvs_contains(Capability::I));
    assert_eq!(caps.rvs_len(), 3);

    let caps = cm.rvs_lookup("Vec::new").unwrap();
    assert!(caps.rvs_is_empty());

    rvs_snapshot_BIP(
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
    let content = "# 这是一个注释\nstd::process::exit=P # 强副作用\n\n";
    let cm = CapsMap::rvs_parse_E(content).unwrap();
    let caps = cm.rvs_lookup("std::process::exit").unwrap();
    assert!(caps.rvs_contains(Capability::P));

    rvs_snapshot_BIP(
        "20260419_capsmap_parse_comments",
        format!("std::process::exit: {}\n", caps),
    );
}

#[test]
fn test_20260419_capsmap_suffix_match() {
    let content = "alloc::vec::Vec::new=\nstd::process::exit=P\n";
    let cm = CapsMap::rvs_parse_E(content).unwrap();

    let caps = cm.rvs_lookup("Vec::new").unwrap();
    assert!(caps.rvs_is_empty());

    let caps = cm.rvs_lookup("exit").unwrap();
    assert!(caps.rvs_contains(Capability::P));

    assert!(cm.rvs_lookup("nonexistent").is_none());

    rvs_snapshot_BIP(
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
fn rvs_good_E() {
    unknown_function();
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(output.warnings[0].callee, "unknown_function");
    assert_eq!(output.warnings[0].caller, "rvs_good_E");

    rvs_snapshot_BIP(
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
    let content = "heavy_io=BEI\npure_thing=\n";
    let cm = CapsMap::rvs_parse_E(content).unwrap();

    let source = r#"
fn rvs_simple() {
    pure_thing();
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &cm).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.warnings.is_empty());

    rvs_snapshot_BIP(
        "20260419_capsmap_known_non_rvs_compliance",
        "violations: 0\nwarnings: 0\n",
    );
}

#[test]
fn test_20260419_capsmap_known_non_rvs_violation() {
    let content = "heavy_io=BEI\n";
    let cm = CapsMap::rvs_parse_E(content).unwrap();

    let source = r#"
fn rvs_simple() {
    heavy_io();
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &cm).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::B));
    assert!(output.violations[0].missing.contains(&Capability::E));
    assert!(output.violations[0].missing.contains(&Capability::I));

    rvs_snapshot_BIP(
        "20260419_capsmap_known_non_rvs_violation",
        format!("violations: {}\n{}\n", output.violations.len(), output.violations[0]),
    );
}

// ─── 静态变量与 thread_local! 检查 ────────────────────────

#[test]
fn test_20260418_static_ref_requires_P() {
    let source = r#"
static COUNTER: i32 = 0;

fn rvs_read_counter() -> i32 {
    COUNTER
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert_eq!(output.violations[0].kind, rivus_linter::check::ViolationKind::StaticRef);
    assert!(output.violations[0].missing.contains(&Capability::P));
    assert_eq!(output.violations[0].target, "COUNTER");

    rvs_snapshot_BIP(
        "20260418_static_ref_requires_P",
        format!("violations: {}\n{}\n", output.violations.len(), output.violations[0]),
    );
}

#[test]
fn test_20260418_static_ref_with_P_ok() {
    let source = r#"
static COUNTER: i32 = 0;

fn rvs_read_counter_P() -> i32 {
    COUNTER
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BIP(
        "20260418_static_ref_with_P_ok",
        "violations: 0\n",
    );
}

#[test]
fn test_20260418_static_mut_ref_requires_PU() {
    let source = r#"
static mut STATE: i32 = 0;

fn rvs_read_state_U() -> i32 {
    unsafe { STATE }
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::P));

    rvs_snapshot_BIP(
        "20260418_static_mut_ref_requires_PU",
        format!("violations: {}\n{}\n", output.violations.len(), output.violations[0]),
    );
}

#[test]
fn test_20260418_static_mut_ref_with_UP_ok() {
    let source = r#"
static mut STATE: i32 = 0;

fn rvs_read_state_PU() -> i32 {
    unsafe { STATE }
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BIP(
        "20260418_static_mut_ref_with_UP_ok",
        "violations: 0\n",
    );
}

#[test]
fn test_20260418_thread_local_ref_requires_TP() {
    let source = r#"
thread_local! {
    static TLS: i32 = 42;
}

fn rvs_read_tls() -> i32 {
    TLS.with(|v| *v)
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.len() >= 1);
    let tls_violation = output.violations.iter().find(|v| v.target == "TLS").unwrap();
    assert!(tls_violation.missing.contains(&Capability::T));
    assert!(tls_violation.missing.contains(&Capability::P));

    rvs_snapshot_BIP(
        "20260418_thread_local_ref_requires_TP",
        format!("violations: {}\n{}\n", output.violations.len(), tls_violation),
    );
}

#[test]
fn test_20260418_thread_local_ref_with_TP_ok() {
    let source = r#"
thread_local! {
    static TLS: i32 = 42;
}

fn rvs_read_tls_PT() -> i32 {
    TLS.with(|v| *v)
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert!(output.violations.is_empty());

    rvs_snapshot_BIP(
        "20260418_thread_local_ref_with_TP_ok",
        "violations: 0\n",
    );
}

#[test]
fn test_20260418_static_in_method_usage() {
    let source = r#"
static CACHE: i32 = 0;

struct Service;

impl Service {
    fn rvs_check_cache_E(&self) -> i32 {
        CACHE
    }
}
"#;
    let output = rvs_check_source_E(source, "test.rs", &CapsMap::rvs_new()).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::P));
    assert_eq!(output.violations[0].target, "CACHE");

    rvs_snapshot_BIP(
        "20260418_static_in_method_usage",
        format!("violations: {}\n{}\n", output.violations.len(), output.violations[0]),
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

fn rvs_read_BEI(_1: &str) -> Result<String, std::io::Error> {
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

    let fns = rivus_linter::mir::rvs_extract_from_mir_E(mir).unwrap();
    assert_eq!(fns.len(), 2);
    assert_eq!(fns[0].name, "rvs_add");
    assert!(fns[0].capabilities.rvs_is_empty());
    assert!(fns[0].calls.is_empty());

    assert_eq!(fns[1].name, "rvs_read_BEI");
    assert!(fns[1].capabilities.rvs_contains(Capability::B));
    assert!(fns[1].capabilities.rvs_contains(Capability::E));
    assert!(fns[1].capabilities.rvs_contains(Capability::I));
    assert_eq!(fns[1].calls.len(), 1);
    assert_eq!(fns[1].calls[0].name, "std::fs::read_to_string");

    rvs_snapshot_BIP(
        "20260419_mir_extract_rvs_functions",
        format!(
            "functions: {}\n1: {} caps={} calls={}\n2: {} caps={} calls={}\n  - {}\n",
            fns.len(),
            fns[0].name, fns[0].capabilities, fns[0].calls.len(),
            fns[1].name, fns[1].capabilities, fns[1].calls.len(),
            fns[1].calls[0].name,
        ),
    );
}

#[test]
fn test_20260419_mir_trait_dispatch() {
    let mir = r#"
fn rvs_process_BEI(_1: &str) -> Result<Vec<i32>, std::io::Error> {
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

    let fns = rivus_linter::mir::rvs_extract_from_mir_E(mir).unwrap();
    assert_eq!(fns.len(), 1);
    let func = &fns[0];
    assert_eq!(func.name, "rvs_process_BEI");

    let call_names: Vec<&str> = func.calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.iter().any(|n| n.contains("Iterator") && n.contains("map")));
    assert!(call_names.iter().any(|n| n.contains("Iterator") && n.contains("collect")));
    assert!(call_names.iter().any(|n| n.contains("core::str") && n.contains("lines")));

    rvs_snapshot_BIP(
        "20260419_mir_trait_dispatch",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            func.name,
            func.calls.len(),
            func.calls.iter().map(|c| format!("  - {}", c.name)).collect::<Vec<_>>().join("\n"),
        ),
    );
}

#[test]
fn test_20260419_mir_inherent_method() {
    let mir = r#"
fn rvs_init_E() -> HashMap<String, i32> {
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

    let fns = rivus_linter::mir::rvs_extract_from_mir_E(mir).unwrap();
    assert_eq!(fns.len(), 1);
    let func = &fns[0];

    let call_names: Vec<&str> = func.calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.iter().any(|n| n.contains("HashMap") && n.contains("new")));
    assert!(call_names.iter().any(|n| n.contains("Clone") && n.contains("clone")));

    rvs_snapshot_BIP(
        "20260419_mir_inherent_method",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            func.name,
            func.calls.len(),
            func.calls.iter().map(|c| format!("  - {}", c.name)).collect::<Vec<_>>().join("\n"),
        ),
    );
}

#[test]
fn test_20260419_mir_closures_skipped() {
    let mir = r#"
fn rvs_outer_AEI(_1: &str) -> Vec<i32> {
    bb0: {
        _3 = rvs_inner_E(copy _1) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}

fn rvs_outer_AEI::{closure#0}(_1: &mut {closure@src/main.rs:5:20: 5:23}, _2: &str) -> i32 {
    bb0: {
        _3 = core::str::<impl str>::len(copy _2) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;

    let fns = rivus_linter::mir::rvs_extract_from_mir_E(mir).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_outer_AEI");
    assert_eq!(fns[0].calls.len(), 2);

    let call_names: Vec<&str> = fns[0].calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.contains(&"rvs_inner_E"));
    assert!(call_names.iter().any(|n| n.contains("core::str") && n.contains("len")));

    rvs_snapshot_BIP(
        "20260419_mir_closures_skipped",
        format!(
            "functions: {}\nname: {}\ncalls: {}\n{}\n",
            fns.len(),
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls.iter().map(|c| format!("  - {}", c.name)).collect::<Vec<_>>().join("\n"),
        ),
    );
}

#[test]
fn test_20260419_mir_bare_path_calls() {
    let mir = r#"
fn rvs_process_E(_1: &str) -> Result<i32, ()> {
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

    let fns = rivus_linter::mir::rvs_extract_from_mir_E(mir).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_process_E");
    assert_eq!(fns[0].calls.len(), 2);

    let call_names: Vec<&str> = fns[0].calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.contains(&"parse_file"));
    assert!(call_names.contains(&"format"));

    rvs_snapshot_BIP(
        "20260419_mir_bare_path_calls",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls.iter().map(|c| format!("  - {}", c.name)).collect::<Vec<_>>().join("\n"),
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

    let fns = rivus_linter::mir::rvs_extract_from_mir_E(mir).unwrap();
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].name, "rvs_harvest_from_expr");
    assert_eq!(fns[0].calls.len(), 1);

    let call_names: Vec<&str> = fns[0].calls.iter().map(|c| c.name.as_str()).collect();
    assert!(call_names.iter().any(|n| n.contains("unwrap_or_else")));

    rvs_snapshot_BIP(
        "20260419_mir_unwrap_or_else_fn_ptr",
        format!(
            "name: {}\ncalls: {}\n{}\n",
            fns[0].name,
            fns[0].calls.len(),
            fns[0].calls.iter().map(|c| format!("  - {}", c.name)).collect::<Vec<_>>().join("\n"),
        ),
    );
}

// ─── MIR 目录级检查测试 ──────────────────────────────────

#[test]
fn test_20260419_mir_check_dir_compliant() {
    let mir = r#"
fn rvs_outer_ABEI(_1: &str) -> () {
    bb0: {
        _0 = rvs_inner_E(copy _1) -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}

fn rvs_inner_E(_1: &str) -> () {
    bb0: {
        return;
    }
}
"#;

    let dir = std::env::temp_dir().join("rivus_test_mir_compliant");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BEIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BIP(
        "20260419_mir_check_dir_compliant",
        "violations: 0\nwarnings: 0\n",
    );
}

#[test]
fn test_20260419_mir_check_dir_violation() {
    let mir = r#"
fn rvs_pure() -> () {
    bb0: {
        _0 = rvs_io_BEI() -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}

fn rvs_io_BEI() -> () {
    bb0: {
        return;
    }
}
"#;

    let dir = std::env::temp_dir().join("rivus_test_mir_violation");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.mir"), mir).unwrap();

    let cm = CapsMap::rvs_new();
    let output = rivus_linter::rvs_check_mir_dir_BEIM(&dir, &cm).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::B));
    assert!(output.violations[0].missing.contains(&Capability::E));
    assert!(output.violations[0].missing.contains(&Capability::I));

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BIP(
        "20260419_mir_check_dir_violation",
        format!("violations: {}\n{}\n", output.violations.len(), output.violations[0]),
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

    let cm = CapsMap::rvs_parse_E("heavy_io=BEI\n").unwrap();
    let output = rivus_linter::rvs_check_mir_dir_BEIM(&dir, &cm).unwrap();
    assert_eq!(output.violations.len(), 1);
    assert!(output.violations[0].missing.contains(&Capability::B));

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BIP(
        "20260419_mir_check_dir_with_capsmap",
        format!("violations: {}\n{}\n", output.violations.len(), output.violations[0]),
    );
}

#[test]
fn test_20260419_mir_check_dir_merge_multiple_files() {
    let mir1 = r#"
fn rvs_outer_E() -> () {
    bb0: {
        _0 = rvs_helper() -> [return: bb1, unwind continue];
    }

    bb1: {
        return;
    }
}
"#;

    let mir2 = r#"
fn rvs_outer_E(_1: i32) -> () {
    bb0: {
        _0 = rvs_other_E() -> [return: bb1, unwind continue];
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
    let output = rivus_linter::rvs_check_mir_dir_BEIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());

    let fns = {
        let sources = rivus_linter::source::rvs_read_mir_sources_BEI(&dir).unwrap();
        let mut all = Vec::new();
        for sf in &sources {
            if let Ok(fns) = rivus_linter::mir::rvs_extract_from_mir_E(&sf.source) {
                all.extend(fns);
            }
        }
        let mut map: std::collections::HashMap<String, rivus_linter::FnDef> = std::collections::HashMap::new();
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

    let outer = fns.iter().find(|f| f.name == "rvs_outer_E").unwrap();
    assert_eq!(outer.calls.len(), 2);

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BIP(
        "20260419_mir_check_dir_merge",
        format!(
            "violations: {}\nmerged calls for rvs_outer_E: {}\n  - {}\n  - {}\n",
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
    let output = rivus_linter::rvs_check_mir_dir_BEIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.warnings.is_empty());

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BIP(
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
    let output = rivus_linter::rvs_check_mir_dir_BEIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());
    assert!(output.warnings.is_empty());

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BIP(
        "20260419_mir_check_dir_empty_dir",
        "violations: 0\nwarnings: 0\n",
    );
}

#[test]
fn test_20260419_mir_check_dir_unknown_non_rvs_warning() {
    let mir = r#"
fn rvs_do_thing_E() -> () {
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
    let output = rivus_linter::rvs_check_mir_dir_BEIM(&dir, &cm).unwrap();
    assert!(output.violations.is_empty());
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(output.warnings[0].callee, "mystery_function");

    std::fs::remove_dir_all(&dir).unwrap();

    rvs_snapshot_BIP(
        "20260419_mir_check_dir_unknown_non_rvs_warning",
        format!("violations: 0\nwarnings: {}\n{}\n", output.warnings.len(), output.warnings[0]),
    );
}
