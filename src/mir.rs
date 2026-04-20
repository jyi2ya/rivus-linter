use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::capability::{parse_rvs_function, rvs_extract_raw_suffix};
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

    let has_mir = std::fs::read_dir(&deps_dir).ok().is_some_and(|entries| {
        entries
            .filter_map(|e| e.ok())
            .any(|e| e.path().extension().is_some_and(|ext| ext == "mir"))
    });

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
    parse_rvs_function(parent)?;
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
        if has_panic_pattern && !trimmed.contains("const \"") {
            return true;
        }
    }
    false
}

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
        } else if parse_rvs_function(&mir_fn.name).is_some() {
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
        let Some((_, caps)) = parse_rvs_function(&name) else {
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
        });
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));

    if result.is_empty() {
        return Err(MirError::NoRvsFunctions);
    }

    Ok(result)
}
