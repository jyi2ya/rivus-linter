use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use crate::capsmap::CapsMap;
use crate::capability::{parse_rvs_function, Capability, CapabilitySet};
use crate::extract::{rvs_extract_functions_E, FnDef};
use crate::source::{rvs_read_rust_sources_BEI, SourceFile};

/// 一条违规：谁调了谁，差了什么。
///
/// 如一封讼状：原告（caller）越权调用被告（callee），
/// 所缺之能力，白纸黑字，历历在目。
#[derive(Debug, Clone)]
pub struct Violation {
    pub caller: String,
    pub caller_caps: CapabilitySet,
    pub callee: String,
    pub callee_caps: CapabilitySet,
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
            "error: {} calls {} but is missing capabilities [{}]\n  at {}:{}\n  caller has: {}\n  callee needs: {}",
            self.caller,
            self.callee,
            missing_str,
            self.file,
            self.line,
            self.caller_caps,
            self.callee_caps,
        )
    }
}

/// 一条警告：调了不知底细的函数。
/// 不以 rvs 起首，册中又无备案，来路不明，需提防。
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
            "warning: {} calls '{}' which is not rvs_ and not in capsmap\n  at {}:{}",
            self.caller, self.callee, self.file, self.line,
        )
    }
}

/// 检查之 verdict：违规与警告，各列各的。
#[derive(Debug, Clone, Default)]
pub struct CheckOutput {
    pub violations: Vec<Violation>,
    pub warnings: Vec<Warning>,
}

/// 名中有 rvs，即为自家函数。
/// 取尾段判之，以应对 `module::rvs_foo_E` 之路径调用。
fn rvs_is_rvs_name(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .unwrap_or(name)
        .starts_with("rvs_")
}

/// 纯函数：检查一组函数定义中的调用合规性。
/// 有则记过，无则放行，不打折扣。
pub fn rvs_check_functions(functions: &[FnDef], file: &str, capsmap: &CapsMap) -> CheckOutput {
    let mut violations = Vec::new();
    let mut warnings = Vec::new();

    for func in functions {
        for call in &func.calls {
            if rvs_is_rvs_name(&call.name) {
                let last_seg = call.name.rsplit("::").next().unwrap();
                let (_, callee_caps) = parse_rvs_function(last_seg)
                    .expect("rvs_ callee suffix parse must succeed");
                let missing = func.capabilities.rvs_missing_for(&callee_caps);

                if !missing.is_empty() {
                    violations.push(Violation {
                        caller: func.name.clone(),
                        caller_caps: func.capabilities.clone(),
                        callee: call.name.clone(),
                        callee_caps,
                        missing,
                        file: file.to_string(),
                        line: call.line,
                    });
                }
            } else {
                match capsmap.rvs_lookup(&call.name) {
                    Some(callee_caps) => {
                        let missing = func.capabilities.rvs_missing_for(callee_caps);
                        if !missing.is_empty() {
                            violations.push(Violation {
                                caller: func.name.clone(),
                                caller_caps: func.capabilities.clone(),
                                callee: call.name.clone(),
                                callee_caps: callee_caps.clone(),
                                missing,
                                file: file.to_string(),
                                line: call.line,
                            });
                        }
                    }
                    None => {
                        warnings.push(Warning {
                            caller: func.name.clone(),
                            callee: call.name.clone(),
                            file: file.to_string(),
                            line: call.line,
                        });
                    }
                }
            }
        }
    }

    CheckOutput {
        violations,
        warnings,
    }
}

/// 从一段源码文本中检查违规。
/// 可能失败：解析出错便报错。
#[allow(non_snake_case)]
pub fn rvs_check_source_E(
    source: &str,
    file: &str,
    capsmap: &CapsMap,
) -> Result<CheckOutput, CheckError> {
    let functions = rvs_extract_functions_E(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    Ok(rvs_check_functions(&functions, file, capsmap))
}

/// 从多个已读入的源文件中检查违规。
/// 可能失败：解析出错便报错。无 IO，干干净净。
#[allow(non_snake_case)]
pub fn rvs_check_sources_E(
    sources: &[SourceFile],
    capsmap: &CapsMap,
) -> Result<CheckOutput, CheckError> {
    let mut output = CheckOutput::default();
    for sf in sources {
        let result = rvs_check_source_E(&sf.source, &sf.path, capsmap)?;
        output.violations.extend(result.violations);
        output.warnings.extend(result.warnings);
    }
    Ok(output)
}

/// 从文件路径（或目录）出发，检查违规。
/// 薄薄一层壳：只管读文件，真正的事交给纯函数。
#[allow(non_snake_case)]
pub fn rvs_check_path_BEI(
    path: &Path,
    capsmap: &CapsMap,
) -> Result<CheckOutput, CheckError> {
    let sources = rvs_read_rust_sources_BEI(path)
        .map_err(|e| CheckError::Read { source: e })?;
    rvs_check_sources_E(&sources, capsmap)
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
