use std::collections::BTreeSet;

use crate::capability::{Capability, CapabilitySet, rvs_extract_raw_suffix, rvs_parse_function};
use syn::spanned::Spanned;

/// 被调用者的蛛丝马迹：名与行。
#[derive(Debug, Clone)]
pub struct CalleeInfo {
    pub name: String,
    pub line: usize,
}

/// 静态变量的引用：名、所需能力、所在行。
///
/// 全局之物，岂可暗用？
/// 引之者必先声明其力。
#[derive(Debug, Clone)]
pub struct StaticRef {
    pub name: String,
    pub required_caps: CapabilitySet,
    pub line: usize,
}

/// 函数之全貌：名、能力、所调、静态引用、所在行、所占行数、参数、已断言之参、推断信号。
#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub capabilities: CapabilitySet,
    pub calls: Vec<CalleeInfo>,
    pub static_refs: Vec<StaticRef>,
    pub line: usize,
    pub line_count: usize,
    pub params: Vec<ParamInfo>,
    pub debug_asserted_params: BTreeSet<String>,
    pub has_body: bool,
    pub has_unsafe_block: bool,
    pub is_async_fn: bool,
    pub is_unsafe_fn: bool,
    pub has_mut_param: bool,
    pub has_mut_self: bool,
    pub has_panic_macro: bool,
    pub raw_suffix: String,
    pub is_test: bool,
    pub allows_dead_code: bool,
    pub has_allow_non_snake_case: bool,
}

/// 一项 `#[test]` 的简记：函数名与所在行。
/// 用于命名规范与唯一性检查。
#[derive(Debug, Clone)]
pub struct TestName {
    pub name: String,
    pub line: usize,
}

/// 一条 use 导入语句的信息：路径与所在行。
#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub use_path: String,
    pub line: usize,
}

/// 参数之名与类型。
#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub ty: ParamType,
}

/// 参数类型的大致分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    PrimitiveNumeric,
    Other,
}

/// 文件中声明的静态变量：名字与所需能力。
#[derive(Clone)]
struct StaticDecl {
    name: String,
    required_caps: CapabilitySet,
}

/// 巡遍所得：函数调用与静态引用，一并收罗。
struct Harvest {
    calls: Vec<CalleeInfo>,
    static_refs: Vec<StaticRef>,
}

impl Harvest {
    fn rvs_empty() -> Self {
        Harvest {
            calls: Vec::new(),
            static_refs: Vec::new(),
        }
    }

    fn rvs_merge(parts: impl IntoIterator<Item = Harvest>) -> Self {
        let mut result = Harvest::rvs_empty();
        for part in parts {
            result.calls.extend(part.calls);
            result.static_refs.extend(part.static_refs);
        }
        result
    }
}

/// 检查路径是否引用了已知的静态变量。
/// 若路径末段与某声明同名，则认领之。
fn rvs_check_path_for_static(path: &syn::Path, statics: &[StaticDecl]) -> Option<StaticRef> {
    let segment = path.segments.last()?;
    let name = segment.ident.to_string();
    statics
        .iter()
        .find(|decl| decl.name == name)
        .map(|decl| StaticRef {
            name: decl.name.clone(),
            required_caps: decl.required_caps.clone(),
            line: segment.ident.span().start().line,
        })
}

/// 从宏的 token 流中提取函数调用。
/// 宏体无法被 syn 解析为 AST，只能做 token 级扫描：
/// 收集所有形如 `path::to::func(` 或 `method(` 的调用路径。
fn rvs_harvest_calls_from_tokens(tokens: proc_macro2::TokenStream) -> Harvest {
    let mut result = Harvest::rvs_empty();
    let mut tokens = tokens.into_iter().peekable();
    let mut path_parts: Vec<String> = Vec::new();
    while let Some(tt) = tokens.next() {
        match tt {
            proc_macro2::TokenTree::Ident(ident) => {
                path_parts.push(ident.to_string());
            }
            proc_macro2::TokenTree::Punct(punct) => {
                if punct.as_char() == ':' {
                    if tokens
                        .peek()
                        .map(
                            |t| matches!(t, proc_macro2::TokenTree::Punct(p) if p.as_char() == ':'),
                        )
                        .unwrap_or(false)
                    {
                        tokens.next();
                    } else {
                        path_parts.clear();
                    }
                } else if punct.as_char() == '(' {
                    if !path_parts.is_empty() {
                        let name = path_parts.join("::");
                        let line = punct.span().start().line;
                        result.calls.push(CalleeInfo { name, line });
                    }
                    path_parts.clear();
                } else if punct.as_char() == '.' {
                    if let Some(last) = path_parts.last()
                        && last != &"::".to_string()
                    {
                        path_parts.clear();
                    }
                } else {
                    path_parts.clear();
                }
            }
            proc_macro2::TokenTree::Group(group) => {
                if !path_parts.is_empty() {
                    let name = path_parts.join("::");
                    let line = group.span_open().start().line;
                    result.calls.push(CalleeInfo { name, line });
                }
                path_parts.clear();
                let sub = rvs_harvest_calls_from_tokens(group.stream());
                result.calls.extend(sub.calls);
                result.static_refs.extend(sub.static_refs);
            }
            proc_macro2::TokenTree::Literal(_) => {
                path_parts.clear();
            }
        }
    }
    result
}

/// 从直接调用中捉拿函数调用与静态引用。
fn rvs_harvest_from_expr_call(call: &syn::ExprCall, statics: &[StaticDecl]) -> Harvest {
    let mut result = Harvest::rvs_empty();
    if let syn::Expr::Path(expr_path) = &*call.func {
        let name: String = expr_path
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        if !name.is_empty()
            && let Some(seg) = expr_path.path.segments.last()
        {
            let line = seg.ident.span().start().line;
            result.calls.push(CalleeInfo { name, line });
        }
    }
    let sub = rvs_harvest_from_expr(&call.func, statics);
    result.calls.extend(sub.calls);
    result.static_refs.extend(sub.static_refs);
    for arg in &call.args {
        let sub = rvs_harvest_from_expr(arg, statics);
        result.calls.extend(sub.calls);
        result.static_refs.extend(sub.static_refs);
    }
    result
}

/// 从方法调用中捉拿函数调用与静态引用。
fn rvs_harvest_from_expr_method_call(
    call: &syn::ExprMethodCall,
    statics: &[StaticDecl],
) -> Harvest {
    let mut result = Harvest::rvs_empty();
    let name = call.method.to_string();
    let line = call.method.span().start().line;
    result.calls.push(CalleeInfo { name, line });
    let sub = rvs_harvest_from_expr(&call.receiver, statics);
    result.calls.extend(sub.calls);
    result.static_refs.extend(sub.static_refs);
    for arg in &call.args {
        let sub = rvs_harvest_from_expr(arg, statics);
        result.calls.extend(sub.calls);
        result.static_refs.extend(sub.static_refs);
    }
    result
}

/// 巡遍表达式，不论深浅，逢调用必捉，逢静态必记。
fn rvs_harvest_from_expr(expr: &syn::Expr, statics: &[StaticDecl]) -> Harvest {
    match expr {
        syn::Expr::Call(call) => rvs_harvest_from_expr_call(call, statics),
        syn::Expr::MethodCall(call) => rvs_harvest_from_expr_method_call(call, statics),
        syn::Expr::Path(path) => {
            let mut result = Harvest::rvs_empty();
            if let Some(sr) = rvs_check_path_for_static(&path.path, statics) {
                result.static_refs.push(sr);
            }
            result
        }
        syn::Expr::Block(block) => rvs_harvest_from_block(&block.block, statics),
        syn::Expr::If(if_expr) => Harvest::rvs_merge([
            rvs_harvest_from_expr(&if_expr.cond, statics),
            rvs_harvest_from_block(&if_expr.then_branch, statics),
            if_expr
                .else_branch
                .as_ref()
                .map(|(_, e)| rvs_harvest_from_expr(e, statics))
                .unwrap_or_else(Harvest::rvs_empty),
        ]),
        syn::Expr::Match(match_expr) => {
            let mut result = rvs_harvest_from_expr(&match_expr.expr, statics);
            for arm in &match_expr.arms {
                if let Some((_, guard)) = &arm.guard {
                    let sub = rvs_harvest_from_expr(guard, statics);
                    result.calls.extend(sub.calls);
                    result.static_refs.extend(sub.static_refs);
                }
                let sub = rvs_harvest_from_expr(&arm.body, statics);
                result.calls.extend(sub.calls);
                result.static_refs.extend(sub.static_refs);
            }
            result
        }
        syn::Expr::Loop(loop_expr) => rvs_harvest_from_block(&loop_expr.body, statics),
        syn::Expr::While(while_expr) => Harvest::rvs_merge([
            rvs_harvest_from_expr(&while_expr.cond, statics),
            rvs_harvest_from_block(&while_expr.body, statics),
        ]),
        syn::Expr::ForLoop(for_expr) => Harvest::rvs_merge([
            rvs_harvest_from_expr(&for_expr.expr, statics),
            rvs_harvest_from_block(&for_expr.body, statics),
        ]),
        syn::Expr::Closure(closure) => rvs_harvest_from_expr(&closure.body, statics),
        syn::Expr::Assign(assign) => Harvest::rvs_merge([
            rvs_harvest_from_expr(&assign.left, statics),
            rvs_harvest_from_expr(&assign.right, statics),
        ]),
        syn::Expr::Binary(binary) => Harvest::rvs_merge([
            rvs_harvest_from_expr(&binary.left, statics),
            rvs_harvest_from_expr(&binary.right, statics),
        ]),
        syn::Expr::Unary(unary) => rvs_harvest_from_expr(&unary.expr, statics),
        syn::Expr::Paren(paren) => rvs_harvest_from_expr(&paren.expr, statics),
        syn::Expr::Tuple(tuple) => Harvest::rvs_merge(
            tuple
                .elems
                .iter()
                .map(|e| rvs_harvest_from_expr(e, statics)),
        ),
        syn::Expr::Array(array) => Harvest::rvs_merge(
            array
                .elems
                .iter()
                .map(|e| rvs_harvest_from_expr(e, statics)),
        ),
        syn::Expr::Struct(struct_expr) => {
            let mut parts: Vec<Harvest> = struct_expr
                .fields
                .iter()
                .map(|f| rvs_harvest_from_expr(&f.expr, statics))
                .collect();
            if let Some(rest) = &struct_expr.rest {
                parts.push(rvs_harvest_from_expr(rest, statics));
            }
            Harvest::rvs_merge(parts)
        }
        syn::Expr::Repeat(repeat) => Harvest::rvs_merge([
            rvs_harvest_from_expr(&repeat.expr, statics),
            rvs_harvest_from_expr(&repeat.len, statics),
        ]),
        syn::Expr::Range(range) => {
            let mut parts = Vec::new();
            if let Some(start) = &range.start {
                parts.push(rvs_harvest_from_expr(start, statics));
            }
            if let Some(end) = &range.end {
                parts.push(rvs_harvest_from_expr(end, statics));
            }
            Harvest::rvs_merge(parts)
        }
        syn::Expr::Index(index) => Harvest::rvs_merge([
            rvs_harvest_from_expr(&index.expr, statics),
            rvs_harvest_from_expr(&index.index, statics),
        ]),
        syn::Expr::Field(field) => rvs_harvest_from_expr(&field.base, statics),
        syn::Expr::Reference(reference) => rvs_harvest_from_expr(&reference.expr, statics),
        syn::Expr::Try(try_expr) => rvs_harvest_from_expr(&try_expr.expr, statics),
        syn::Expr::Await(await_expr) => rvs_harvest_from_expr(&await_expr.base, statics),
        syn::Expr::Return(ret) => ret
            .expr
            .as_ref()
            .map(|e| rvs_harvest_from_expr(e, statics))
            .unwrap_or_else(Harvest::rvs_empty),
        syn::Expr::Break(brk) => brk
            .expr
            .as_ref()
            .map(|e| rvs_harvest_from_expr(e, statics))
            .unwrap_or_else(Harvest::rvs_empty),
        syn::Expr::Group(group) => rvs_harvest_from_expr(&group.expr, statics),
        syn::Expr::Let(let_expr) => rvs_harvest_from_expr(&let_expr.expr, statics),
        syn::Expr::Unsafe(unsafe_expr) => rvs_harvest_from_block(&unsafe_expr.block, statics),
        syn::Expr::Async(async_expr) => rvs_harvest_from_block(&async_expr.block, statics),
        syn::Expr::Cast(cast_expr) => rvs_harvest_from_expr(&cast_expr.expr, statics),
        syn::Expr::TryBlock(try_block) => rvs_harvest_from_block(&try_block.block, statics),
        syn::Expr::Macro(mac) => {
            let mut result = rvs_harvest_calls_from_tokens(mac.mac.tokens.clone());
            if let Some(sr) = rvs_check_path_for_static(&mac.mac.path, statics) {
                result.static_refs.push(sr);
            }
            result
        }
        syn::Expr::Lit(_) | syn::Expr::Continue(_) | syn::Expr::Verbatim(_) => Harvest::rvs_empty(),
        _ => Harvest::rvs_empty(),
    }
}

/// 巡遍一个块中的每一条语句。
fn rvs_harvest_from_block(block: &syn::Block, statics: &[StaticDecl]) -> Harvest {
    let mut result = Harvest::rvs_empty();
    for stmt in &block.stmts {
        let sub = match stmt {
            syn::Stmt::Local(local) => {
                let mut parts = Vec::new();
                if let Some(init) = &local.init {
                    parts.push(rvs_harvest_from_expr(&init.expr, statics));
                    if let Some((_, diverge)) = &init.diverge {
                        parts.push(rvs_harvest_from_expr(diverge, statics));
                    }
                }
                Harvest::rvs_merge(parts)
            }
            syn::Stmt::Expr(expr, _) => rvs_harvest_from_expr(expr, statics),
            syn::Stmt::Item(_) => Harvest::rvs_empty(),
            syn::Stmt::Macro(stmt_mac) => {
                rvs_harvest_calls_from_tokens(stmt_mac.mac.tokens.clone())
            }
        };
        result.calls.extend(sub.calls);
        result.static_refs.extend(sub.static_refs);
    }
    result
}

/// 巡遍一个块，收集其中所有调用与静态引用。
fn rvs_collect_calls_and_statics(
    block: &syn::Block,
    statics: &[StaticDecl],
) -> (Vec<CalleeInfo>, Vec<StaticRef>) {
    let harvest = rvs_harvest_from_block(block, statics);
    (harvest.calls, harvest.static_refs)
}

/// 取首尾行号之差加一，即为函数所占行数。
fn rvs_calc_line_count(start_span: proc_macro2::Span, end_span: proc_macro2::Span) -> usize {
    let start_line = start_span.start().line;
    let end_line = end_span.end().line;
    debug_assert!(end_line >= start_line, "函数尾行不应在首行之前");
    end_line - start_line + 1
}

/// 从 thread_local! 宏的 token 流中萃取变量名。
/// thread_local! { static FOO: T = ...; } → FOO
fn rvs_parse_thread_local_names(tokens: &proc_macro2::TokenStream) -> Vec<String> {
    let mut names = Vec::new();
    let mut tokens = tokens.clone().into_iter().fuse().peekable();
    while let Some(tt) = tokens.next() {
        match tt {
            proc_macro2::TokenTree::Group(group) => {
                names.extend(rvs_parse_thread_local_names(&group.stream()));
            }
            proc_macro2::TokenTree::Ident(ident) if ident == "static" => {
                while let Some(next) = tokens.peek() {
                    match next {
                        proc_macro2::TokenTree::Ident(name) => {
                            let name_str = name.to_string();
                            if name_str != "mut" {
                                names.push(name_str);
                            }
                            tokens.next();
                            break;
                        }
                        proc_macro2::TokenTree::Punct(p) if p.as_char() == ':' => {
                            tokens.next();
                        }
                        _ => break,
                    }
                }
            }
            _ => {}
        }
    }
    names
}

/// 从顶层项中搜集所有 static 声明与 thread_local! 宏声明。
fn rvs_collect_static_decls_from_items(items: &[syn::Item]) -> Vec<StaticDecl> {
    let mut decls = Vec::new();
    for item in items {
        match item {
            syn::Item::Static(s) => {
                let suffix = if matches!(s.mutability, syn::StaticMutability::Mut(_)) {
                    "SU"
                } else {
                    "S"
                };
                decls.push(StaticDecl {
                    name: s.ident.to_string(),
                    required_caps: CapabilitySet::rvs_from_validated(suffix),
                });
            }
            syn::Item::Macro(m) => {
                let macro_name = m
                    .mac
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_default();
                if macro_name == "thread_local" {
                    let names = rvs_parse_thread_local_names(&m.mac.tokens);
                    for name in names {
                        decls.push(StaticDecl {
                            name,
                            required_caps: CapabilitySet::rvs_from_validated("ST"),
                        });
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    decls.extend(rvs_collect_static_decls_from_items(items));
                }
            }
            _ => {}
        }
    }
    decls
}

const PRIMITIVE_NUMERIC_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "f32", "f64", "isize",
    "usize",
];

fn rvs_classify_param_type(ty: &syn::Type) -> ParamType {
    if let syn::Type::Path(type_path) = ty {
        let ident = type_path.path.segments.last().map(|s| s.ident.to_string());
        if let Some(name) = ident
            && PRIMITIVE_NUMERIC_TYPES.contains(&name.as_str())
        {
            return ParamType::PrimitiveNumeric;
        }
    }
    ParamType::Other
}

/// 判断参数列表中是否有 &mut self 接收者。
fn rvs_has_mut_receiver(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
) -> bool {
    inputs.iter().any(|arg| match arg {
        syn::FnArg::Receiver(r) => r.mutability.is_some(),
        _ => false,
    })
}

/// 判断参数列表中是否有 &mut T 参数（不含 self/&self/&mut self）。
fn rvs_has_mut_typed_param(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
) -> bool {
    inputs.iter().any(|arg| match arg {
        syn::FnArg::Typed(pt) => {
            matches!(&*pt.ty, syn::Type::Reference(r) if r.mutability.is_some())
        }
        _ => false,
    })
}

/// 从函数签名的参数列表中萃取参数名与类型。
/// self、&self、&mut self 不算参数，跳过之。
fn rvs_extract_param_names(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
) -> Vec<ParamInfo> {
    inputs
        .iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => {
                if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                    let ty = rvs_classify_param_type(&pat_type.ty);
                    Some(ParamInfo {
                        name: pat_ident.ident.to_string(),
                        ty,
                    })
                } else {
                    None
                }
            }
            syn::FnArg::Receiver(_) => None,
        })
        .collect()
}

/// 判断一个宏是否为 debug_assert! / debug_assert_eq! / debug_assert_ne!。
fn rvs_is_debug_assert(mac: &syn::Macro) -> bool {
    mac.path
        .segments
        .last()
        .map(|s| s.ident.to_string().starts_with("debug_assert"))
        .unwrap_or(false)
}

/// 从宏的 token 流中萃取所有标识符。
fn rvs_collect_ident_tokens(tokens: &proc_macro2::TokenStream) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for tt in tokens.clone() {
        match tt {
            proc_macro2::TokenTree::Ident(ident) => {
                ids.insert(ident.to_string());
            }
            proc_macro2::TokenTree::Group(group) => {
                ids.extend(rvs_collect_ident_tokens(&group.stream()));
            }
            _ => {}
        }
    }
    ids
}

/// 从一个块中搜集所有 debug_assert! 宏里出现的参数名。
fn rvs_collect_debug_asserted_params(block: &syn::Block) -> BTreeSet<String> {
    rvs_collect_assert_ids_from_block(block)
        .into_iter()
        .collect()
}

/// 纯函数：从块中收集所有 debug_assert! 参数名。
fn rvs_collect_assert_ids_from_block(block: &syn::Block) -> Vec<String> {
    let mut ids = Vec::new();
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Macro(m) => {
                if rvs_is_debug_assert(&m.mac) {
                    ids.extend(rvs_collect_ident_tokens(&m.mac.tokens));
                }
            }
            syn::Stmt::Expr(expr, _) => {
                ids.extend(rvs_collect_assert_ids_from_expr(expr));
            }
            syn::Stmt::Local(l) => {
                if let Some(init) = &l.init {
                    ids.extend(rvs_collect_assert_ids_from_expr(&init.expr));
                }
            }
            syn::Stmt::Item(_) => {}
        }
    }
    ids
}

/// 纯函数：从表达式中收集所有 debug_assert! 参数名。
fn rvs_collect_assert_ids_from_expr(expr: &syn::Expr) -> Vec<String> {
    let mut ids = Vec::new();
    match expr {
        syn::Expr::Macro(m) if rvs_is_debug_assert(&m.mac) => {
            ids.extend(rvs_collect_ident_tokens(&m.mac.tokens));
        }
        syn::Expr::Block(b) => ids.extend(rvs_collect_assert_ids_from_block(&b.block)),
        syn::Expr::If(e) => {
            ids.extend(rvs_collect_assert_ids_from_expr(&e.cond));
            ids.extend(rvs_collect_assert_ids_from_block(&e.then_branch));
            if let Some((_, els)) = &e.else_branch {
                ids.extend(rvs_collect_assert_ids_from_expr(els));
            }
        }
        syn::Expr::Match(e) => {
            for arm in &e.arms {
                ids.extend(rvs_collect_assert_ids_from_expr(&arm.body));
            }
        }
        syn::Expr::Loop(e) => ids.extend(rvs_collect_assert_ids_from_block(&e.body)),
        syn::Expr::While(e) => {
            ids.extend(rvs_collect_assert_ids_from_expr(&e.cond));
            ids.extend(rvs_collect_assert_ids_from_block(&e.body));
        }
        syn::Expr::ForLoop(e) => {
            ids.extend(rvs_collect_assert_ids_from_expr(&e.expr));
            ids.extend(rvs_collect_assert_ids_from_block(&e.body));
        }
        syn::Expr::Unsafe(e) => ids.extend(rvs_collect_assert_ids_from_block(&e.block)),
        syn::Expr::Closure(c) => ids.extend(rvs_collect_assert_ids_from_expr(&c.body)),
        syn::Expr::Call(e) => {
            ids.extend(rvs_collect_assert_ids_from_expr(&e.func));
            for a in &e.args {
                ids.extend(rvs_collect_assert_ids_from_expr(a));
            }
        }
        syn::Expr::MethodCall(e) => {
            ids.extend(rvs_collect_assert_ids_from_expr(&e.receiver));
            for a in &e.args {
                ids.extend(rvs_collect_assert_ids_from_expr(a));
            }
        }
        syn::Expr::Assign(e) => {
            ids.extend(rvs_collect_assert_ids_from_expr(&e.left));
            ids.extend(rvs_collect_assert_ids_from_expr(&e.right));
        }
        syn::Expr::Binary(e) => {
            ids.extend(rvs_collect_assert_ids_from_expr(&e.left));
            ids.extend(rvs_collect_assert_ids_from_expr(&e.right));
        }
        syn::Expr::Unary(e) => ids.extend(rvs_collect_assert_ids_from_expr(&e.expr)),
        syn::Expr::Paren(e) => ids.extend(rvs_collect_assert_ids_from_expr(&e.expr)),
        syn::Expr::Group(e) => ids.extend(rvs_collect_assert_ids_from_expr(&e.expr)),
        syn::Expr::Reference(e) => ids.extend(rvs_collect_assert_ids_from_expr(&e.expr)),
        syn::Expr::Try(e) => ids.extend(rvs_collect_assert_ids_from_expr(&e.expr)),
        syn::Expr::Await(e) => ids.extend(rvs_collect_assert_ids_from_expr(&e.base)),
        syn::Expr::Return(e) => {
            if let Some(inner) = &e.expr {
                ids.extend(rvs_collect_assert_ids_from_expr(inner));
            }
        }
        syn::Expr::Break(e) => {
            if let Some(inner) = &e.expr {
                ids.extend(rvs_collect_assert_ids_from_expr(inner));
            }
        }
        syn::Expr::Let(e) => ids.extend(rvs_collect_assert_ids_from_expr(&e.expr)),
        syn::Expr::Index(e) => {
            ids.extend(rvs_collect_assert_ids_from_expr(&e.expr));
            ids.extend(rvs_collect_assert_ids_from_expr(&e.index));
        }
        syn::Expr::Field(e) => ids.extend(rvs_collect_assert_ids_from_expr(&e.base)),
        syn::Expr::Range(e) => {
            if let Some(s) = &e.start {
                ids.extend(rvs_collect_assert_ids_from_expr(s));
            }
            if let Some(end) = &e.end {
                ids.extend(rvs_collect_assert_ids_from_expr(end));
            }
        }
        syn::Expr::Repeat(e) => {
            ids.extend(rvs_collect_assert_ids_from_expr(&e.expr));
            ids.extend(rvs_collect_assert_ids_from_expr(&e.len));
        }
        syn::Expr::Tuple(e) => {
            for el in &e.elems {
                ids.extend(rvs_collect_assert_ids_from_expr(el));
            }
        }
        syn::Expr::Array(e) => {
            for el in &e.elems {
                ids.extend(rvs_collect_assert_ids_from_expr(el));
            }
        }
        syn::Expr::Struct(e) => {
            for f in &e.fields {
                ids.extend(rvs_collect_assert_ids_from_expr(&f.expr));
            }
        }
        _ => {}
    }
    ids
}

/// 从顶层函数定义中萃取信息。
#[allow(non_snake_case)]
fn rvs_extract_from_item_fn(item_fn: &syn::ItemFn, statics: &[StaticDecl]) -> Option<FnDef> {
    let name = item_fn.sig.ident.to_string();
    let (_, caps) = rvs_parse_function(&name)?;
    let raw_suffix = rvs_extract_raw_suffix(&name);
    let line = item_fn.sig.ident.span().start().line;
    let line_count = rvs_calc_line_count(
        item_fn.sig.fn_token.span,
        item_fn.block.brace_token.span.join(),
    );
    let (calls, static_refs) = rvs_collect_calls_and_statics(&item_fn.block, statics);
    let params = rvs_extract_param_names(&item_fn.sig.inputs);
    let debug_asserted_params = rvs_collect_debug_asserted_params(&item_fn.block);
    let is_async_fn = item_fn.sig.asyncness.is_some();
    let is_unsafe_fn = item_fn.sig.unsafety.is_some();
    let has_unsafe_block = rvs_scan_block_has_unsafe(&item_fn.block);
    let has_panic_macro = rvs_scan_block_has_panic(&item_fn.block);
    let has_mut_self = rvs_has_mut_receiver(&item_fn.sig.inputs);
    let has_mut_param = rvs_has_mut_typed_param(&item_fn.sig.inputs);

    Some(FnDef {
        name,
        capabilities: caps,
        calls,
        static_refs,
        line,
        line_count,
        params,
        debug_asserted_params,
        has_body: true,
        has_unsafe_block,
        is_async_fn,
        is_unsafe_fn,
        has_mut_param,
        has_mut_self,
        has_panic_macro,
        raw_suffix,
        is_test: false,
        allows_dead_code: false,
        has_allow_non_snake_case: false,
    })
}

/// 从 impl 块中的方法萃取信息。
#[allow(non_snake_case)]
fn rvs_extract_from_impl_fn(impl_fn: &syn::ImplItemFn, statics: &[StaticDecl]) -> Option<FnDef> {
    let name = impl_fn.sig.ident.to_string();
    let (_, caps) = rvs_parse_function(&name)?;
    let raw_suffix = rvs_extract_raw_suffix(&name);
    let line = impl_fn.sig.ident.span().start().line;
    let line_count = rvs_calc_line_count(
        impl_fn.sig.fn_token.span,
        impl_fn.block.brace_token.span.join(),
    );
    let (calls, static_refs) = rvs_collect_calls_and_statics(&impl_fn.block, statics);
    let params = rvs_extract_param_names(&impl_fn.sig.inputs);
    let debug_asserted_params = rvs_collect_debug_asserted_params(&impl_fn.block);
    let is_async_fn = impl_fn.sig.asyncness.is_some();
    let is_unsafe_fn = impl_fn.sig.unsafety.is_some();
    let has_unsafe_block = rvs_scan_block_has_unsafe(&impl_fn.block);
    let has_panic_macro = rvs_scan_block_has_panic(&impl_fn.block);
    let has_mut_self = rvs_has_mut_receiver(&impl_fn.sig.inputs);
    let has_mut_param = rvs_has_mut_typed_param(&impl_fn.sig.inputs);

    Some(FnDef {
        name,
        capabilities: caps,
        calls,
        static_refs,
        line,
        line_count,
        params,
        debug_asserted_params,
        has_body: true,
        has_unsafe_block,
        is_async_fn,
        is_unsafe_fn,
        has_mut_param,
        has_mut_self,
        has_panic_macro,
        raw_suffix,
        is_test: false,
        allows_dead_code: false,
        has_allow_non_snake_case: false,
    })
}

/// 从 trait 定义中的方法签名萃取信息。
#[allow(non_snake_case)]
fn rvs_extract_from_trait_fn(trait_fn: &syn::TraitItemFn, statics: &[StaticDecl]) -> Option<FnDef> {
    let name = trait_fn.sig.ident.to_string();
    let (_, caps) = rvs_parse_function(&name)?;
    let raw_suffix = rvs_extract_raw_suffix(&name);
    let line = trait_fn.sig.ident.span().start().line;
    let (calls, static_refs) = trait_fn
        .default
        .as_ref()
        .map(|block| rvs_collect_calls_and_statics(block, statics))
        .unwrap_or_default();
    let line_count = trait_fn
        .default
        .as_ref()
        .map(|block| rvs_calc_line_count(trait_fn.sig.fn_token.span, block.brace_token.span.join()))
        .unwrap_or(1);
    let has_body = trait_fn.default.is_some();
    let (params, debug_asserted_params) = trait_fn
        .default
        .as_ref()
        .map(|block| {
            let params = rvs_extract_param_names(&trait_fn.sig.inputs);
            let debug_asserted_params = rvs_collect_debug_asserted_params(block);
            (params, debug_asserted_params)
        })
        .unwrap_or_default();
    let is_async_fn = trait_fn.sig.asyncness.is_some();
    let is_unsafe_fn = trait_fn.sig.unsafety.is_some();
    let has_unsafe_block = trait_fn
        .default
        .as_ref()
        .is_some_and(rvs_scan_block_has_unsafe);
    let has_panic_macro = trait_fn
        .default
        .as_ref()
        .is_some_and(rvs_scan_block_has_panic);
    let has_mut_self = rvs_has_mut_receiver(&trait_fn.sig.inputs);
    let has_mut_param = rvs_has_mut_typed_param(&trait_fn.sig.inputs);

    Some(FnDef {
        name,
        capabilities: caps,
        calls,
        static_refs,
        line,
        line_count,
        params,
        debug_asserted_params,
        has_body,
        has_unsafe_block,
        is_async_fn,
        is_unsafe_fn,
        has_mut_param,
        has_mut_self,
        has_panic_macro,
        raw_suffix,
        is_test: false,
        allows_dead_code: false,
        has_allow_non_snake_case: false,
    })
}

/// 判断一个宏是否为 panic 类宏（panic!/assert!/assert_eq!/assert_ne!/unreachable!/todo!/unimplemented!），
/// 但排除 debug_assert! 系列。
///
/// 注意：方法调用 `.unwrap()` / `.expect()` 的检测在 `rvs_scan_expr_has_panic` 的
/// `MethodCall` 分支中完成，不在此函数范围内。
fn rvs_is_panic_macro(mac: &syn::Macro) -> bool {
    let name = mac
        .path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();
    matches!(
        name.as_str(),
        "panic" | "assert" | "assert_eq" | "assert_ne" | "unreachable" | "todo" | "unimplemented"
    )
}

/// 扫描块中是否存在 unsafe 块。
fn rvs_scan_block_has_unsafe(block: &syn::Block) -> bool {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    if rvs_scan_expr_has_unsafe(&init.expr) {
                        return true;
                    }
                    if let Some((_, diverge)) = &init.diverge
                        && rvs_scan_expr_has_unsafe(diverge)
                    {
                        return true;
                    }
                }
            }
            syn::Stmt::Expr(expr, _) => {
                if rvs_scan_expr_has_unsafe(expr) {
                    return true;
                }
            }
            syn::Stmt::Item(_) | syn::Stmt::Macro(_) => {}
        }
    }
    false
}

fn rvs_scan_expr_has_unsafe(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Unsafe(_) => true,
        syn::Expr::Block(b) => rvs_scan_block_has_unsafe(&b.block),
        syn::Expr::If(e) => {
            rvs_scan_expr_has_unsafe(&e.cond)
                || rvs_scan_block_has_unsafe(&e.then_branch)
                || e.else_branch
                    .as_ref()
                    .is_some_and(|(_, els)| rvs_scan_expr_has_unsafe(els))
        }
        syn::Expr::Match(e) => {
            rvs_scan_expr_has_unsafe(&e.expr)
                || e.arms.iter().any(|arm| {
                    arm.guard
                        .as_ref()
                        .is_some_and(|(_, g)| rvs_scan_expr_has_unsafe(g))
                        || rvs_scan_expr_has_unsafe(&arm.body)
                })
        }
        syn::Expr::Loop(e) => rvs_scan_block_has_unsafe(&e.body),
        syn::Expr::While(e) => {
            rvs_scan_expr_has_unsafe(&e.cond) || rvs_scan_block_has_unsafe(&e.body)
        }
        syn::Expr::ForLoop(e) => {
            rvs_scan_expr_has_unsafe(&e.expr) || rvs_scan_block_has_unsafe(&e.body)
        }
        syn::Expr::Closure(c) => rvs_scan_expr_has_unsafe(&c.body),
        syn::Expr::Call(c) => {
            rvs_scan_expr_has_unsafe(&c.func) || c.args.iter().any(rvs_scan_expr_has_unsafe)
        }
        syn::Expr::MethodCall(c) => {
            rvs_scan_expr_has_unsafe(&c.receiver) || c.args.iter().any(rvs_scan_expr_has_unsafe)
        }
        syn::Expr::Assign(a) => {
            rvs_scan_expr_has_unsafe(&a.left) || rvs_scan_expr_has_unsafe(&a.right)
        }
        syn::Expr::Binary(b) => {
            rvs_scan_expr_has_unsafe(&b.left) || rvs_scan_expr_has_unsafe(&b.right)
        }
        syn::Expr::Unary(u) => rvs_scan_expr_has_unsafe(&u.expr),
        syn::Expr::Paren(p) => rvs_scan_expr_has_unsafe(&p.expr),
        syn::Expr::Group(g) => rvs_scan_expr_has_unsafe(&g.expr),
        syn::Expr::Reference(r) => rvs_scan_expr_has_unsafe(&r.expr),
        syn::Expr::Try(t) => rvs_scan_expr_has_unsafe(&t.expr),
        syn::Expr::Await(a) => rvs_scan_expr_has_unsafe(&a.base),
        syn::Expr::Return(r) => r.expr.as_ref().is_some_and(|e| rvs_scan_expr_has_unsafe(e)),
        syn::Expr::Break(b) => b.expr.as_ref().is_some_and(|e| rvs_scan_expr_has_unsafe(e)),
        syn::Expr::Let(l) => rvs_scan_expr_has_unsafe(&l.expr),
        syn::Expr::Index(i) => {
            rvs_scan_expr_has_unsafe(&i.expr) || rvs_scan_expr_has_unsafe(&i.index)
        }
        syn::Expr::Field(f) => rvs_scan_expr_has_unsafe(&f.base),
        syn::Expr::Range(r) => {
            r.start
                .as_ref()
                .is_some_and(|s| rvs_scan_expr_has_unsafe(s))
                || r.end.as_ref().is_some_and(|e| rvs_scan_expr_has_unsafe(e))
        }
        syn::Expr::Repeat(r) => {
            rvs_scan_expr_has_unsafe(&r.expr) || rvs_scan_expr_has_unsafe(&r.len)
        }
        syn::Expr::Tuple(t) => t.elems.iter().any(rvs_scan_expr_has_unsafe),
        syn::Expr::Array(a) => a.elems.iter().any(rvs_scan_expr_has_unsafe),
        syn::Expr::Struct(s) => {
            s.fields.iter().any(|f| rvs_scan_expr_has_unsafe(&f.expr))
                || s.rest.as_ref().is_some_and(|r| rvs_scan_expr_has_unsafe(r))
        }
        syn::Expr::Async(a) => rvs_scan_block_has_unsafe(&a.block),
        syn::Expr::Cast(c) => rvs_scan_expr_has_unsafe(&c.expr),
        syn::Expr::TryBlock(t) => rvs_scan_block_has_unsafe(&t.block),
        syn::Expr::Path(_)
        | syn::Expr::Lit(_)
        | syn::Expr::Continue(_)
        | syn::Expr::Macro(_)
        | syn::Expr::Verbatim(_) => false,
        _ => false,
    }
}

/// 扫描块中是否存在 panic 类宏调用。
fn rvs_scan_block_has_panic(block: &syn::Block) -> bool {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Macro(m) if rvs_is_panic_macro(&m.mac) => return true,
            syn::Stmt::Expr(expr, _) => {
                if rvs_scan_expr_has_panic(expr) {
                    return true;
                }
            }
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    if rvs_scan_expr_has_panic(&init.expr) {
                        return true;
                    }
                    if let Some((_, diverge)) = &init.diverge
                        && rvs_scan_expr_has_panic(diverge)
                    {
                        return true;
                    }
                }
            }
            syn::Stmt::Item(_) | syn::Stmt::Macro(_) => {}
        }
    }
    false
}

fn rvs_scan_expr_has_panic(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Macro(m) if rvs_is_panic_macro(&m.mac) => true,
        syn::Expr::Block(b) => rvs_scan_block_has_panic(&b.block),
        syn::Expr::If(e) => {
            rvs_scan_expr_has_panic(&e.cond)
                || rvs_scan_block_has_panic(&e.then_branch)
                || e.else_branch
                    .as_ref()
                    .is_some_and(|(_, els)| rvs_scan_expr_has_panic(els))
        }
        syn::Expr::Match(e) => {
            rvs_scan_expr_has_panic(&e.expr)
                || e.arms.iter().any(|arm| {
                    arm.guard
                        .as_ref()
                        .is_some_and(|(_, g)| rvs_scan_expr_has_panic(g))
                        || rvs_scan_expr_has_panic(&arm.body)
                })
        }
        syn::Expr::Loop(e) => rvs_scan_block_has_panic(&e.body),
        syn::Expr::While(e) => {
            rvs_scan_expr_has_panic(&e.cond) || rvs_scan_block_has_panic(&e.body)
        }
        syn::Expr::ForLoop(e) => {
            rvs_scan_expr_has_panic(&e.expr) || rvs_scan_block_has_panic(&e.body)
        }
        syn::Expr::Closure(c) => rvs_scan_expr_has_panic(&c.body),
        syn::Expr::Call(c) => {
            rvs_scan_expr_has_panic(&c.func) || c.args.iter().any(rvs_scan_expr_has_panic)
        }
        syn::Expr::MethodCall(c) => {
            let is_panic_method = matches!(c.method.to_string().as_str(), "unwrap" | "expect");
            is_panic_method
                || rvs_scan_expr_has_panic(&c.receiver)
                || c.args.iter().any(rvs_scan_expr_has_panic)
        }
        syn::Expr::Assign(a) => {
            rvs_scan_expr_has_panic(&a.left) || rvs_scan_expr_has_panic(&a.right)
        }
        syn::Expr::Binary(b) => {
            rvs_scan_expr_has_panic(&b.left) || rvs_scan_expr_has_panic(&b.right)
        }
        syn::Expr::Unary(u) => rvs_scan_expr_has_panic(&u.expr),
        syn::Expr::Paren(p) => rvs_scan_expr_has_panic(&p.expr),
        syn::Expr::Group(g) => rvs_scan_expr_has_panic(&g.expr),
        syn::Expr::Reference(r) => rvs_scan_expr_has_panic(&r.expr),
        syn::Expr::Try(t) => rvs_scan_expr_has_panic(&t.expr),
        syn::Expr::Await(a) => rvs_scan_expr_has_panic(&a.base),
        syn::Expr::Return(r) => r.expr.as_ref().is_some_and(|e| rvs_scan_expr_has_panic(e)),
        syn::Expr::Break(b) => b.expr.as_ref().is_some_and(|e| rvs_scan_expr_has_panic(e)),
        syn::Expr::Let(l) => rvs_scan_expr_has_panic(&l.expr),
        syn::Expr::Index(i) => {
            rvs_scan_expr_has_panic(&i.expr) || rvs_scan_expr_has_panic(&i.index)
        }
        syn::Expr::Field(f) => rvs_scan_expr_has_panic(&f.base),
        syn::Expr::Range(r) => {
            r.start.as_ref().is_some_and(|s| rvs_scan_expr_has_panic(s))
                || r.end.as_ref().is_some_and(|e| rvs_scan_expr_has_panic(e))
        }
        syn::Expr::Repeat(r) => rvs_scan_expr_has_panic(&r.expr) || rvs_scan_expr_has_panic(&r.len),
        syn::Expr::Tuple(t) => t.elems.iter().any(rvs_scan_expr_has_panic),
        syn::Expr::Array(a) => a.elems.iter().any(rvs_scan_expr_has_panic),
        syn::Expr::Struct(s) => {
            s.fields.iter().any(|f| rvs_scan_expr_has_panic(&f.expr))
                || s.rest.as_ref().is_some_and(|r| rvs_scan_expr_has_panic(r))
        }
        syn::Expr::Async(a) => rvs_scan_block_has_panic(&a.block),
        syn::Expr::Cast(c) => rvs_scan_expr_has_panic(&c.expr),
        syn::Expr::TryBlock(t) => rvs_scan_block_has_panic(&t.block),
        syn::Expr::Unsafe(u) => rvs_scan_block_has_panic(&u.block),
        syn::Expr::Path(_)
        | syn::Expr::Lit(_)
        | syn::Expr::Continue(_)
        | syn::Expr::Macro(_)
        | syn::Expr::Verbatim(_) => false,
        _ => false,
    }
}

fn rvs_is_test_fn(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|s| s.ident == "test")
    })
}

fn rvs_is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .is_some_and(|s| s.ident == "cfg")
            && attr
                .meta
                .require_list()
                .is_ok_and(|list| list.tokens.to_string().contains("test"))
    })
}

fn rvs_allows_dead_code(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr
            .path()
            .segments
            .last()
            .is_some_and(|s| s.ident == "allow")
        {
            return false;
        }
        let Some(list) = attr.meta.require_list().ok() else {
            return false;
        };
        let tokens = list.tokens.to_string();
        tokens.contains("dead_code") || tokens.contains("unused")
    })
}

/// 判断属性列表中是否有 `#[allow(non_snake_case)]`（含 `#![allow(non_snake_case)]`）。
/// `allow(a, non_snake_case, b)` 这类组合亦可识别。
fn rvs_allows_non_snake_case(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr
            .path()
            .segments
            .last()
            .is_some_and(|s| s.ident == "allow")
        {
            return false;
        }
        let Some(list) = attr.meta.require_list().ok() else {
            return false;
        };
        let tokens = list.tokens.to_string();
        tokens.contains("non_snake_case")
    })
}

/// 从顶层项中萃取 rvs_ 函数定义，递归进入 mod 块。
/// 跳过 #[cfg(test)] 模块和 #[test] 函数。
/// 标记 #[allow(dead_code)] / #[allow(unused)] 的函数。
/// `inherited_allow_snake` 自文件/外层 impl/trait/mod 传入，用于表达"外层已开过豁免"。
fn rvs_extract_from_items(
    items: &[syn::Item],
    statics: &[StaticDecl],
    inherited_allow_snake: bool,
) -> Vec<FnDef> {
    let mut functions = Vec::new();

    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                if rvs_is_test_fn(&item_fn.attrs) {
                    continue;
                }
                if let Some(mut fn_def) = rvs_extract_from_item_fn(item_fn, statics) {
                    fn_def.allows_dead_code = rvs_allows_dead_code(&item_fn.attrs);
                    fn_def.has_allow_non_snake_case =
                        inherited_allow_snake || rvs_allows_non_snake_case(&item_fn.attrs);
                    functions.push(fn_def);
                }
            }
            syn::Item::Impl(item_impl) => {
                if rvs_is_cfg_test(&item_impl.attrs) {
                    continue;
                }
                let impl_allows_dead_code = rvs_allows_dead_code(&item_impl.attrs);
                let impl_allows_snake =
                    inherited_allow_snake || rvs_allows_non_snake_case(&item_impl.attrs);
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item
                        && !rvs_is_test_fn(&impl_fn.attrs)
                        && let Some(mut fn_def) = rvs_extract_from_impl_fn(impl_fn, statics)
                    {
                        fn_def.allows_dead_code =
                            impl_allows_dead_code || rvs_allows_dead_code(&impl_fn.attrs);
                        fn_def.has_allow_non_snake_case =
                            impl_allows_snake || rvs_allows_non_snake_case(&impl_fn.attrs);
                        functions.push(fn_def);
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                let trait_allows_snake =
                    inherited_allow_snake || rvs_allows_non_snake_case(&item_trait.attrs);
                for trait_item in &item_trait.items {
                    if let syn::TraitItem::Fn(trait_fn) = trait_item
                        && !rvs_is_test_fn(&trait_fn.attrs)
                        && let Some(mut fn_def) = rvs_extract_from_trait_fn(trait_fn, statics)
                    {
                        fn_def.allows_dead_code = rvs_allows_dead_code(&trait_fn.attrs);
                        fn_def.has_allow_non_snake_case =
                            trait_allows_snake || rvs_allows_non_snake_case(&trait_fn.attrs);
                        functions.push(fn_def);
                    }
                }
            }
            syn::Item::Mod(m) => {
                if rvs_is_cfg_test(&m.attrs) {
                    continue;
                }
                let mod_allows_snake = inherited_allow_snake || rvs_allows_non_snake_case(&m.attrs);
                if let Some((_, inner_items)) = &m.content {
                    functions.extend(rvs_extract_from_items(
                        inner_items,
                        statics,
                        mod_allows_snake,
                    ));
                }
            }
            _ => {}
        }
    }

    functions
}

/// 搜集源码中所有 `#[test]` 函数名与所在行。
/// 递归进入 mod 块（含 `#[cfg(test)]`）与 impl 块，不漏一只。
/// 纯函数：只看不改。
fn rvs_collect_tests_from_items(items: &[syn::Item]) -> Vec<TestName> {
    let mut tests = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) if rvs_is_test_fn(&item_fn.attrs) => {
                let name = item_fn.sig.ident.to_string();
                let line = item_fn.sig.ident.span().start().line;
                tests.push(TestName { name, line });
            }
            syn::Item::Impl(item_impl) => {
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(f) = impl_item
                        && rvs_is_test_fn(&f.attrs)
                    {
                        let name = f.sig.ident.to_string();
                        let line = f.sig.ident.span().start().line;
                        tests.push(TestName { name, line });
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    tests.extend(rvs_collect_tests_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    tests
}

/// 从顶层项中萃取 use 导入语句，递归进入 mod 块。
fn rvs_collect_imports_from_items(items: &[syn::Item]) -> Vec<ImportInfo> {
    let mut imports = Vec::new();
    for item in items {
        match item {
            syn::Item::Use(item_use) => {
                let line = item_use.span().start().line;
                let paths = rvs_extract_use_paths(&item_use.tree);
                for path in paths {
                    imports.push(ImportInfo {
                        use_path: path,
                        line,
                    });
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    imports.extend(rvs_collect_imports_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    imports
}

/// 从 use 树的单个项中提取完整路径。
fn rvs_extract_use_paths(tree: &syn::UseTree) -> Vec<String> {
    let mut paths = Vec::new();
    match tree {
        syn::UseTree::Path(path) => {
            let prefix = path.ident.to_string();
            let sub_paths = rvs_extract_use_paths(&path.tree);
            for sub in sub_paths {
                paths.push(format!("{}::{}", prefix, sub));
            }
        }
        syn::UseTree::Name(name) => {
            paths.push(name.ident.to_string());
        }
        syn::UseTree::Rename(rename) => {
            paths.push(format!("{} as {}", rename.ident, rename.rename));
        }
        syn::UseTree::Glob(_) => {
            paths.push("*".to_string());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                paths.extend(rvs_extract_use_paths(item));
            }
        }
    }
    paths
}

/// 从一段源码中萃取所有 use 导入语句。
pub fn rvs_extract_imports(source: &str) -> Result<Vec<ImportInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;

    Ok(rvs_collect_imports_from_items(&file.items))
}

/// 从一段源码中萃取所有 rvs_ 函数定义。
/// 顶层函数、impl 方法、trait 方法、mod 块内函数，一网打尽。
/// 同时搜集文件中的 static 与 thread_local! 声明，
/// 据此检查函数体内的静态变量引用。
pub fn rvs_extract_functions(source: &str) -> Result<Vec<FnDef>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;

    let statics = rvs_collect_static_decls_from_items(&file.items);
    let file_allows_snake = rvs_allows_non_snake_case(&file.attrs);

    Ok(rvs_extract_from_items(
        &file.items,
        &statics,
        file_allows_snake,
    ))
}

/// 从一段源码中萃取所有 `#[test]` 函数的名字与行号。
/// 不过滤 `#[cfg(test)]` 模块——测试函数往往正藏其中。
pub fn rvs_extract_tests(source: &str) -> Result<Vec<TestName>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;

    Ok(rvs_collect_tests_from_items(&file.items))
}

/// 非 rvs_ 前缀函数信息：用于检查缺少 rvs_ 前缀的函数。
#[derive(Debug, Clone)]
pub struct NonRvsFnInfo {
    pub name: String,
    pub line: usize,
    pub has_rvs_prefix: bool,
}

/// 从顶层项中收集所有函数（pub 和非 pub）信息。
/// 包括顶层函数、impl 方法、mod 块内函数，递归收集。
/// 跳过 #[test] 函数、main 函数、trait impl 方法。
fn rvs_collect_non_rvs_fns_from_items(items: &[syn::Item]) -> Vec<NonRvsFnInfo> {
    let mut fns = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                let is_test = rvs_is_test_fn(&item_fn.attrs);
                let name = item_fn.sig.ident.to_string();
                let is_main = name == "main";
                if !is_test && !is_main {
                    let line = item_fn.sig.ident.span().start().line;
                    fns.push(NonRvsFnInfo {
                        has_rvs_prefix: name.starts_with("rvs_"),
                        name,
                        line,
                    });
                }
            }
            syn::Item::Impl(item_impl) => {
                if item_impl.trait_.is_some() {
                    continue;
                }
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item {
                        let is_test = rvs_is_test_fn(&impl_fn.attrs);
                        if !is_test {
                            let name = impl_fn.sig.ident.to_string();
                            let line = impl_fn.sig.ident.span().start().line;
                            fns.push(NonRvsFnInfo {
                                has_rvs_prefix: name.starts_with("rvs_"),
                                name,
                                line,
                            });
                        }
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    fns.extend(rvs_collect_non_rvs_fns_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    fns
}

/// 从一段源码中萃取所有函数（pub 和非 pub），用于检查 rvs_ 前缀。
/// 返回的函数名不包括 rvs_ 前缀的，用于生成警告。
pub fn rvs_extract_non_rvs_fns(source: &str) -> Result<Vec<NonRvsFnInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;

    Ok(rvs_collect_non_rvs_fns_from_items(&file.items))
}

/// 公开项信息：用于检查 pub 函数/方法是否有文档注释。
#[derive(Debug, Clone)]
pub struct PubItemInfo {
    pub name: String,
    pub line: usize,
    pub has_doc: bool,
}

/// 判断一组属性中是否含有文档注释（/// 或 #[doc = "..."]）。
fn rvs_has_doc_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("doc"))
}

/// 从顶层项中收集所有 pub 函数/方法信息。
/// 包括顶层 pub fn、pub mod 内的 pub fn、impl 块内的 pub fn；
/// 跳过 trait 实现方法（由 trait 自身负责文档）、#[test] 函数。
fn rvs_collect_pub_items_from_items(items: &[syn::Item]) -> Vec<PubItemInfo> {
    let mut pubs = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                let is_pub = matches!(item_fn.vis, syn::Visibility::Public(_));
                let is_test = rvs_is_test_fn(&item_fn.attrs);
                if is_pub && !is_test {
                    let name = item_fn.sig.ident.to_string();
                    let line = item_fn.sig.ident.span().start().line;
                    pubs.push(PubItemInfo {
                        has_doc: rvs_has_doc_attr(&item_fn.attrs),
                        name,
                        line,
                    });
                }
            }
            syn::Item::Impl(item_impl) => {
                // trait 实现方法由 trait 提供文档，不做独立检查
                if item_impl.trait_.is_some() {
                    continue;
                }
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item {
                        let is_pub = matches!(impl_fn.vis, syn::Visibility::Public(_));
                        let is_test = rvs_is_test_fn(&impl_fn.attrs);
                        if is_pub && !is_test {
                            let name = impl_fn.sig.ident.to_string();
                            let line = impl_fn.sig.ident.span().start().line;
                            pubs.push(PubItemInfo {
                                has_doc: rvs_has_doc_attr(&impl_fn.attrs),
                                name,
                                line,
                            });
                        }
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    pubs.extend(rvs_collect_pub_items_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    pubs
}

/// 从一段源码中萃取所有 pub 函数/方法（用于检查文档注释）。
pub fn rvs_extract_pub_items(source: &str) -> Result<Vec<PubItemInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;

    Ok(rvs_collect_pub_items_from_items(&file.items))
}

/// 借用类型参数信息：函数名、参数名、原类型、建议类型、行号。
#[derive(Debug, Clone)]
pub struct BorrowedParamInfo {
    pub function: String,
    pub param: String,
    pub original: String,
    pub suggestion: String,
    pub line: usize,
}

/// unsafe 函数信息：名字、行号、是否有 `/// # Safety` 文档。
#[derive(Debug, Clone)]
pub struct UnsafeFnInfo {
    pub name: String,
    pub line: usize,
    pub has_safety_doc: bool,
}

/// 检测类型是否为 `&String` / `&Vec<T>` / `&Box<T>`，返回 (原类型, 建议类型)。
fn rvs_check_borrowed_type(ty: &syn::Type) -> Option<(String, String)> {
    let syn::Type::Reference(ref_expr) = ty else {
        return None;
    };
    if ref_expr.mutability.is_some() {
        return None;
    }
    let syn::Type::Path(type_path) = &*ref_expr.elem else {
        return None;
    };
    let last_ident = type_path.path.segments.last()?.ident.to_string();
    match last_ident.as_str() {
        "String" => Some(("&String".to_string(), "&str".to_string())),
        "Vec" => Some(("&Vec<T>".to_string(), "&[T]".to_string())),
        "Box" => Some(("&Box<T>".to_string(), "&T".to_string())),
        _ => None,
    }
}

/// 判断一组属性中是否含有 `/// # Safety` 文档小节。
fn rvs_has_safety_doc(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("doc") {
            return false;
        }
        if let Ok(nv) = a.meta.require_name_value()
            && let syn::Expr::Lit(expr_lit) = &nv.value
            && let syn::Lit::Str(lit_str) = &expr_lit.lit
        {
            return lit_str.value().trim().starts_with("# Safety");
        }
        false
    })
}

/// 从顶层项中收集所有借用类型参数（`&String`/`&Vec<T>`/`&Box<T>`）。
fn rvs_collect_borrowed_params_from_items(items: &[syn::Item]) -> Vec<BorrowedParamInfo> {
    let mut result = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                let name = item_fn.sig.ident.to_string();
                let line = item_fn.sig.ident.span().start().line;
                for arg in &item_fn.sig.inputs {
                    if let syn::FnArg::Typed(pt) = arg
                        && let Some((original, suggestion)) = rvs_check_borrowed_type(&pt.ty)
                        && let syn::Pat::Ident(pat_ident) = &*pt.pat
                    {
                        result.push(BorrowedParamInfo {
                            function: name.clone(),
                            param: pat_ident.ident.to_string(),
                            original,
                            suggestion,
                            line,
                        });
                    }
                }
            }
            syn::Item::Impl(item_impl) => {
                if item_impl.trait_.is_some() {
                    continue;
                }
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item {
                        let name = impl_fn.sig.ident.to_string();
                        let line = impl_fn.sig.ident.span().start().line;
                        for arg in &impl_fn.sig.inputs {
                            if let syn::FnArg::Typed(pt) = arg
                                && let Some((original, suggestion)) =
                                    rvs_check_borrowed_type(&pt.ty)
                                && let syn::Pat::Ident(pat_ident) = &*pt.pat
                            {
                                result.push(BorrowedParamInfo {
                                    function: name.clone(),
                                    param: pat_ident.ident.to_string(),
                                    original,
                                    suggestion,
                                    line,
                                });
                            }
                        }
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for trait_item in &item_trait.items {
                    if let syn::TraitItem::Fn(trait_fn) = trait_item {
                        let name = trait_fn.sig.ident.to_string();
                        let line = trait_fn.sig.ident.span().start().line;
                        for arg in &trait_fn.sig.inputs {
                            if let syn::FnArg::Typed(pt) = arg
                                && let Some((original, suggestion)) =
                                    rvs_check_borrowed_type(&pt.ty)
                                && let syn::Pat::Ident(pat_ident) = &*pt.pat
                            {
                                result.push(BorrowedParamInfo {
                                    function: name.clone(),
                                    param: pat_ident.ident.to_string(),
                                    original,
                                    suggestion,
                                    line,
                                });
                            }
                        }
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    result.extend(rvs_collect_borrowed_params_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    result
}

/// 从顶层项中收集所有 unsafe 函数及其 `/// # Safety` 文档情况。
fn rvs_collect_unsafe_fns_from_items(items: &[syn::Item]) -> Vec<UnsafeFnInfo> {
    let mut result = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) if item_fn.sig.unsafety.is_some() => {
                let name = item_fn.sig.ident.to_string();
                let line = item_fn.sig.ident.span().start().line;
                result.push(UnsafeFnInfo {
                    name,
                    line,
                    has_safety_doc: rvs_has_safety_doc(&item_fn.attrs),
                });
            }
            syn::Item::Impl(item_impl) => {
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item
                        && impl_fn.sig.unsafety.is_some()
                    {
                        let name = impl_fn.sig.ident.to_string();
                        let line = impl_fn.sig.ident.span().start().line;
                        result.push(UnsafeFnInfo {
                            name,
                            line,
                            has_safety_doc: rvs_has_safety_doc(&impl_fn.attrs),
                        });
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for trait_item in &item_trait.items {
                    if let syn::TraitItem::Fn(trait_fn) = trait_item
                        && trait_fn.sig.unsafety.is_some()
                    {
                        let name = trait_fn.sig.ident.to_string();
                        let line = trait_fn.sig.ident.span().start().line;
                        result.push(UnsafeFnInfo {
                            name,
                            line,
                            has_safety_doc: rvs_has_safety_doc(&trait_fn.attrs),
                        });
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    result.extend(rvs_collect_unsafe_fns_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    result
}

/// 检测文件级属性中是否有 `#![deny(warnings)]`，返回其行号。
fn rvs_find_deny_warnings(attrs: &[syn::Attribute]) -> Option<usize> {
    attrs.iter().find_map(|attr| {
        if !attr
            .path()
            .segments
            .last()
            .is_some_and(|s| s.ident == "deny")
        {
            return None;
        }
        let Ok(list) = attr.meta.require_list() else {
            return None;
        };
        let tokens = list.tokens.to_string();
        if tokens.split(',').any(|t| t.trim() == "warnings") {
            Some(attr.span().start().line)
        } else {
            None
        }
    })
}

/// 从一段源码中萃取所有借用类型参数（`&String`/`&Vec<T>`/`&Box<T>`）。
pub fn rvs_extract_borrowed_params(source: &str) -> Result<Vec<BorrowedParamInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;

    Ok(rvs_collect_borrowed_params_from_items(&file.items))
}

/// 从一段源码中萃取所有 unsafe 函数及其 `/// # Safety` 文档情况。
pub fn rvs_extract_unsafe_fns(source: &str) -> Result<Vec<UnsafeFnInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;

    Ok(rvs_collect_unsafe_fns_from_items(&file.items))
}

/// 检测源码中是否有 `#![deny(warnings)]`，返回其行号。
pub fn rvs_extract_deny_warnings(source: &str) -> Result<Option<usize>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;

    Ok(rvs_find_deny_warnings(&file.attrs))
}

/// 公开类型缺少 `Debug` derive 的信息：类型名、行号。
#[derive(Debug, Clone)]
pub struct MissingDebugInfo {
    pub name: String,
    pub line: usize,
}

/// `rvs_` 函数带 `P` 标记但缺少 `/// # Panics` 文档的信息：函数名、行号。
#[derive(Debug, Clone)]
pub struct MissingPanicsDocInfo {
    pub function: String,
    pub line: usize,
}

/// 直接实现 `Into` 而非 `From` 的信息：实现类型、目标类型、行号。
#[derive(Debug, Clone)]
pub struct IntoImplInfo {
    pub impl_type: String,
    pub target_type: String,
    pub line: usize,
}

/// 返回类型为 `Result<(), E>` 且消费了 `T` 类型参数但错误类型中不含 `T` 的信息。
#[derive(Debug, Clone)]
pub struct ConsumedArgOnErrorInfo {
    pub function: String,
    pub param: String,
    pub param_type: String,
    pub line: usize,
}

/// `impl Deref for X { Target = Y }` 反模式信息：实现类型、目标类型、行号。
#[derive(Debug, Clone)]
pub struct DerefPolymorphismInfo {
    pub impl_type: String,
    pub target_type: String,
    pub line: usize,
}

/// 使用了 `std::any::Any` / `type_name` / `type_id` 的信息：函数名、调用的路径、行号。
#[derive(Debug, Clone)]
pub struct ReflectionUsageInfo {
    pub function: String,
    pub path: String,
    pub line: usize,
}

const REFLECTION_PATHS: &[&str] = &[
    "std::any::Any",
    "std::any::type_name",
    "std::any::type_id",
    "any::Any",
    "any::type_name",
    "any::type_id",
];

fn rvs_has_debug_derive(attrs: &[syn::Attribute]) -> bool {
    for a in attrs {
        if a.path().is_ident("derive")
            && let Ok(nested) = a.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
        {
            for path in nested {
                if path.is_ident("Debug") {
                    return true;
                }
            }
        }
    }
    false
}

fn rvs_has_panics_doc(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("doc") {
            return false;
        }
        if let Ok(nv) = a.meta.require_name_value()
            && let syn::Expr::Lit(expr_lit) = &nv.value
            && let syn::Lit::Str(lit_str) = &expr_lit.lit
        {
            return lit_str.value().trim().starts_with("# Panics");
        }
        false
    })
}

fn rvs_collect_missing_debug_from_items(items: &[syn::Item]) -> Vec<MissingDebugInfo> {
    let mut result = Vec::new();
    for item in items {
        match item {
            syn::Item::Struct(s)
                if matches!(s.vis, syn::Visibility::Public(_))
                    && !rvs_has_debug_derive(&s.attrs) =>
            {
                result.push(MissingDebugInfo {
                    name: s.ident.to_string(),
                    line: s.ident.span().start().line,
                });
            }
            syn::Item::Enum(e)
                if matches!(e.vis, syn::Visibility::Public(_))
                    && !rvs_has_debug_derive(&e.attrs) =>
            {
                result.push(MissingDebugInfo {
                    name: e.ident.to_string(),
                    line: e.ident.span().start().line,
                });
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    result.extend(rvs_collect_missing_debug_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    result
}

fn rvs_collect_missing_panics_doc_from_items(items: &[syn::Item]) -> Vec<MissingPanicsDocInfo> {
    let mut result = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                let name = item_fn.sig.ident.to_string();
                if let Some((_, caps)) = rvs_parse_function(&name)
                    && caps.rvs_contains(Capability::P)
                    && !rvs_has_panics_doc(&item_fn.attrs)
                {
                    result.push(MissingPanicsDocInfo {
                        function: name,
                        line: item_fn.sig.ident.span().start().line,
                    });
                }
            }
            syn::Item::Impl(item_impl) => {
                if item_impl.trait_.is_some() {
                    continue;
                }
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item {
                        let name = impl_fn.sig.ident.to_string();
                        if let Some((_, caps)) = rvs_parse_function(&name)
                            && caps.rvs_contains(Capability::P)
                            && !rvs_has_panics_doc(&impl_fn.attrs)
                        {
                            result.push(MissingPanicsDocInfo {
                                function: name,
                                line: impl_fn.sig.ident.span().start().line,
                            });
                        }
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    result.extend(rvs_collect_missing_panics_doc_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    result
}

fn rvs_collect_into_impls_from_items(items: &[syn::Item]) -> Vec<IntoImplInfo> {
    let mut result = Vec::new();
    for item in items {
        if let syn::Item::Impl(impl_block) = item
            && let Some((_, trait_path, _)) = &impl_block.trait_
            && let Some(seg) = trait_path.segments.last()
            && seg.ident == "Into"
        {
            let impl_type = rvs_type_name(&impl_block.self_ty);
            let target_type = rvs_type_name_from_path(&seg.arguments);
            result.push(IntoImplInfo {
                impl_type,
                target_type,
                line: seg.ident.span().start().line,
            });
        }
    }
    result
}

fn rvs_type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn rvs_type_name_from_path(args: &syn::PathArguments) -> String {
    match args {
        syn::PathArguments::AngleBracketed(ab) => ab.args.first().map_or_else(String::new, |arg| {
            if let syn::GenericArgument::Type(ty) = arg {
                rvs_type_name(ty)
            } else {
                String::new()
            }
        }),
        _ => String::new(),
    }
}

fn rvs_type_ident(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(p) = ty {
        p.path.segments.last().map(|s| s.ident.to_string())
    } else {
        None
    }
}

fn rvs_collect_consumed_arg_on_error_from_items(
    items: &[syn::Item],
) -> Vec<ConsumedArgOnErrorInfo> {
    let mut result = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                let name = item_fn.sig.ident.to_string();
                let line = item_fn.sig.ident.span().start().line;
                if let Some(info) = rvs_find_consumed_args(&item_fn.sig, &name, line) {
                    result.push(info);
                }
            }
            syn::Item::Impl(item_impl) => {
                if item_impl.trait_.is_some() {
                    continue;
                }
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item {
                        let name = impl_fn.sig.ident.to_string();
                        let line = impl_fn.sig.ident.span().start().line;
                        if let Some(info) = rvs_find_consumed_args(&impl_fn.sig, &name, line) {
                            result.push(info);
                        }
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for trait_item in &item_trait.items {
                    if let syn::TraitItem::Fn(tfn) = trait_item {
                        let name = tfn.sig.ident.to_string();
                        let line = tfn.sig.ident.span().start().line;
                        if let Some(info) = rvs_find_consumed_args(&tfn.sig, &name, line) {
                            result.push(info);
                        }
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    result.extend(rvs_collect_consumed_arg_on_error_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    result
}

fn rvs_find_consumed_args(
    sig: &syn::Signature,
    name: &str,
    line: usize,
) -> Option<ConsumedArgOnErrorInfo> {
    debug_assert!(line > 0);
    let ret = &sig.output;
    let syn::ReturnType::Type(_, ret_ty) = ret else {
        return None;
    };
    let syn::Type::Path(type_path) = ret_ty.as_ref() else {
        return None;
    };
    let last_seg = type_path.path.segments.last()?;
    if last_seg.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(ab) = &last_seg.arguments else {
        return None;
    };
    if ab.args.len() != 2 {
        return None;
    };
    let syn::GenericArgument::Type(ok_ty) = &ab.args[0] else {
        return None;
    };
    let syn::GenericArgument::Type(err_ty) = &ab.args[1] else {
        return None;
    };
    let is_unit = matches!(ok_ty, syn::Type::Tuple(t) if t.elems.is_empty());
    if is_unit {
        let err_idents = rvs_collect_type_idents(err_ty);
        for arg in &sig.inputs {
            if let syn::FnArg::Typed(pt) = arg
                && !matches!(pt.ty.as_ref(), syn::Type::Reference(_))
                && let syn::Pat::Ident(pat_ident) = &*pt.pat
                && let Some(arg_ident) = rvs_type_ident(&pt.ty)
                && !err_idents.contains(&arg_ident)
            {
                return Some(ConsumedArgOnErrorInfo {
                    function: name.to_string(),
                    param: pat_ident.ident.to_string(),
                    param_type: arg_ident,
                    line,
                });
            }
        }
    }
    None
}

fn rvs_collect_type_idents(ty: &syn::Type) -> Vec<String> {
    let mut idents = Vec::new();
    match ty {
        syn::Type::Path(p) => {
            for seg in &p.path.segments {
                idents.push(seg.ident.to_string());
                if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                    for arg in &ab.args {
                        if let syn::GenericArgument::Type(inner) = arg {
                            idents.extend(rvs_collect_type_idents(inner));
                        }
                    }
                }
            }
        }
        syn::Type::Tuple(t) => {
            for inner in &t.elems {
                idents.extend(rvs_collect_type_idents(inner));
            }
        }
        syn::Type::Reference(r) => {
            idents.extend(rvs_collect_type_idents(&r.elem));
        }
        _ => {}
    }
    idents
}

fn rvs_collect_deref_polymorphism_from_items(items: &[syn::Item]) -> Vec<DerefPolymorphismInfo> {
    let mut result = Vec::new();
    for item in items {
        if let syn::Item::Impl(impl_block) = item
            && let Some((_, trait_path, _)) = &impl_block.trait_
            && let Some(seg) = trait_path.segments.last()
            && seg.ident == "Deref"
        {
            let impl_type = rvs_type_name(&impl_block.self_ty);
            for item in &impl_block.items {
                if let syn::ImplItem::Const(assoc_const) = item
                    && assoc_const.ident == "Target"
                {
                    let target_type = rvs_type_name_from_type_expr(&assoc_const.ty);
                    result.push(DerefPolymorphismInfo {
                        impl_type,
                        target_type,
                        line: seg.ident.span().start().line,
                    });
                    break;
                }
                if let syn::ImplItem::Type(assoc_type) = item
                    && assoc_type.ident == "Target"
                {
                    let target_type = rvs_type_name(&assoc_type.ty);
                    result.push(DerefPolymorphismInfo {
                        impl_type,
                        target_type,
                        line: seg.ident.span().start().line,
                    });
                    break;
                }
            }
        }
    }
    result
}

fn rvs_type_name_from_type_expr(ty: &syn::Type) -> String {
    rvs_type_name(ty)
}

fn rvs_collect_reflection_usage_from_fns(functions: &[FnDef]) -> Vec<ReflectionUsageInfo> {
    let mut result = Vec::new();
    for func in functions {
        for callee in &func.calls {
            for &rp in REFLECTION_PATHS {
                if callee.name == rp || callee.name.ends_with(&format!("::{rp}")) {
                    result.push(ReflectionUsageInfo {
                        function: func.name.clone(),
                        path: callee.name.clone(),
                        line: callee.line,
                    });
                    break;
                }
            }
        }
    }
    result
}

/// Extract public structs/enums missing `#[derive(Debug)]`.
pub fn rvs_extract_missing_debug(source: &str) -> Result<Vec<MissingDebugInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;
    Ok(rvs_collect_missing_debug_from_items(&file.items))
}

/// Extract `rvs_` functions with `P` marker missing `/// # Panics` doc.
pub fn rvs_extract_missing_panics_doc(
    source: &str,
) -> Result<Vec<MissingPanicsDocInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;
    Ok(rvs_collect_missing_panics_doc_from_items(&file.items))
}

/// Extract `impl Into<T>` blocks (prefer `impl From<T>`).
pub fn rvs_extract_into_impls(source: &str) -> Result<Vec<IntoImplInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;
    Ok(rvs_collect_into_impls_from_items(&file.items))
}

/// Extract consumed owned arguments not preserved in error variants.
pub fn rvs_extract_consumed_arg_on_error(
    source: &str,
) -> Result<Vec<ConsumedArgOnErrorInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;
    Ok(rvs_collect_consumed_arg_on_error_from_items(&file.items))
}

/// Extract `impl Deref` for non-smart-pointer types (anti-pattern).
pub fn rvs_extract_deref_polymorphism(
    source: &str,
) -> Result<Vec<DerefPolymorphismInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;
    Ok(rvs_collect_deref_polymorphism_from_items(&file.items))
}

/// Extract usage of `std::any::Any`/`type_name`/`type_id` reflection APIs.
pub fn rvs_extract_reflection_usage(functions: &[FnDef]) -> Vec<ReflectionUsageInfo> {
    rvs_collect_reflection_usage_from_fns(functions)
}

/// 一项 stub 宏使用信息：`todo!()` 或 `unimplemented!()`。
#[derive(Debug, Clone)]
pub struct StubMacroInfo {
    pub function: String,
    pub macro_name: String,
    pub line: usize,
}

/// 一项空函数体信息：函数体中除了 debug_assert! 外无其他逻辑。
#[derive(Debug, Clone)]
pub struct EmptyFnInfo {
    pub function: String,
    pub line: usize,
}

/// 一项 TODO/FIXME 注释信息。
#[derive(Debug, Clone)]
pub struct TodoCommentInfo {
    pub kind: String,
    pub text: String,
    pub line: usize,
}

/// Extract `todo!()` / `unimplemented!()` stub macro usage from source.
pub fn rvs_extract_stub_macros(source: &str) -> Result<Vec<StubMacroInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;
    Ok(rvs_collect_stub_macros_from_items(&file.items))
}

const STUB_MACROS: &[&str] = &["todo", "unimplemented"];

fn rvs_is_stub_macro(mac: &syn::Macro) -> bool {
    let name = mac
        .path
        .segments
        .last()
        .map(|s| s.ident.to_string())
        .unwrap_or_default();
    STUB_MACROS.contains(&name.as_str())
}

fn rvs_collect_stub_macros_from_items(items: &[syn::Item]) -> Vec<StubMacroInfo> {
    let mut result = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                let fn_name = item_fn.sig.ident.to_string();
                result.extend(rvs_scan_block_for_stubs(&item_fn.block, &fn_name));
            }
            syn::Item::Impl(impl_block) => {
                for impl_item in &impl_block.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item {
                        let fn_name = impl_fn.sig.ident.to_string();
                        result.extend(rvs_scan_block_for_stubs(&impl_fn.block, &fn_name));
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    result.extend(rvs_collect_stub_macros_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    result
}

fn rvs_scan_block_for_stubs(block: &syn::Block, fn_name: &str) -> Vec<StubMacroInfo> {
    let mut result = Vec::new();
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Macro(m) if rvs_is_stub_macro(&m.mac) => {
                let macro_name = m
                    .mac
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_default();
                let line = m
                    .mac
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.span().start().line)
                    .unwrap_or(0);
                result.push(StubMacroInfo {
                    function: fn_name.to_string(),
                    macro_name,
                    line,
                });
            }
            syn::Stmt::Expr(expr, _) => {
                result.extend(rvs_scan_expr_for_stubs(expr, fn_name));
            }
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    result.extend(rvs_scan_expr_for_stubs(&init.expr, fn_name));
                    if let Some((_, diverge)) = &init.diverge {
                        result.extend(rvs_scan_expr_for_stubs(diverge, fn_name));
                    }
                }
            }
            syn::Stmt::Item(_) | syn::Stmt::Macro(_) => {}
        }
    }
    result
}

fn rvs_scan_expr_for_stubs(expr: &syn::Expr, fn_name: &str) -> Vec<StubMacroInfo> {
    match expr {
        syn::Expr::Macro(m) if rvs_is_stub_macro(&m.mac) => {
            let macro_name = m
                .mac
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            let line = m
                .mac
                .path
                .segments
                .last()
                .map(|s| s.ident.span().start().line)
                .unwrap_or(0);
            vec![StubMacroInfo {
                function: fn_name.to_string(),
                macro_name,
                line,
            }]
        }
        syn::Expr::Block(b) => rvs_scan_block_for_stubs(&b.block, fn_name),
        syn::Expr::If(e) => {
            let mut r = Vec::new();
            r.extend(rvs_scan_expr_for_stubs(&e.cond, fn_name));
            r.extend(rvs_scan_block_for_stubs(&e.then_branch, fn_name));
            if let Some((_, els)) = &e.else_branch {
                r.extend(rvs_scan_expr_for_stubs(els, fn_name));
            }
            r
        }
        syn::Expr::Match(e) => {
            let mut r = rvs_scan_expr_for_stubs(&e.expr, fn_name);
            for arm in &e.arms {
                if let Some((_, guard)) = &arm.guard {
                    r.extend(rvs_scan_expr_for_stubs(guard, fn_name));
                }
                r.extend(rvs_scan_expr_for_stubs(&arm.body, fn_name));
            }
            r
        }
        syn::Expr::Loop(e) => rvs_scan_block_for_stubs(&e.body, fn_name),
        syn::Expr::While(e) => {
            let mut r = rvs_scan_expr_for_stubs(&e.cond, fn_name);
            r.extend(rvs_scan_block_for_stubs(&e.body, fn_name));
            r
        }
        syn::Expr::ForLoop(e) => {
            let mut r = rvs_scan_expr_for_stubs(&e.expr, fn_name);
            r.extend(rvs_scan_block_for_stubs(&e.body, fn_name));
            r
        }
        syn::Expr::Closure(c) => rvs_scan_expr_for_stubs(&c.body, fn_name),
        syn::Expr::Call(c) => {
            let mut r = rvs_scan_expr_for_stubs(&c.func, fn_name);
            for arg in &c.args {
                r.extend(rvs_scan_expr_for_stubs(arg, fn_name));
            }
            r
        }
        syn::Expr::MethodCall(c) => {
            let mut r = rvs_scan_expr_for_stubs(&c.receiver, fn_name);
            for arg in &c.args {
                r.extend(rvs_scan_expr_for_stubs(arg, fn_name));
            }
            r
        }
        syn::Expr::Assign(a) => {
            let mut r = rvs_scan_expr_for_stubs(&a.left, fn_name);
            r.extend(rvs_scan_expr_for_stubs(&a.right, fn_name));
            r
        }
        syn::Expr::Binary(b) => {
            let mut r = rvs_scan_expr_for_stubs(&b.left, fn_name);
            r.extend(rvs_scan_expr_for_stubs(&b.right, fn_name));
            r
        }
        syn::Expr::Unary(u) => rvs_scan_expr_for_stubs(&u.expr, fn_name),
        syn::Expr::Paren(p) => rvs_scan_expr_for_stubs(&p.expr, fn_name),
        syn::Expr::Tuple(t) => t
            .elems
            .iter()
            .flat_map(|e| rvs_scan_expr_for_stubs(e, fn_name))
            .collect(),
        syn::Expr::Array(a) => a
            .elems
            .iter()
            .flat_map(|e| rvs_scan_expr_for_stubs(e, fn_name))
            .collect(),
        syn::Expr::Struct(s) => s
            .fields
            .iter()
            .flat_map(|f| rvs_scan_expr_for_stubs(&f.expr, fn_name))
            .collect(),
        syn::Expr::Repeat(r) => {
            let mut res = rvs_scan_expr_for_stubs(&r.expr, fn_name);
            res.extend(rvs_scan_expr_for_stubs(&r.len, fn_name));
            res
        }
        syn::Expr::Range(r) => {
            let mut res = Vec::new();
            if let Some(s) = &r.start {
                res.extend(rvs_scan_expr_for_stubs(s, fn_name));
            }
            if let Some(e) = &r.end {
                res.extend(rvs_scan_expr_for_stubs(e, fn_name));
            }
            res
        }
        syn::Expr::Index(i) => {
            let mut r = rvs_scan_expr_for_stubs(&i.expr, fn_name);
            r.extend(rvs_scan_expr_for_stubs(&i.index, fn_name));
            r
        }
        syn::Expr::Field(f) => rvs_scan_expr_for_stubs(&f.base, fn_name),
        syn::Expr::Reference(r) => rvs_scan_expr_for_stubs(&r.expr, fn_name),
        syn::Expr::Try(t) => rvs_scan_expr_for_stubs(&t.expr, fn_name),
        syn::Expr::Await(a) => rvs_scan_expr_for_stubs(&a.base, fn_name),
        syn::Expr::Return(r) => r
            .expr
            .as_ref()
            .map(|e| rvs_scan_expr_for_stubs(e, fn_name))
            .unwrap_or_default(),
        syn::Expr::Break(b) => b
            .expr
            .as_ref()
            .map(|e| rvs_scan_expr_for_stubs(e, fn_name))
            .unwrap_or_default(),
        syn::Expr::Group(g) => rvs_scan_expr_for_stubs(&g.expr, fn_name),
        syn::Expr::Let(l) => rvs_scan_expr_for_stubs(&l.expr, fn_name),
        syn::Expr::Unsafe(u) => rvs_scan_block_for_stubs(&u.block, fn_name),
        syn::Expr::Async(a) => rvs_scan_block_for_stubs(&a.block, fn_name),
        syn::Expr::Cast(c) => rvs_scan_expr_for_stubs(&c.expr, fn_name),
        syn::Expr::TryBlock(t) => rvs_scan_block_for_stubs(&t.block, fn_name),
        syn::Expr::Macro(_)
        | syn::Expr::Lit(_)
        | syn::Expr::Continue(_)
        | syn::Expr::Verbatim(_)
        | syn::Expr::Path(_) => Vec::new(),
        _ => Vec::new(),
    }
}

/// Extract functions with empty bodies (only debug_assert! statements, no logic).
pub fn rvs_extract_empty_fns(source: &str) -> Result<Vec<EmptyFnInfo>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;
    Ok(rvs_collect_empty_fns_from_items(&file.items))
}

fn rvs_collect_empty_fns_from_items(items: &[syn::Item]) -> Vec<EmptyFnInfo> {
    let mut result = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                let name = item_fn.sig.ident.to_string();
                let line = item_fn.sig.ident.span().start().line;
                if rvs_is_empty_block(&item_fn.block) {
                    result.push(EmptyFnInfo {
                        function: name,
                        line,
                    });
                }
            }
            syn::Item::Impl(impl_block) => {
                for impl_item in &impl_block.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item {
                        let name = impl_fn.sig.ident.to_string();
                        let line = impl_fn.sig.ident.span().start().line;
                        if rvs_is_empty_block(&impl_fn.block) {
                            result.push(EmptyFnInfo {
                                function: name,
                                line,
                            });
                        }
                    }
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, inner_items)) = &m.content {
                    result.extend(rvs_collect_empty_fns_from_items(inner_items));
                }
            }
            _ => {}
        }
    }
    result
}

fn rvs_is_empty_block(block: &syn::Block) -> bool {
    if block.stmts.is_empty() {
        return true;
    }
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Macro(m) if rvs_is_debug_assert(&m.mac) => continue,
            syn::Stmt::Macro(_) => return false,
            syn::Stmt::Expr(expr, _) => {
                if rvs_is_only_debug_asserts_expr(expr) {
                    continue;
                }
                return false;
            }
            syn::Stmt::Local(_) | syn::Stmt::Item(_) => return false,
        }
    }
    true
}

fn rvs_is_only_debug_asserts_expr(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Macro(m) if rvs_is_debug_assert(&m.mac) => true,
        syn::Expr::Block(b) => rvs_is_empty_block(&b.block),
        _ => false,
    }
}

/// Extract TODO/FIXME comments from source (string scanning, not AST).
pub fn rvs_extract_todo_comments(source: &str) -> Vec<TodoCommentInfo> {
    let mut result = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let rest = if let Some(after_line_comment) = trimmed.strip_prefix("//") {
            after_line_comment
        } else if let Some(after_block_comment) = trimmed.strip_prefix("/*") {
            after_block_comment
                .trim_end_matches('*')
                .trim_end_matches('/')
        } else {
            continue;
        };
        let rest = rest.trim_start();
        if let Some(text) = rest.strip_prefix("TODO") {
            result.push(TodoCommentInfo {
                kind: "TODO".to_string(),
                text: text.trim().to_string(),
                line: i + 1,
            });
        } else if let Some(text) = rest.strip_prefix("FIXME") {
            result.push(TodoCommentInfo {
                kind: "FIXME".to_string(),
                text: text.trim().to_string(),
                line: i + 1,
            });
        }
    }
    result
}

/// 从 `#[test]` 函数体中提取所有调用目标名（去重排序）。
/// 用于交叉比对哪些 `rvs_` 好函数被测试覆盖。
pub fn rvs_extract_test_call_names(source: &str) -> Result<Vec<String>, ExtractError> {
    let file = syn::parse_file(source).map_err(|e| ExtractError::Parse {
        message: e.to_string(),
    })?;
    let statics = rvs_collect_static_decls_from_items(&file.items);
    let calls = rvs_collect_test_calls_from_items(&file.items, &statics);
    let mut names: Vec<String> = calls
        .into_iter()
        .map(|c| c.name.rsplit("::").next().unwrap_or(&c.name).to_string())
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

fn rvs_collect_test_calls_from_items(
    items: &[syn::Item],
    statics: &[StaticDecl],
) -> Vec<CalleeInfo> {
    let mut result = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) if rvs_is_test_fn(&item_fn.attrs) => {
                let (calls, _) = rvs_collect_calls_and_statics(&item_fn.block, statics);
                result.extend(calls);
            }
            syn::Item::Mod(m) => {
                let inner_statics = rvs_collect_static_decls_from_items(
                    m.content
                        .as_ref()
                        .map(|(_, i)| i)
                        .map_or(&[] as &[syn::Item], |v| v),
                );
                let mut merged_statics = statics.to_vec();
                merged_statics.extend(inner_statics);
                if let Some((_, inner_items)) = &m.content {
                    result.extend(rvs_collect_test_calls_from_items(
                        inner_items,
                        &merged_statics,
                    ));
                }
            }
            _ => {}
        }
    }
    result
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("parse error: {message}")]
    Parse { message: String },
}
