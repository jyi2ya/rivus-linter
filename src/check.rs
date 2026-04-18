use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::capability::{parse_rvs_function, Capability, CapabilitySet};
use crate::capsmap::CapsMap;
use crate::extract::{rvs_extract_functions_E, FnDef};
use crate::source::rvs_read_rust_sources_BEI;

/// 违规之别：调用越权与静态引用越权。
#[derive(Debug, Clone, PartialEq)]
pub enum ViolationKind {
    Call,
    StaticRef,
}

impl fmt::Display for ViolationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViolationKind::Call => write!(f, "calls"),
            ViolationKind::StaticRef => write!(f, "references"),
        }
    }
}

/// 一条违规：谁做了什么，差了什么。
#[derive(Debug, Clone)]
pub struct Violation {
    pub kind: ViolationKind,
    pub caller: String,
    pub caller_caps: CapabilitySet,
    pub target: String,
    pub target_caps: CapabilitySet,
    pub missing: BTreeSet<Capability>,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let missing_str = self
            .missing
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        write!(
            f,
            "error: {} {} {} but is missing capabilities [{}]\n  at {}:{}\n  caller has: {}\n  target needs: {}",
            self.caller,
            self.kind,
            self.target,
            missing_str,
            self.file,
            self.line,
            self.caller_caps,
            self.target_caps,
        )
    }
}

/// 一条警告：调用了一个既非 rvs_ 亦不在册的函数。
#[derive(Debug, Clone)]
pub struct Warning {
    pub caller: String,
    pub callee: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: {} calls {} which is neither rvs_-prefixed nor in capsmap\n  at {}:{}",
            self.caller, self.callee, self.file, self.line,
        )
    }
}

/// 检查结果：违规与警告。
#[derive(Debug, Clone)]
pub struct CheckOutput {
    pub violations: Vec<Violation>,
    pub warnings: Vec<Warning>,
}

/// 内部实现：检查函数调用合规性与静态引用合规性。
fn rvs_check_functions_impl(
    functions: &[FnDef],
    file: &str,
    capsmap: &CapsMap,
) -> CheckOutput {
    let mut violations = Vec::new();
    let mut warnings = Vec::new();

    for func in functions {
        for call in &func.calls {
            let callee_caps = match parse_rvs_function(&call.name) {
                Some((_, caps)) => caps,
                None => {
                    if let Some(caps) = capsmap.rvs_lookup(&call.name) {
                        caps.clone()
                    } else {
                        warnings.push(Warning {
                            caller: func.name.clone(),
                            callee: call.name.clone(),
                            file: file.to_string(),
                            line: call.line,
                        });
                        continue;
                    }
                }
            };
            let missing = func.capabilities.rvs_missing_for(&callee_caps);

            if !missing.is_empty() {
                violations.push(Violation {
                    kind: ViolationKind::Call,
                    caller: func.name.clone(),
                    caller_caps: func.capabilities.clone(),
                    target: call.name.clone(),
                    target_caps: callee_caps,
                    missing,
                    file: file.to_string(),
                    line: call.line,
                });
            }
        }

        for sr in &func.static_refs {
            let missing = func.capabilities.rvs_missing_for(&sr.required_caps);

            if !missing.is_empty() {
                violations.push(Violation {
                    kind: ViolationKind::StaticRef,
                    caller: func.name.clone(),
                    caller_caps: func.capabilities.clone(),
                    target: sr.name.clone(),
                    target_caps: sr.required_caps.clone(),
                    missing,
                    file: file.to_string(),
                    line: sr.line,
                });
            }
        }
    }

    CheckOutput { violations, warnings }
}

/// 纯函数：检查一组函数定义中的调用合规性与静态引用合规性。
pub fn rvs_check_functions(functions: &[FnDef], file: &str) -> Vec<Violation> {
    rvs_check_functions_impl(functions, file, &CapsMap::rvs_new()).violations
}

/// 从一段源码文本中检查违规，配合 CapsMap。
#[allow(non_snake_case)]
pub fn rvs_check_source_E(
    source: &str,
    file: &str,
    capsmap: &CapsMap,
) -> Result<CheckOutput, CheckError> {
    let functions = rvs_extract_functions_E(source)
        .map_err(|e| CheckError::Extract {
            source: e,
            file: file.to_string(),
        })?;
    Ok(rvs_check_functions_impl(&functions, file, capsmap))
}

/// 从文件路径（或目录）出发，检查违规。
/// CapsMap 用于查找非 rvs_ 函数的能力。
#[allow(non_snake_case)]
pub fn rvs_check_path_BEI(path: &Path, capsmap: &CapsMap) -> Result<CheckOutput, CheckError> {
    let sources = rvs_read_rust_sources_BEI(path)
        .map_err(|e| CheckError::Read { source: e })?;
    let mut output = CheckOutput {
        violations: Vec::new(),
        warnings: Vec::new(),
    };
    for sf in &sources {
        let functions = rvs_extract_functions_E(&sf.source)
            .map_err(|e| CheckError::Extract {
                source: e,
                file: sf.path.clone(),
            })?;
        let result = rvs_check_functions_impl(&functions, &sf.path, capsmap);
        output.violations.extend(result.violations);
        output.warnings.extend(result.warnings);
    }
    Ok(output)
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("failed to read: {source}")]
    Read {
        source: crate::source::ReadError,
    },
    #[error("failed to extract from '{file}': {source}")]
    Extract {
        file: String,
        source: crate::extract::ExtractError,
    },
}
