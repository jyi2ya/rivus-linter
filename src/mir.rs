use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::capability::{rvs_extract_raw_suffix, rvs_parse_function};
use crate::extract::{CalleeInfo, FnDef};

#[derive(Debug, thiserror::Error)]
pub enum MirError {
    #[error("no rvs_ functions found in MIR output")]
    NoRvsFunctions,
}

#[derive(Debug, thiserror::Error)]
pub enum MirCompileError {
    #[error("no Cargo.toml found in '{path}'")]
    NoCargoToml { path: String },
    #[error("cargo build failed:\n{stderr}")]
    BuildFailed { stderr: String },
    #[error("no .mir files found after compilation in '{path}'")]
    NoMirFiles { path: String },
    #[error("failed to spawn cargo: {0}")]
    SpawnFailed(String),
}

/// 调用 `cargo rustc` 编译项目并将 MIR 写出到 `target/mir-dump`，返回 MIR 目录路径。
///
/// # Panics
///
/// Panics if `project_dir` does not exist.
#[allow(non_snake_case)]
pub fn rvs_compile_to_mir_BIMPS(project_dir: &Path) -> Result<PathBuf, MirCompileError> {
    debug_assert!(project_dir.exists(), "项目目录必须存在");

    let cargo_toml = project_dir.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(MirCompileError::NoCargoToml {
            path: project_dir.display().to_string(),
        });
    }

    let output = std::process::Command::new("cargo")
        .args(["build"])
        .env("RUSTFLAGS", "--emit=mir")
        .current_dir(project_dir)
        .output()
        .map_err(|e| MirCompileError::SpawnFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MirCompileError::BuildFailed { stderr });
    }

    let deps_dir = project_dir.join("target").join("debug").join("deps");
    if !deps_dir.exists() {
        return Err(MirCompileError::NoMirFiles {
            path: deps_dir.display().to_string(),
        });
    }

    let dir_entries = match std::fs::read_dir(&deps_dir) {
        Ok(entries) => entries,
        Err(_) => {
            return Err(MirCompileError::NoMirFiles {
                path: deps_dir.display().to_string(),
            });
        }
    };
    let has_mir = dir_entries
        .filter_map(|e| match e {
            Ok(entry) => Some(entry),
            Err(_) => None,
        })
        .any(|e| e.path().extension().is_some_and(|ext| ext == "mir"));

    if !has_mir {
        return Err(MirCompileError::NoMirFiles {
            path: deps_dir.display().to_string(),
        });
    }

    Ok(deps_dir)
}

#[derive(Debug, Clone)]
struct MirFnDef {
    name: String,
    sig_line: String,
    body_lines: Vec<String>,
}

fn rvs_extract_calls_from_body(body_lines: &[String]) -> Vec<CalleeInfo> {
    let mut calls = Vec::new();
    for (i, line) in body_lines.iter().enumerate() {
        if let Some(target) = rvs_extract_call_target(line) {
            calls.push(CalleeInfo {
                name: target,
                line: i + 1,
            });
        }
    }
    calls
}

fn rvs_extract_parent_fn_name(closure_name: &str) -> Option<&str> {
    let brace_pos = closure_name.find("::{")?;
    let parent = &closure_name[..brace_pos];
    rvs_parse_function(parent)?;
    Some(parent)
}

fn rvs_strip_generics(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    let mut depth: i32 = 0;
    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ':' if depth == 0 && i + 1 < chars.len() && chars[i + 1] == ':' => {
                if i + 2 < chars.len() && chars[i + 2] == '<' {
                    i += 2;
                } else {
                    result.push(':');
                    result.push(':');
                    i += 2;
                }
            }
            '<' => {
                depth += 1;
                i += 1;
            }
            '>' if i > 0 && chars[i - 1] == '-' => {
                i += 1;
            }
            '>' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            _ if depth == 0 => {
                result.push(chars[i]);
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    result
}

fn rvs_find_call_open_paren(s: &str) -> Option<usize> {
    let mut angle_depth: i32 = 0;
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '<' => angle_depth += 1,
            '>' if i > 0 && chars[i - 1] == '-' => {}
            '>' => angle_depth -= 1,
            '(' if angle_depth == 0 => return Some(i),
            ' ' if angle_depth == 0 => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

fn rvs_find_matching_angle(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in s.chars().enumerate() {
        match ch {
            '<' => depth += 1,
            '>' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn rvs_extract_call_target(line: &str) -> Option<String> {
    let line = line.trim_start();
    if !line.contains("-> [") {
        return None;
    }
    let rest = line.strip_prefix("_")?;
    let digits_end = rest
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()?;
    let rest = &rest[digits_end.0 + digits_end.1.len_utf8()..];
    let rest = rest.strip_prefix(" = ")?;
    let rest = rest.strip_prefix("const ").unwrap_or(rest);

    if let Some(stripped) = rest.strip_prefix('<') {
        let end = rvs_find_matching_angle(stripped)?;
        let end_in_rest = end + 1;
        let inner = &rest[1..end_in_rest];
        let after_angle = &rest[end_in_rest..];

        if let Some(method_path) = after_angle.strip_prefix(">::") {
            let method_end = rvs_find_call_open_paren(method_path)
                .unwrap_or_else(|| method_path.find(' ').unwrap_or(method_path.len()));
            let method = &method_path[..method_end];
            let clean_method = rvs_strip_generics(method);

            if let Some(as_pos) = inner.find(" as ") {
                let trait_part = &inner[as_pos + 4..];
                let clean_trait = rvs_strip_generics(trait_part);
                Some(format!("{clean_trait}::{clean_method}"))
            } else {
                let sep = inner.rfind(">::")?;
                let type_part = rvs_strip_generics(&inner[..sep + 1]);
                Some(format!("{type_part}::{clean_method}"))
            }
        } else {
            None
        }
    } else if rest.contains("::") {
        let paren_pos = rvs_find_call_open_paren(rest)?;
        let path = &rest[..paren_pos];
        let clean = rvs_strip_generics(path);
        if clean.contains("::")
            || (!clean.is_empty() && clean.chars().all(|c| c.is_alphanumeric() || c == '_'))
        {
            Some(clean)
        } else {
            None
        }
    } else if rest.starts_with("rvs_") {
        let paren_pos =
            rvs_find_call_open_paren(rest).unwrap_or_else(|| rest.find(' ').unwrap_or(rest.len()));
        Some(rest[..paren_pos].to_string())
    } else if rvs_find_call_open_paren(rest).is_some() && rest.contains("->") {
        let paren_pos =
            rvs_find_call_open_paren(rest).unwrap_or_else(|| rest.find(' ').unwrap_or(rest.len()));
        if paren_pos > 0 {
            Some(rest[..paren_pos].to_string())
        } else {
            None
        }
    } else {
        None
    }
}

fn rvs_parse_mir_functions(mir_text: &str) -> Vec<MirFnDef> {
    let mut functions = Vec::new();
    let mut current_fn: Option<MirFnDef> = None;
    let mut brace_depth: usize = 0;

    for line in mir_text.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("fn ")
            && let Some(paren_pos) = rest.find('(')
        {
            let name = rest[..paren_pos].trim().to_string();
            brace_depth = 0;
            for ch in rest.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth = brace_depth.saturating_sub(1),
                    _ => {}
                }
            }
            if brace_depth == 0 {
                functions.push(MirFnDef {
                    name,
                    sig_line: trimmed.to_string(),
                    body_lines: Vec::new(),
                });
            } else {
                current_fn = Some(MirFnDef {
                    name,
                    sig_line: trimmed.to_string(),
                    body_lines: Vec::new(),
                });
            }
            continue;
        }

        if current_fn.is_some() {
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth = brace_depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }

            if brace_depth == 0 {
                if let Some(done) = current_fn.take() {
                    functions.push(done);
                }
            } else if let Some(fn_def) = current_fn.as_mut() {
                fn_def.body_lines.push(line.to_string());
            }
        }
    }

    functions
}

fn rvs_scan_mir_has_mut_param(mir_fn: &MirFnDef) -> bool {
    let is_closure = mir_fn.name.contains("::{closure#");
    if is_closure {
        return false;
    }
    mir_fn.sig_line.contains("&mut")
}

fn rvs_scan_mir_has_panic(mir_fn: &MirFnDef) -> bool {
    for line in &mir_fn.body_lines {
        let trimmed = line.trim();
        let has_panic_pattern = trimmed.contains("panicking::panic")
            || trimmed.contains("panicking::assert")
            || trimmed.contains("panicking::panic_fmt");
        if has_panic_pattern && !trimmed.contains("const \"") && !trimmed.contains("\"never:") {
            return true;
        }
    }
    false
}

/// 从 MIR 文本中萃取函数定义（函数体、调用关系、元信息）。
pub fn rvs_extract_from_mir(mir_text: &str) -> Result<Vec<FnDef>, MirError> {
    let mir_functions = rvs_parse_mir_functions(mir_text);

    let mut fn_calls: HashMap<String, Vec<CalleeInfo>> = HashMap::new();
    let mut fn_meta: HashMap<String, (bool, bool)> = HashMap::new();

    for mir_fn in &mir_functions {
        let has_mut_param = rvs_scan_mir_has_mut_param(mir_fn);
        let has_panic_macro = rvs_scan_mir_has_panic(mir_fn);
        let calls = rvs_extract_calls_from_body(&mir_fn.body_lines);

        if let Some(parent) = rvs_extract_parent_fn_name(&mir_fn.name) {
            fn_calls
                .entry(parent.to_string())
                .or_default()
                .extend(calls);
            let entry = fn_meta.entry(parent.to_string()).or_insert((false, false));
            entry.0 = entry.0 || has_mut_param;
            entry.1 = entry.1 || has_panic_macro;
        } else if rvs_parse_function(&mir_fn.name).is_some() {
            fn_calls
                .entry(mir_fn.name.clone())
                .or_default()
                .extend(calls);
            let entry = fn_meta.entry(mir_fn.name.clone()).or_insert((false, false));
            entry.0 = entry.0 || has_mut_param;
            entry.1 = entry.1 || has_panic_macro;
        }
    }

    let mut result = Vec::new();
    for (name, calls) in fn_calls {
        let Some((_, caps)) = rvs_parse_function(&name) else {
            continue;
        };
        let raw_suffix = rvs_extract_raw_suffix(&name);
        let (has_mut_param, has_panic_macro) = fn_meta.remove(&name).unwrap_or((false, false));
        result.push(FnDef {
            name,
            capabilities: caps,
            calls,
            static_refs: Vec::new(),
            line: 0,
            line_count: 0,
            params: Vec::new(),
            debug_asserted_params: BTreeSet::new(),
            has_body: true,
            has_unsafe_block: false,
            is_async_fn: false,
            is_unsafe_fn: false,
            has_mut_param,
            has_mut_self: false,
            has_panic_macro,
            raw_suffix,
            is_test: false,
            allows_dead_code: false,
            has_allow_non_snake_case: true,
        });
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));

    if result.is_empty() {
        return Err(MirError::NoRvsFunctions);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use crate::extract::FnDef;

    fn mir_fn(name: &str, sig_line: &str, body_lines: &[&str]) -> MirFnDef {
        MirFnDef {
            name: name.to_string(),
            sig_line: sig_line.to_string(),
            body_lines: body_lines.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn test_20260425_extract_calls_from_body_basic() {
        let line1 = "        _0 = core::panicking::panic(const \"explicit panic\" -> [1: return]) -> [return]";
        let line2 = "        _1 = rvs_helper() -> [2: bb1]";
        let line3 = "        _2 = alloc::vec::Vec::<i32>::new() -> [3: bb2]";
        let line4 = "        return";
        let body: Vec<String> = vec![
            line1.to_string(),
            line2.to_string(),
            line3.to_string(),
            line4.to_string(),
        ];
        let calls = rvs_extract_calls_from_body(&body);
        assert!(calls.iter().any(|c| c.name.contains("panicking::panic")));
        assert!(calls.iter().any(|c| c.name == "rvs_helper"));
        assert!(calls.iter().any(|c| c.name == "alloc::vec::Vec::new"));
    }

    #[test]
    fn test_20260425_extract_calls_from_body_empty() {
        let body: Vec<String> = vec!["return".to_string()];
        let calls = rvs_extract_calls_from_body(&body);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_20260425_extract_parent_fn_name_valid() {
        let name = "rivus_linter::check::rvs_check_source_M::{closure#0}";
        let result = rvs_extract_parent_fn_name(name);
        assert_eq!(result, Some("rivus_linter::check::rvs_check_source_M"));
    }

    #[test]
    fn test_20260425_extract_parent_fn_name_not_rvs() {
        let name = "rivus_linter::check::helper::{closure#0}";
        let result = rvs_extract_parent_fn_name(name);
        assert_eq!(result, None);
    }

    #[test]
    fn test_20260425_extract_parent_fn_name_no_closure() {
        let result = rvs_extract_parent_fn_name("rvs_foo_M");
        assert_eq!(result, None);
    }

    #[test]
    fn test_20260425_strip_generics_no_generics() {
        assert_eq!(rvs_strip_generics("std::fs::read"), "std::fs::read");
    }

    #[test]
    fn test_20260425_strip_generics_with_args() {
        assert_eq!(
            rvs_strip_generics("alloc::vec::Vec::<i32>::new"),
            "alloc::vec::Vec::new"
        );
    }

    #[test]
    fn test_20260425_strip_generics_nested() {
        assert_eq!(
            rvs_strip_generics("alloc::collections::BTreeMap::<String, Vec::<u8>>::insert"),
            "alloc::collections::BTreeMap::insert"
        );
    }

    #[test]
    fn test_20260425_strip_generics_arrow() {
        assert_eq!(
            rvs_strip_generics("core::result::Result::<T, E>::unwrap"),
            "core::result::Result::unwrap"
        );
    }

    #[test]
    fn test_20260425_find_call_open_paren_simple() {
        assert_eq!(rvs_find_call_open_paren("foo()"), Some(3));
    }

    #[test]
    fn test_20260425_find_call_open_paren_with_generics() {
        assert_eq!(rvs_find_call_open_paren("Vec::<i32>::new()"), Some(15));
    }

    #[test]
    fn test_20260425_find_call_open_paren_space_before() {
        assert_eq!(rvs_find_call_open_paren("foo bar("), None);
    }

    #[test]
    fn test_20260425_find_call_open_paren_no_paren() {
        assert_eq!(rvs_find_call_open_paren("foo"), None);
    }

    #[test]
    fn test_20260425_find_matching_angle_basic() {
        assert_eq!(rvs_find_matching_angle("abc>"), Some(3));
    }

    #[test]
    fn test_20260425_find_matching_angle_nested() {
        assert_eq!(rvs_find_matching_angle("a<b>>"), Some(4));
    }

    #[test]
    fn test_20260425_find_matching_angle_no_close() {
        assert_eq!(rvs_find_matching_angle("abc"), None);
    }

    #[test]
    fn test_20260425_extract_call_target_simple_call() {
        let line = "        _1 = rvs_helper() -> [2: bb1]";
        assert_eq!(
            rvs_extract_call_target(line),
            Some("rvs_helper".to_string())
        );
    }

    #[test]
    fn test_20260425_extract_call_target_qualified() {
        let line = "        _2 = alloc::vec::Vec::<i32>::new() -> [3: bb2]";
        assert_eq!(
            rvs_extract_call_target(line),
            Some("alloc::vec::Vec::new".to_string())
        );
    }

    #[test]
    fn test_20260425_extract_call_target_panic() {
        let line = "        _0 = core::panicking::panic(const \"msg\" -> [1: return]) -> [return]";
        assert_eq!(
            rvs_extract_call_target(line),
            Some("core::panicking::panic".to_string())
        );
    }

    #[test]
    fn test_20260425_extract_call_target_return() {
        let line = "        return";
        assert_eq!(rvs_extract_call_target(line), None);
    }

    #[test]
    fn test_20260425_extract_call_target_trait_method() {
        let line = "        _3 = <std::io::Error as core::fmt::Display>::fmt(move _4, move _5) -> [6: bb3]";
        assert_eq!(
            rvs_extract_call_target(line),
            Some("core::fmt::Display::fmt".to_string())
        );
    }

    #[test]
    fn test_20260425_parse_mir_functions_single() {
        let mir = "fn rvs_foo_M() -> () {\n    _0 = rvs_bar() -> [1: return]\n    return\n}\n";
        let fns = rvs_parse_mir_functions(mir);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].name, "rvs_foo_M");
        assert_eq!(fns[0].body_lines.len(), 2);
    }

    #[test]
    fn test_20260425_parse_mir_functions_multiple() {
        let mir = "fn rvs_foo() -> () {\n    return\n}\n\nfn rvs_bar_M() -> () {\n    return\n}\n";
        let fns = rvs_parse_mir_functions(mir);
        assert_eq!(fns.len(), 2);
        assert_eq!(fns[0].name, "rvs_foo");
        assert_eq!(fns[1].name, "rvs_bar_M");
    }

    #[test]
    fn test_20260425_parse_mir_functions_empty() {
        let fns = rvs_parse_mir_functions("");
        assert!(fns.is_empty());
    }

    #[test]
    fn test_20260425_scan_mir_has_mut_param_true() {
        let f = mir_fn(
            "rvs_foo_M",
            "fn rvs_foo_M(_1: &mut i32) -> () {",
            &["    return"],
        );
        assert!(rvs_scan_mir_has_mut_param(&f));
    }

    #[test]
    fn test_20260425_scan_mir_has_mut_param_false() {
        let f = mir_fn("rvs_foo", "fn rvs_foo(_1: i32) -> () {", &["    return"]);
        assert!(!rvs_scan_mir_has_mut_param(&f));
    }

    #[test]
    fn test_20260425_scan_mir_has_mut_param_closure_skipped() {
        let f = mir_fn(
            "rvs_foo_M::{closure#0}",
            "fn rvs_foo_M::{closure#0}(_1: &mut i32) -> () {",
            &["    return"],
        );
        assert!(!rvs_scan_mir_has_mut_param(&f));
    }

    #[test]
    fn test_20260425_scan_mir_has_panic_true() {
        let f = mir_fn(
            "rvs_foo_P",
            "fn rvs_foo_P() -> () {",
            &["    _0 = core::panicking::panic(move _1, move _2) -> [return]"],
        );
        assert!(rvs_scan_mir_has_panic(&f));
    }

    #[test]
    fn test_20260425_scan_mir_has_panic_assert() {
        let f = mir_fn(
            "rvs_check_P",
            "fn rvs_check_P() -> () {",
            &["    _0 = core::panicking::assert(move _1, move _2) -> [return]"],
        );
        assert!(rvs_scan_mir_has_panic(&f));
    }

    #[test]
    fn test_20260425_scan_mir_has_panic_false() {
        let f = mir_fn("rvs_foo", "fn rvs_foo() -> () {", &["    return"]);
        assert!(!rvs_scan_mir_has_panic(&f));
    }

    #[test]
    fn test_20260425_scan_mir_has_panic_fmt() {
        let f = mir_fn(
            "rvs_foo_P",
            "fn rvs_foo_P() -> () {",
            &["    _0 = core::panicking::panic_fmt(move _1, move _2) -> [return]"],
        );
        assert!(rvs_scan_mir_has_panic(&f));
    }

    #[test]
    fn test_20260425_scan_mir_has_panic_never_expect_ok() {
        let f = mir_fn(
            "rvs_foo",
            "fn rvs_foo() -> () {",
            &["    _0 = core::panicking::panic(\"never: valid utf-8\") -> [return]"],
        );
        assert!(!rvs_scan_mir_has_panic(&f));
    }

    #[test]
    fn test_20260425_scan_mir_has_panic_expect_still_panic() {
        let f = mir_fn(
            "rvs_foo_P",
            "fn rvs_foo_P() -> () {",
            &["    _0 = core::panicking::panic(\"something went wrong\") -> [return]"],
        );
        assert!(rvs_scan_mir_has_panic(&f));
    }

    #[test]
    fn test_20260425_extract_from_mir_basic() {
        let mir = "\
fn rvs_foo_M() -> () {
    _0 = rvs_bar() -> [1: return]
    return
}
";
        let result = rvs_extract_from_mir(mir).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "rvs_foo_M");
        assert!(result[0].has_body);
        assert!(result[0].calls.iter().any(|c| c.name == "rvs_bar"));
    }

    #[test]
    fn test_20260425_extract_from_mir_with_closure() {
        let mir = "\
fn rvs_process_AI() -> () {
    _0 = rvs_read_file_BI() -> [1: bb1]
    return
}

fn rvs_process_AI::{closure#0}() -> () {
    _0 = core::panicking::panic(move _1, move _2) -> [return]
    return
}
";
        let result = rvs_extract_from_mir(mir).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "rvs_process_AI");
        assert!(result[0].has_panic_macro);
    }

    #[test]
    fn test_20260425_extract_from_mir_mut_param() {
        let mir = "\
fn rvs_modify_M(_1: &mut i32) -> () {
    return
}
";
        let result = rvs_extract_from_mir(mir).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].has_mut_param);
    }

    #[test]
    fn test_20260425_extract_from_mir_no_rvs_functions() {
        let mir = "\
fn helper() -> () {
    return
}
";
        let result = rvs_extract_from_mir(mir);
        assert!(result.is_err());
    }

    #[test]
    fn test_20260425_extract_from_mir_multiple() {
        let mir = "\
fn rvs_foo() -> () {
    _0 = rvs_bar() -> [1: return]
    return
}

fn rvs_bar_M() -> () {
    return
}
";
        let result = rvs_extract_from_mir(mir).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "rvs_bar_M");
        assert_eq!(result[1].name, "rvs_foo");
    }
}
