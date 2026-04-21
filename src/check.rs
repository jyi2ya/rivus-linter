use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use crate::capability::{Capability, CapabilitySet, rvs_parse_function};
use crate::capsmap::CapsMap;
use crate::extract::{
    BorrowedParamInfo, ConsumedArgOnErrorInfo, DerefPolymorphismInfo, EmptyFnInfo, FnDef,
    ImportInfo, IntoImplInfo, MissingDebugInfo, MissingPanicsDocInfo, NonRvsFnInfo, PubItemInfo,
    ReflectionUsageInfo, StubMacroInfo, TestName, TodoCommentInfo, UnsafeFnInfo,
    rvs_extract_borrowed_params, rvs_extract_consumed_arg_on_error, rvs_extract_deny_warnings,
    rvs_extract_deref_polymorphism, rvs_extract_empty_fns, rvs_extract_functions,
    rvs_extract_imports, rvs_extract_into_impls, rvs_extract_missing_debug,
    rvs_extract_missing_panics_doc, rvs_extract_non_rvs_fns, rvs_extract_pub_items,
    rvs_extract_reflection_usage, rvs_extract_stub_macros, rvs_extract_test_call_names,
    rvs_extract_tests, rvs_extract_todo_comments, rvs_extract_unsafe_fns,
};
use crate::source::rvs_read_rust_sources_BI;

/// 被禁用的 crate 列表。
const BANNED_CRATES: &[&str] = &["anyhow", "eyre", "color_eyre"];

/// 纯函数：检查导入列表中是否包含被禁 crate。
/// 返回所有被禁导入的警告。
pub fn rvs_check_imports(imports: &[ImportInfo], file: &str) -> Vec<BannedImportWarning> {
    let mut warnings = Vec::new();
    for imp in imports {
        let first_segment = imp.use_path.split("::").next().unwrap_or("");
        for &banned in BANNED_CRATES {
            if first_segment == banned {
                warnings.push(BannedImportWarning {
                    crate_name: banned.to_string(),
                    use_path: imp.use_path.clone(),
                    file: file.to_string(),
                    line: imp.line,
                });
                break;
            }
        }
    }
    warnings
}

/// 纯函数：检查导入列表中是否有 wildcard import（`use xxx::*;`）。
/// 例外：`use super::*;` 和 `use *::prelude::*;`。
fn rvs_check_wildcard_imports(imports: &[ImportInfo], file: &str) -> Vec<WildcardImportWarning> {
    imports
        .iter()
        .filter(|imp| rvs_is_banned_wildcard(&imp.use_path))
        .map(|imp| WildcardImportWarning {
            use_path: imp.use_path.clone(),
            file: file.to_string(),
            line: imp.line,
        })
        .collect()
}

/// 判断 use 路径是否为被禁的 wildcard import。
/// 允许 `super::*` 和 `*::prelude::*`。
fn rvs_is_banned_wildcard(use_path: &str) -> bool {
    if !use_path.contains('*') {
        return false;
    }
    let normalized = use_path.replace(' ', "");
    if normalized == "super::*" {
        return false;
    }
    if normalized.contains("::prelude::*") {
        return false;
    }
    true
}

/// 纯函数：检查函数参数中是否有 `&String`/`&Vec<T>`/`&Box<T>` 借用类型。
fn rvs_check_borrowed_params(
    params: &[BorrowedParamInfo],
    file: &str,
) -> Vec<BorrowedParamWarning> {
    params
        .iter()
        .map(|p| BorrowedParamWarning {
            function: p.function.clone(),
            param: p.param.clone(),
            original: p.original.clone(),
            suggestion: p.suggestion.clone(),
            file: file.to_string(),
            line: p.line,
        })
        .collect()
}

/// 纯函数：检查 unsafe 函数是否缺少 `/// # Safety` 文档。
fn rvs_check_unsafe_safety_doc(fns: &[UnsafeFnInfo], file: &str) -> Vec<MissingSafetyDocWarning> {
    fns.iter()
        .filter(|f| !f.has_safety_doc)
        .map(|f| MissingSafetyDocWarning {
            function: f.name.clone(),
            file: file.to_string(),
            line: f.line,
        })
        .collect()
}

/// 纯函数：检查文件级 `#![deny(warnings)]` 反模式。
fn rvs_check_deny_warnings(line: Option<usize>, file: &str) -> Vec<DenyWarningsWarning> {
    line.map(|l| {
        vec![DenyWarningsWarning {
            file: file.to_string(),
            line: l,
        }]
    })
    .unwrap_or_default()
}

fn rvs_check_missing_debug(items: &[MissingDebugInfo], file: &str) -> Vec<MissingDebugWarning> {
    items
        .iter()
        .map(|i| MissingDebugWarning {
            name: i.name.clone(),
            file: file.to_string(),
            line: i.line,
        })
        .collect()
}

fn rvs_check_missing_panics_doc(
    items: &[MissingPanicsDocInfo],
    file: &str,
) -> Vec<MissingPanicsDocWarning> {
    items
        .iter()
        .map(|i| MissingPanicsDocWarning {
            function: i.function.clone(),
            file: file.to_string(),
            line: i.line,
        })
        .collect()
}

fn rvs_check_into_impls(items: &[IntoImplInfo], file: &str) -> Vec<IntoImplWarning> {
    items
        .iter()
        .map(|i| IntoImplWarning {
            impl_type: i.impl_type.clone(),
            target_type: i.target_type.clone(),
            file: file.to_string(),
            line: i.line,
        })
        .collect()
}

fn rvs_check_consumed_arg_on_error(
    items: &[ConsumedArgOnErrorInfo],
    file: &str,
) -> Vec<ConsumedArgOnErrorWarning> {
    items
        .iter()
        .map(|i| ConsumedArgOnErrorWarning {
            function: i.function.clone(),
            param: i.param.clone(),
            param_type: i.param_type.clone(),
            file: file.to_string(),
            line: i.line,
        })
        .collect()
}

fn rvs_check_deref_polymorphism(
    items: &[DerefPolymorphismInfo],
    file: &str,
) -> Vec<DerefPolymorphismWarning> {
    items
        .iter()
        .map(|i| DerefPolymorphismWarning {
            impl_type: i.impl_type.clone(),
            target_type: i.target_type.clone(),
            file: file.to_string(),
            line: i.line,
        })
        .collect()
}

fn rvs_check_reflection_usage(
    items: &[ReflectionUsageInfo],
    file: &str,
) -> Vec<ReflectionUsageWarning> {
    items
        .iter()
        .map(|i| ReflectionUsageWarning {
            function: i.function.clone(),
            path: i.path.clone(),
            file: file.to_string(),
            line: i.line,
        })
        .collect()
}

fn rvs_check_stub_macros(items: &[StubMacroInfo], file: &str) -> Vec<Violation> {
    items
        .iter()
        .map(|i| Violation {
            kind: ViolationKind::StubMacro {
                macro_name: i.macro_name.clone(),
            },
            caller: i.function.clone(),
            caller_caps: CapabilitySet::rvs_new(),
            target: i.macro_name.clone(),
            target_caps: CapabilitySet::rvs_new(),
            missing: BTreeSet::new(),
            file: file.to_string(),
            line: i.line,
        })
        .collect()
}

fn rvs_check_empty_fns(items: &[EmptyFnInfo], file: &str) -> Vec<Violation> {
    items
        .iter()
        .map(|i| Violation {
            kind: ViolationKind::EmptyFn,
            caller: i.function.clone(),
            caller_caps: CapabilitySet::rvs_new(),
            target: String::new(),
            target_caps: CapabilitySet::rvs_new(),
            missing: BTreeSet::new(),
            file: file.to_string(),
            line: i.line,
        })
        .collect()
}

fn rvs_check_todo_comments(items: &[TodoCommentInfo], file: &str) -> Vec<TodoCommentWarning> {
    items
        .iter()
        .map(|i| TodoCommentWarning {
            kind: i.kind.clone(),
            text: i.text.clone(),
            file: file.to_string(),
            line: i.line,
        })
        .collect()
}

fn rvs_check_untested_good_fns(
    functions: &[FnDef],
    test_call_names: &[String],
    file: &str,
) -> Vec<UntestedGoodFnWarning> {
    let good_allowed = CapabilitySet::rvs_from_good_caps();
    functions
        .iter()
        .filter(|f| {
            f.capabilities.rvs_is_subset_of(&good_allowed) && !f.allows_dead_code && !f.is_test
        })
        .filter(|f| {
            let name = f.name.as_str();
            !test_call_names
                .iter()
                .any(|tc| tc == name || tc.ends_with(&format!("::{name}")))
        })
        .map(|f| UntestedGoodFnWarning {
            function: f.name.clone(),
            file: file.to_string(),
            line: f.line,
        })
        .collect()
}

/// 纯函数：检查函数列表中是否缺少 rvs_ 前缀。
/// 返回所有缺少前缀的函数警告。
fn rvs_check_non_rvs_fn_names(non_rvs_fns: &[NonRvsFnInfo], file: &str) -> Vec<NonRvsFnWarning> {
    let mut warnings = Vec::new();
    for func in non_rvs_fns {
        if !func.has_rvs_prefix {
            warnings.push(NonRvsFnWarning {
                function: func.name.clone(),
                file: file.to_string(),
                line: func.line,
            });
        }
    }
    warnings
}

/// 纯函数：检查 pub 函数/方法列表中是否缺少文档注释。
pub fn rvs_check_missing_doc(pubs: &[PubItemInfo], file: &str) -> Vec<MissingDocWarning> {
    let mut warnings = Vec::new();
    for item in pubs {
        if !item.has_doc {
            warnings.push(MissingDocWarning {
                item: item.name.clone(),
                file: file.to_string(),
                line: item.line,
            });
        }
    }
    warnings
}

/// 违规之别。
#[derive(Debug, Clone, PartialEq)]
pub enum ViolationKind {
    Call,
    StaticRef,
    StubMacro { macro_name: String },
    EmptyFn,
}

impl fmt::Display for ViolationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViolationKind::Call => write!(f, "calls"),
            ViolationKind::StaticRef => write!(f, "references"),
            ViolationKind::StubMacro { macro_name } => {
                write!(f, "contains {macro_name}!() stub macro")
            }
            ViolationKind::EmptyFn => write!(f, "has empty body (no logic beyond debug_assert)"),
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
        match &self.kind {
            ViolationKind::StubMacro { .. } | ViolationKind::EmptyFn => {
                write!(
                    f,
                    "error: {} {} \n  at {}:{}",
                    self.caller, self.kind, self.file, self.line,
                )
            }
            ViolationKind::Call | ViolationKind::StaticRef => {
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

/// 一条被禁导入警告：使用了 anyhow、eyre 或 color_eyre 等被禁 crate。
#[derive(Debug, Clone)]
pub struct BannedImportWarning {
    pub crate_name: String,
    pub use_path: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for BannedImportWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: banned crate '{}' imported via '{}'\n  at {}:{}",
            self.crate_name, self.use_path, self.file, self.line,
        )
    }
}

/// 一条私有函数命名警告：非 rvs_ 前缀的私有函数。
/// 函数应以 rvs_ 开头以便追踪能力标记。
#[derive(Debug, Clone)]
pub struct NonRvsFnWarning {
    pub function: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for NonRvsFnWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: function '{}' should have rvs_ prefix\n  at {}:{}",
            self.function, self.file, self.line,
        )
    }
}

/// 一条公开 API 缺文档警告：pub 函数/方法没有 `///` 或 `#[doc]` 注释。
#[derive(Debug, Clone)]
pub struct MissingDocWarning {
    pub item: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for MissingDocWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: public item '{}' is missing a doc comment (///)\n  at {}:{}",
            self.item, self.file, self.line,
        )
    }
}

/// 检查结果：违规、警告、缺断言警告、死代码警告、推断警告、
/// 缺 `#[allow(non_snake_case)]` 警告、测试命名格式警告、测试命名重复警告、
/// 被禁导入警告、私有函数命名警告、缺文档警告、`#![deny(warnings)]` 反模式警告、
/// wildcard import 警告、unsafe fn 缺 safety 文档警告、借用类型参数建议警告。
#[derive(Debug, Clone, Default)]
pub struct CheckOutput {
    pub violations: Vec<Violation>,
    pub warnings: Vec<Warning>,
    pub assert_warnings: Vec<MissingAssertWarning>,
    pub dead_code_warnings: Vec<DeadCodeWarning>,
    pub inference_warnings: Vec<InferenceWarning>,
    pub missing_allow_warnings: Vec<MissingAllowWarning>,
    pub test_name_warnings: Vec<TestNameFormatWarning>,
    pub duplicate_test_warnings: Vec<DuplicateTestWarning>,
    pub banned_import_warnings: Vec<BannedImportWarning>,
    pub non_rvs_fn_warnings: Vec<NonRvsFnWarning>,
    pub missing_doc_warnings: Vec<MissingDocWarning>,
    pub deny_warnings_warnings: Vec<DenyWarningsWarning>,
    pub wildcard_import_warnings: Vec<WildcardImportWarning>,
    pub missing_safety_doc_warnings: Vec<MissingSafetyDocWarning>,
    pub borrowed_param_warnings: Vec<BorrowedParamWarning>,
    pub missing_debug_warnings: Vec<MissingDebugWarning>,
    pub missing_panics_doc_warnings: Vec<MissingPanicsDocWarning>,
    pub into_impl_warnings: Vec<IntoImplWarning>,
    pub consumed_arg_on_error_warnings: Vec<ConsumedArgOnErrorWarning>,
    pub deref_polymorphism_warnings: Vec<DerefPolymorphismWarning>,
    pub reflection_usage_warnings: Vec<ReflectionUsageWarning>,
    pub todo_comment_warnings: Vec<TodoCommentWarning>,
    pub untested_good_fn_warnings: Vec<UntestedGoodFnWarning>,
}

/// 一条 `#![deny(warnings)]` 反模式警告：这种粗粒度 deny 会随编译器升级意外破坏构建。
/// 建议改用具名 lint，如 `#![deny(dead_code, unused_imports)]`。
#[derive(Debug, Clone)]
pub struct DenyWarningsWarning {
    pub file: String,
    pub line: usize,
}

impl fmt::Display for DenyWarningsWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: `#![deny(warnings)]` is an anti-pattern—use named lints instead\n  at {}:{}",
            self.file, self.line,
        )
    }
}

/// 一条 wildcard import 警告：`use xxx::*;` 易与未来版本命名冲突。
/// 例外：`use super::*;`（测试常用）、`use *::prelude::*;`（作者刻意暴露）。
#[derive(Debug, Clone)]
pub struct WildcardImportWarning {
    pub use_path: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for WildcardImportWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: wildcard import '{}' may cause name clashes with future versions\n  at {}:{}",
            self.use_path, self.file, self.line,
        )
    }
}

/// 一条 unsafe 函数缺 `/// # Safety` 文档警告。
/// unsafe 函数的前置条件必须显式记录，否则调用者无法安全使用。
#[derive(Debug, Clone)]
pub struct MissingSafetyDocWarning {
    pub function: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for MissingSafetyDocWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: unsafe fn '{}' is missing a `/// # Safety` doc section\n  at {}:{}",
            self.function, self.file, self.line,
        )
    }
}

/// 一条借用类型参数建议：`&String`/`&Vec<T>`/`&Box<T>` 应改为 `&str`/`&[T]`/`&T`。
/// 借用类型更灵活——能接受更多调用者类型，也消除多层间接。
#[derive(Debug, Clone)]
pub struct BorrowedParamWarning {
    pub function: String,
    pub param: String,
    pub original: String,
    pub suggestion: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for BorrowedParamWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: param '{}' of '{}' uses '{}'—prefer '{}'\n  at {}:{}",
            self.param, self.function, self.original, self.suggestion, self.file, self.line,
        )
    }
}

/// 一条公开类型缺 `Debug` derive 警告：pub struct/enum 未派生 `Debug`。
/// `Debug` 是日志和错误报告的基础，公开类型几乎总是应该实现它。
#[derive(Debug, Clone)]
pub struct MissingDebugWarning {
    pub name: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for MissingDebugWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: public type '{}' is missing #[derive(Debug)]\n  at {}:{}",
            self.name, self.file, self.line,
        )
    }
}

/// 一条 `rvs_` 函数带 `P` 标记但缺少 `/// # Panics` 文档警告。
/// 与 `unsafe fn` 需要 `/// # Safety` 对称——可能 panic 的函数应文档化其 panic 条件。
#[derive(Debug, Clone)]
pub struct MissingPanicsDocWarning {
    pub function: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for MissingPanicsDocWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: function '{}' has P marker but is missing a `/// # Panics` doc section\n  at {}:{}",
            self.function, self.file, self.line,
        )
    }
}

/// 一条直接实现 `Into` 的警告：应实现 `From` 代替，`Into` 会自动提供。
#[derive(Debug, Clone)]
pub struct IntoImplWarning {
    pub impl_type: String,
    pub target_type: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for IntoImplWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: impl Into<{}> for {}—prefer impl From<{}> for {} instead\n  at {}:{}",
            self.target_type,
            self.impl_type,
            self.impl_type,
            self.target_type,
            self.file,
            self.line,
        )
    }
}

/// 一条消费参数未在错误中保留的警告：`fn(x: T) -> Result<(), E>` 在失败时丢失 `x`。
#[derive(Debug, Clone)]
pub struct ConsumedArgOnErrorWarning {
    pub function: String,
    pub param: String,
    pub param_type: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for ConsumedArgOnErrorWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: function '{}' consumes '{}' (type '{}') but the error type doesn't preserve it—consider returning it in the error\n  at {}:{}",
            self.function, self.param, self.param_type, self.file, self.line,
        )
    }
}

/// 一条 Deref 多态反模式警告：`impl Deref for X { Target = Y }` 不应用于方法复用。
#[derive(Debug, Clone)]
pub struct DerefPolymorphismWarning {
    pub impl_type: String,
    pub target_type: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for DerefPolymorphismWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: impl Deref for {} with Target = {} looks like Deref polymorphism—use composition instead of emulating inheritance\n  at {}:{}",
            self.impl_type, self.target_type, self.file, self.line,
        )
    }
}

/// 一条反射使用警告：使用了 `std::any::Any` / `type_name` / `type_id`，应改用 trait 分发。
#[derive(Debug, Clone)]
pub struct ReflectionUsageWarning {
    pub function: String,
    pub path: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for ReflectionUsageWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: function '{}' uses '{}'—prefer trait-based dispatch over reflection\n  at {}:{}",
            self.function, self.path, self.file, self.line,
        )
    }
}

/// 一条 TODO/FIXME 注释警告：代码中留有未完成标记。
#[derive(Debug, Clone)]
pub struct TodoCommentWarning {
    pub kind: String,
    pub text: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for TodoCommentWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: {} comment found: {}{}\n  at {}:{}",
            self.kind,
            if self.text.is_empty() { "" } else { &self.text },
            if self.text.is_empty() { "" } else { " " },
            self.file,
            self.line,
        )
    }
}

/// 一条好函数未被测试覆盖的警告：好函数（能力 ≤ ABM）应有对应测试。
#[derive(Debug, Clone)]
pub struct UntestedGoodFnWarning {
    pub function: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for UntestedGoodFnWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: good function '{}' is not called by any test\n  at {}:{}",
            self.function, self.file, self.line,
        )
    }
}

/// 一条缺断言警告：函数有参数却未对每个参数写 debug_assert。
#[derive(Debug, Clone)]
pub struct MissingAssertWarning {
    pub function: String,
    pub missing_params: Vec<String>,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for MissingAssertWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: {} has parameters without debug_assert: [{}]\n  at {}:{}",
            self.function,
            self.missing_params.join(", "),
            self.file,
            self.line,
        )
    }
}

/// 一条死代码警告：函数被 #[allow(dead_code)] 或 #[allow(unused)] 标记。
#[derive(Debug, Clone)]
pub struct DeadCodeWarning {
    pub function: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for DeadCodeWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: {} is marked #[allow(dead_code)] or #[allow(unused)] and excluded from report\n  at {}:{}",
            self.function, self.file, self.line,
        )
    }
}

/// 推断警告之别：函数的实际行为与其声明的能力后缀不符。
#[derive(Debug, Clone, PartialEq)]
pub enum InferenceKind {
    MissingAsync,
    MissingUnsafe,
    MissingMutable,
    MissingPanic,
    MissingSideEffect,
    MissingThreadLocal,
    NonAlphabeticalSuffix,
    DuplicateSuffixLetter,
}

impl fmt::Display for InferenceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InferenceKind::MissingAsync => write!(f, "declared async but missing A"),
            InferenceKind::MissingUnsafe => write!(f, "contains unsafe but missing U"),
            InferenceKind::MissingMutable => write!(f, "has &mut parameter but missing M"),
            InferenceKind::MissingPanic => write!(f, "calls panic macro but missing P"),
            InferenceKind::MissingSideEffect => {
                write!(f, "reads static/thread_local but missing S")
            }
            InferenceKind::MissingThreadLocal => write!(f, "reads thread_local but missing T"),
            InferenceKind::NonAlphabeticalSuffix => {
                write!(f, "capability suffix not in alphabetical order")
            }
            InferenceKind::DuplicateSuffixLetter => {
                write!(f, "duplicate capability letter in suffix")
            }
        }
    }
}

/// 一条推断警告：函数的实际行为暗示它应有某能力，但名字里没写。
#[derive(Debug, Clone)]
pub struct InferenceWarning {
    pub function: String,
    pub kind: InferenceKind,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for InferenceWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = if self.kind == InferenceKind::MissingPanic {
            "warning"
        } else {
            "hint"
        };
        write!(
            f,
            "{}: {} {} in its name\n  at {}:{}",
            severity, self.function, self.kind, self.file, self.line,
        )
    }
}

/// 一条缺 `#[allow(non_snake_case)]` 警告：函数名有大写后缀，却未豁免 snake_case 检查。
/// 外层 impl/trait/mod/crate 级的 `#![allow(non_snake_case)]` 可以覆盖本函数。
#[derive(Debug, Clone)]
pub struct MissingAllowWarning {
    pub function: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for MissingAllowWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: {} has uppercase capability suffix but is not covered by #[allow(non_snake_case)]\n  at {}:{}",
            self.function, self.file, self.line,
        )
    }
}

/// 一条 `#[test]` 命名格式警告：测试函数名不匹配 `^test_\d{{8}}_\w+$`。
#[derive(Debug, Clone)]
pub struct TestNameFormatWarning {
    pub function: String,
    pub file: String,
    pub line: usize,
}

impl fmt::Display for TestNameFormatWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "warning: test {} does not match required format ^test_\\d{{8}}_\\w+$\n  at {}:{}",
            self.function, self.file, self.line,
        )
    }
}

/// 一条 `#[test]` 命名重复警告：同名测试函数出现多次。
/// `occurrences` 列出每一处出现（文件 + 行）。
#[derive(Debug, Clone)]
pub struct DuplicateTestWarning {
    pub name: String,
    pub occurrences: Vec<(String, usize)>,
}

impl fmt::Display for DuplicateTestWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "warning: duplicate test name '{}' ({} occurrences):",
            self.name,
            self.occurrences.len()
        )?;
        for (i, (file, line)) in self.occurrences.iter().enumerate() {
            if i + 1 == self.occurrences.len() {
                write!(f, "  - {file}:{line}")?;
            } else {
                writeln!(f, "  - {file}:{line}")?;
            }
        }
        Ok(())
    }
}

/// 内部实现：检查函数调用合规性与静态引用合规性。
fn rvs_check_functions_impl(functions: &[FnDef], file: &str, capsmap: &CapsMap) -> CheckOutput {
    let mut violations = Vec::new();
    let mut warnings = Vec::new();
    let mut assert_warnings = Vec::new();
    let mut dead_code_warnings = Vec::new();
    let mut inference_warnings = Vec::new();
    let mut missing_allow_warnings = Vec::new();

    for func in functions {
        if !func.raw_suffix.is_empty() && !func.has_allow_non_snake_case {
            missing_allow_warnings.push(MissingAllowWarning {
                function: func.name.clone(),
                file: file.to_string(),
                line: func.line,
            });
        }
        if func.allows_dead_code {
            dead_code_warnings.push(DeadCodeWarning {
                function: func.name.clone(),
                file: file.to_string(),
                line: func.line,
            });
        }
        if func.has_body && !func.params.is_empty() {
            let missing: Vec<String> = func
                .params
                .iter()
                .filter(|p| p.ty == crate::extract::ParamType::PrimitiveNumeric)
                .filter(|p| !func.debug_asserted_params.contains(&p.name))
                .map(|p| p.name.clone())
                .collect();
            if !missing.is_empty() {
                assert_warnings.push(MissingAssertWarning {
                    function: func.name.clone(),
                    missing_params: missing,
                    file: file.to_string(),
                    line: func.line,
                });
            }
        }

        for call in &func.calls {
            let callee_caps = match rvs_parse_function(&call.name) {
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

        if func.is_async_fn && !func.capabilities.rvs_contains(Capability::A) {
            inference_warnings.push(InferenceWarning {
                function: func.name.clone(),
                kind: InferenceKind::MissingAsync,
                file: file.to_string(),
                line: func.line,
            });
        }
        if (func.has_unsafe_block || func.is_unsafe_fn)
            && !func.capabilities.rvs_contains(Capability::U)
        {
            inference_warnings.push(InferenceWarning {
                function: func.name.clone(),
                kind: InferenceKind::MissingUnsafe,
                file: file.to_string(),
                line: func.line,
            });
        }
        if (func.has_mut_param || func.has_mut_self)
            && !func.capabilities.rvs_contains(Capability::M)
        {
            inference_warnings.push(InferenceWarning {
                function: func.name.clone(),
                kind: InferenceKind::MissingMutable,
                file: file.to_string(),
                line: func.line,
            });
        }
        if func.has_panic_macro && !func.capabilities.rvs_contains(Capability::P) {
            inference_warnings.push(InferenceWarning {
                function: func.name.clone(),
                kind: InferenceKind::MissingPanic,
                file: file.to_string(),
                line: func.line,
            });
        }
        for sr in &func.static_refs {
            if sr.required_caps.rvs_contains(Capability::S)
                && !func.capabilities.rvs_contains(Capability::S)
            {
                inference_warnings.push(InferenceWarning {
                    function: func.name.clone(),
                    kind: InferenceKind::MissingSideEffect,
                    file: file.to_string(),
                    line: func.line,
                });
                break;
            }
        }
        for sr in &func.static_refs {
            if sr.required_caps.rvs_contains(Capability::T)
                && !func.capabilities.rvs_contains(Capability::T)
            {
                inference_warnings.push(InferenceWarning {
                    function: func.name.clone(),
                    kind: InferenceKind::MissingThreadLocal,
                    file: file.to_string(),
                    line: func.line,
                });
                break;
            }
        }

        if !func.raw_suffix.is_empty() {
            let chars: Vec<char> = func.raw_suffix.chars().collect();
            let sorted: Vec<char> = {
                let mut s = chars.clone();
                s.sort();
                s
            };
            if chars != sorted {
                inference_warnings.push(InferenceWarning {
                    function: func.name.clone(),
                    kind: InferenceKind::NonAlphabeticalSuffix,
                    file: file.to_string(),
                    line: func.line,
                });
            }
            let mut seen = std::collections::HashSet::new();
            for &c in &chars {
                if !seen.insert(c) {
                    inference_warnings.push(InferenceWarning {
                        function: func.name.clone(),
                        kind: InferenceKind::DuplicateSuffixLetter,
                        file: file.to_string(),
                        line: func.line,
                    });
                    break;
                }
            }
        }
    }

    CheckOutput {
        violations,
        warnings,
        assert_warnings,
        dead_code_warnings,
        inference_warnings,
        missing_allow_warnings,
        test_name_warnings: Vec::new(),
        duplicate_test_warnings: Vec::new(),
        banned_import_warnings: Vec::new(),
        non_rvs_fn_warnings: Vec::new(),
        missing_doc_warnings: Vec::new(),
        deny_warnings_warnings: Vec::new(),
        wildcard_import_warnings: Vec::new(),
        missing_safety_doc_warnings: Vec::new(),
        borrowed_param_warnings: Vec::new(),
        missing_debug_warnings: Vec::new(),
        missing_panics_doc_warnings: Vec::new(),
        into_impl_warnings: Vec::new(),
        consumed_arg_on_error_warnings: Vec::new(),
        deref_polymorphism_warnings: Vec::new(),
        reflection_usage_warnings: Vec::new(),
        todo_comment_warnings: Vec::new(),
        untested_good_fn_warnings: Vec::new(),
    }
}

/// 纯函数：检查一组函数定义中的调用合规性与静态引用合规性。
pub fn rvs_check_functions(functions: &[FnDef], file: &str) -> Vec<Violation> {
    rvs_check_functions_impl(functions, file, &CapsMap::rvs_new()).violations
}

/// 纯函数：判断一个测试函数名是否符合 `^test_\d{8}_\w+$`。
/// `test_` 前缀 + 八位数字（YYYYMMDD）+ 下划线 + 至少一个字母数字或下划线。
pub fn rvs_is_valid_test_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("test_") else {
        return false;
    };
    if rest.len() < 10 {
        return false;
    }
    let (date, suffix) = rest.split_at(8);
    if !date.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Some(tail) = suffix.strip_prefix('_') else {
        return false;
    };
    if tail.is_empty() {
        return false;
    }
    tail.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// 纯函数：从带文件信息的测试清单中找出重名的组，每组生成一条警告。
/// 输入不要求排序；输出按测试名字典序。
pub fn rvs_find_duplicate_tests(entries: &[(String, TestName)]) -> Vec<DuplicateTestWarning> {
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
    for (file, t) in entries {
        groups
            .entry(t.name.clone())
            .or_default()
            .push((file.clone(), t.line));
    }

    groups
        .into_iter()
        .filter(|(_, occ)| occ.len() >= 2)
        .map(|(name, occurrences)| DuplicateTestWarning { name, occurrences })
        .collect()
}

/// 纯函数：为一批测试名计算格式警告与同源重复警告。
/// 同源是指此次调用所覆盖的所有测试——单文件或整个路径。
/// 返回两个 Vec：格式警告与重复警告。不改任何入参。
fn rvs_test_warnings(
    tests: &[(String, TestName)],
) -> (Vec<TestNameFormatWarning>, Vec<DuplicateTestWarning>) {
    let mut fmt_warnings = Vec::new();
    for (file, t) in tests {
        if !rvs_is_valid_test_name(&t.name) {
            fmt_warnings.push(TestNameFormatWarning {
                function: t.name.clone(),
                file: file.clone(),
                line: t.line,
            });
        }
    }
    let dup_warnings = rvs_find_duplicate_tests(tests);
    (fmt_warnings, dup_warnings)
}

/// 从一段源码文本中检查违规，配合 CapsMap。
pub fn rvs_check_source(
    source: &str,
    file: &str,
    capsmap: &CapsMap,
) -> Result<CheckOutput, CheckError> {
    let functions = rvs_extract_functions(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    let tests = rvs_extract_tests(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    let imports = rvs_extract_imports(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    let non_rvs_fns = rvs_extract_non_rvs_fns(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    let pub_items = rvs_extract_pub_items(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;

    let mut output = rvs_check_functions_impl(&functions, file, capsmap);
    let entries: Vec<(String, TestName)> =
        tests.into_iter().map(|t| (file.to_string(), t)).collect();
    let (fmt_warnings, dup_warnings) = rvs_test_warnings(&entries);
    output.test_name_warnings.extend(fmt_warnings);
    output.duplicate_test_warnings.extend(dup_warnings);

    // 检查被禁导入
    output
        .banned_import_warnings
        .extend(rvs_check_imports(&imports, file));

    // 检查函数命名（缺少 rvs_ 前缀）
    output
        .non_rvs_fn_warnings
        .extend(rvs_check_non_rvs_fn_names(&non_rvs_fns, file));

    // 检查 pub 函数/方法的文档注释
    output
        .missing_doc_warnings
        .extend(rvs_check_missing_doc(&pub_items, file));

    // 检查 wildcard import
    output
        .wildcard_import_warnings
        .extend(rvs_check_wildcard_imports(&imports, file));

    // 检查借用类型参数
    let borrowed_params = rvs_extract_borrowed_params(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    output
        .borrowed_param_warnings
        .extend(rvs_check_borrowed_params(&borrowed_params, file));

    // 检查 unsafe 函数缺 safety 文档
    let unsafe_fns = rvs_extract_unsafe_fns(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    output
        .missing_safety_doc_warnings
        .extend(rvs_check_unsafe_safety_doc(&unsafe_fns, file));

    // 检查 #![deny(warnings)] 反模式
    let deny_line = rvs_extract_deny_warnings(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    output
        .deny_warnings_warnings
        .extend(rvs_check_deny_warnings(deny_line, file));

    // 检查公开类型缺 Debug derive
    let missing_debug = rvs_extract_missing_debug(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    output
        .missing_debug_warnings
        .extend(rvs_check_missing_debug(&missing_debug, file));

    // 检查 P 标记函数缺 /// # Panics 文档
    let missing_panics =
        rvs_extract_missing_panics_doc(source).map_err(|e| CheckError::Extract {
            source: e,
            file: file.to_string(),
        })?;
    output
        .missing_panics_doc_warnings
        .extend(rvs_check_missing_panics_doc(&missing_panics, file));

    // 检查直接实现 Into（应实现 From 代替）
    let into_impls = rvs_extract_into_impls(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    output
        .into_impl_warnings
        .extend(rvs_check_into_impls(&into_impls, file));

    // 检查消费参数未在错误中保留
    let consumed_args =
        rvs_extract_consumed_arg_on_error(source).map_err(|e| CheckError::Extract {
            source: e,
            file: file.to_string(),
        })?;
    output
        .consumed_arg_on_error_warnings
        .extend(rvs_check_consumed_arg_on_error(&consumed_args, file));

    // 检查 Deref 多态反模式
    let deref_poly = rvs_extract_deref_polymorphism(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    output
        .deref_polymorphism_warnings
        .extend(rvs_check_deref_polymorphism(&deref_poly, file));

    // 检查反射使用（std::any::Any / type_name / type_id）
    let reflection_usage = rvs_extract_reflection_usage(&functions);
    output
        .reflection_usage_warnings
        .extend(rvs_check_reflection_usage(&reflection_usage, file));

    // 检查 stub 宏（todo! / unimplemented!）→ 违规
    let stub_macros = rvs_extract_stub_macros(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    output
        .violations
        .extend(rvs_check_stub_macros(&stub_macros, file));

    // 检查空函数体 → 违规
    let empty_fns = rvs_extract_empty_fns(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    output
        .violations
        .extend(rvs_check_empty_fns(&empty_fns, file));

    // 检查 TODO/FIXME 注释
    let todo_comments = rvs_extract_todo_comments(source);
    output
        .todo_comment_warnings
        .extend(rvs_check_todo_comments(&todo_comments, file));

    // 检查好函数未被测试覆盖
    let test_call_names = rvs_extract_test_call_names(source).map_err(|e| CheckError::Extract {
        source: e,
        file: file.to_string(),
    })?;
    output
        .untested_good_fn_warnings
        .extend(rvs_check_untested_good_fns(
            &functions,
            &test_call_names,
            file,
        ));

    Ok(output)
}

/// 从文件路径（或目录）出发，检查违规。
/// CapsMap 用于查找非 rvs_ 函数的能力。
/// 测试命名唯一性检查在整个路径内进行——跨文件同名亦会被抓。
#[allow(non_snake_case)]
pub fn rvs_check_path_BI(path: &Path, capsmap: &CapsMap) -> Result<CheckOutput, CheckError> {
    let sources = rvs_read_rust_sources_BI(path).map_err(|e| CheckError::Read { source: e })?;
    let mut output = CheckOutput::default();
    let mut all_tests: Vec<(String, TestName)> = Vec::new();
    let mut all_test_call_names: Vec<String> = Vec::new();
    let mut all_file_functions: Vec<(String, Vec<FnDef>)> = Vec::new();

    for sf in &sources {
        let functions = rvs_extract_functions(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;
        let tests = rvs_extract_tests(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;
        let imports = rvs_extract_imports(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;
        let non_rvs_fns = rvs_extract_non_rvs_fns(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;
        let pub_items = rvs_extract_pub_items(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;

        let result = rvs_check_functions_impl(&functions, &sf.path, capsmap);
        output.violations.extend(result.violations);
        output.warnings.extend(result.warnings);
        output.assert_warnings.extend(result.assert_warnings);
        output.dead_code_warnings.extend(result.dead_code_warnings);
        output.inference_warnings.extend(result.inference_warnings);
        output
            .missing_allow_warnings
            .extend(result.missing_allow_warnings);

        // 检查被禁导入
        output
            .banned_import_warnings
            .extend(rvs_check_imports(&imports, &sf.path));

        // 检查私有函数命名
        output
            .non_rvs_fn_warnings
            .extend(rvs_check_non_rvs_fn_names(&non_rvs_fns, &sf.path));

        // 检查 pub 函数/方法的文档注释
        output
            .missing_doc_warnings
            .extend(rvs_check_missing_doc(&pub_items, &sf.path));

        // 检查 wildcard import
        output
            .wildcard_import_warnings
            .extend(rvs_check_wildcard_imports(&imports, &sf.path));

        // 检查借用类型参数
        let borrowed_params =
            rvs_extract_borrowed_params(&sf.source).map_err(|e| CheckError::Extract {
                source: e,
                file: sf.path.clone(),
            })?;
        output
            .borrowed_param_warnings
            .extend(rvs_check_borrowed_params(&borrowed_params, &sf.path));

        // 检查 unsafe 函数缺 safety 文档
        let unsafe_fns = rvs_extract_unsafe_fns(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;
        output
            .missing_safety_doc_warnings
            .extend(rvs_check_unsafe_safety_doc(&unsafe_fns, &sf.path));

        // 检查 #![deny(warnings)] 反模式
        let deny_line = rvs_extract_deny_warnings(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;
        output
            .deny_warnings_warnings
            .extend(rvs_check_deny_warnings(deny_line, &sf.path));

        // 检查公开类型缺 Debug derive
        let missing_debug =
            rvs_extract_missing_debug(&sf.source).map_err(|e| CheckError::Extract {
                source: e,
                file: sf.path.clone(),
            })?;
        output
            .missing_debug_warnings
            .extend(rvs_check_missing_debug(&missing_debug, &sf.path));

        // 检查 P 标记函数缺 /// # Panics 文档
        let missing_panics =
            rvs_extract_missing_panics_doc(&sf.source).map_err(|e| CheckError::Extract {
                source: e,
                file: sf.path.clone(),
            })?;
        output
            .missing_panics_doc_warnings
            .extend(rvs_check_missing_panics_doc(&missing_panics, &sf.path));

        // 检查直接实现 Into
        let into_impls = rvs_extract_into_impls(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;
        output
            .into_impl_warnings
            .extend(rvs_check_into_impls(&into_impls, &sf.path));

        // 检查消费参数未在错误中保留
        let consumed_args =
            rvs_extract_consumed_arg_on_error(&sf.source).map_err(|e| CheckError::Extract {
                source: e,
                file: sf.path.clone(),
            })?;
        output
            .consumed_arg_on_error_warnings
            .extend(rvs_check_consumed_arg_on_error(&consumed_args, &sf.path));

        // 检查 Deref 多态反模式
        let deref_poly =
            rvs_extract_deref_polymorphism(&sf.source).map_err(|e| CheckError::Extract {
                source: e,
                file: sf.path.clone(),
            })?;
        output
            .deref_polymorphism_warnings
            .extend(rvs_check_deref_polymorphism(&deref_poly, &sf.path));

        // 检查反射使用
        let reflection_usage = rvs_extract_reflection_usage(&functions);
        output
            .reflection_usage_warnings
            .extend(rvs_check_reflection_usage(&reflection_usage, &sf.path));

        // 检查 stub 宏（todo! / unimplemented!）→ 违规
        let stub_macros = rvs_extract_stub_macros(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;
        output
            .violations
            .extend(rvs_check_stub_macros(&stub_macros, &sf.path));

        // 检查空函数体 → 违规
        let empty_fns = rvs_extract_empty_fns(&sf.source).map_err(|e| CheckError::Extract {
            source: e,
            file: sf.path.clone(),
        })?;
        output
            .violations
            .extend(rvs_check_empty_fns(&empty_fns, &sf.path));

        // 检查 TODO/FIXME 注释
        let todo_comments = rvs_extract_todo_comments(&sf.source);
        output
            .todo_comment_warnings
            .extend(rvs_check_todo_comments(&todo_comments, &sf.path));

        for t in tests {
            all_tests.push((sf.path.clone(), t));
        }

        // 收集测试调用名和函数定义，供后续跨文件交叉检查
        let test_call_names =
            rvs_extract_test_call_names(&sf.source).map_err(|e| CheckError::Extract {
                source: e,
                file: sf.path.clone(),
            })?;
        all_test_call_names.extend(test_call_names);

        all_file_functions.push((sf.path.clone(), functions));
    }

    // 跨文件检查：好函数未被测试覆盖
    all_test_call_names.sort();
    all_test_call_names.dedup();
    for (file, functions) in &all_file_functions {
        output
            .untested_good_fn_warnings
            .extend(rvs_check_untested_good_fns(
                functions,
                &all_test_call_names,
                file,
            ));
    }

    let (fmt_warnings, dup_warnings) = rvs_test_warnings(&all_tests);
    output.test_name_warnings.extend(fmt_warnings);
    output.duplicate_test_warnings.extend(dup_warnings);
    Ok(output)
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("failed to read: {source}")]
    Read { source: crate::source::ReadError },
    #[error("failed to extract from '{file}': {source}")]
    Extract {
        file: String,
        source: crate::extract::ExtractError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum MirCheckError {
    #[error("failed to compile MIR: {source}")]
    Compile { source: crate::mir::MirCompileError },
    #[error("failed to read MIR files: {source}")]
    Read { source: crate::source::ReadError },
    #[error("failed to extract MIR from '{file}': {source}")]
    MirExtract {
        file: String,
        source: crate::mir::MirError,
    },
}

/// 从目录中所有 `.mir` 文件中萃取函数定义，做跨文件合并后检查调用合规性。
#[allow(non_snake_case)]
pub fn rvs_check_mir_dir_BIM(
    mir_dir: &Path,
    capsmap: &CapsMap,
) -> Result<CheckOutput, MirCheckError> {
    let sources = crate::source::rvs_read_mir_sources_BI(mir_dir)
        .map_err(|e| MirCheckError::Read { source: e })?;

    let mut all_functions: Vec<FnDef> = Vec::new();
    for sf in &sources {
        if let Ok(functions) = crate::mir::rvs_extract_from_mir(&sf.source) {
            all_functions.extend(functions);
        }
    }

    let mut fn_map: HashMap<String, FnDef> = HashMap::new();
    for func in all_functions {
        let name = func.name.clone();
        match fn_map.entry(name) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().calls.extend(func.calls);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(func);
            }
        }
    }
    let functions: Vec<FnDef> = fn_map.into_values().collect();

    Ok(rvs_check_functions_impl(
        &functions,
        &mir_dir.display().to_string(),
        capsmap,
    ))
}

/// 先用 cargo 编译项目至 MIR，再对生成的 `.mir` 文件做能力检查。
///
/// # Panics
///
/// Panics if MIR compilation or file operations fail unexpectedly.
#[allow(non_snake_case)]
pub fn rvs_check_mir_path_BIMPS(
    project_dir: &Path,
    capsmap: &CapsMap,
) -> Result<CheckOutput, MirCheckError> {
    let deps_dir = crate::mir::rvs_compile_to_mir_BIMPS(project_dir)
        .map_err(|e| MirCheckError::Compile { source: e })?;
    rvs_check_mir_dir_BIM(&deps_dir, capsmap)
}
