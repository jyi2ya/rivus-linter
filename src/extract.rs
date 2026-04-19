use std::collections::BTreeSet;

use crate::capability::{parse_rvs_function, Capability, CapabilitySet};

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

/// 函数之全貌：名、能力、所调、静态引用、所在行、所占行数、参数、已断言之参。
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
    fn empty() -> Self {
        Harvest {
            calls: Vec::new(),
            static_refs: Vec::new(),
        }
    }

    fn merge(parts: impl IntoIterator<Item = Harvest>) -> Self {
        let mut result = Harvest::empty();
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

/// 从直接调用中捉拿函数调用与静态引用。
fn rvs_harvest_from_expr_call(call: &syn::ExprCall, statics: &[StaticDecl]) -> Harvest {
    let mut result = Harvest::empty();
    if let syn::Expr::Path(expr_path) = &*call.func {
        let name: String = expr_path
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        if !name.is_empty() {
            let line = expr_path
                .path
                .segments
                .last()
                .unwrap()
                .ident
                .span()
                .start()
                .line;
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
    let mut result = Harvest::empty();
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
            let mut result = Harvest::empty();
            if let Some(sr) = rvs_check_path_for_static(&path.path, statics) {
                result.static_refs.push(sr);
            }
            result
        }
        syn::Expr::Block(block) => rvs_harvest_from_block(&block.block, statics),
        syn::Expr::If(if_expr) => Harvest::merge([
            rvs_harvest_from_expr(&if_expr.cond, statics),
            rvs_harvest_from_block(&if_expr.then_branch, statics),
            if_expr
                .else_branch
                .as_ref()
                .map(|(_, e)| rvs_harvest_from_expr(e, statics))
                .unwrap_or_else(Harvest::empty),
        ]),
        syn::Expr::Match(match_expr) => {
            let mut result = rvs_harvest_from_expr(&match_expr.expr, statics);
            for arm in &match_expr.arms {
                let sub = rvs_harvest_from_expr(&arm.body, statics);
                result.calls.extend(sub.calls);
                result.static_refs.extend(sub.static_refs);
            }
            result
        }
        syn::Expr::Loop(loop_expr) => rvs_harvest_from_block(&loop_expr.body, statics),
        syn::Expr::While(while_expr) => Harvest::merge([
            rvs_harvest_from_expr(&while_expr.cond, statics),
            rvs_harvest_from_block(&while_expr.body, statics),
        ]),
        syn::Expr::ForLoop(for_expr) => Harvest::merge([
            rvs_harvest_from_expr(&for_expr.expr, statics),
            rvs_harvest_from_block(&for_expr.body, statics),
        ]),
        syn::Expr::Closure(closure) => rvs_harvest_from_expr(&closure.body, statics),
        syn::Expr::Assign(assign) => Harvest::merge([
            rvs_harvest_from_expr(&assign.left, statics),
            rvs_harvest_from_expr(&assign.right, statics),
        ]),
        syn::Expr::Binary(binary) => Harvest::merge([
            rvs_harvest_from_expr(&binary.left, statics),
            rvs_harvest_from_expr(&binary.right, statics),
        ]),
        syn::Expr::Unary(unary) => rvs_harvest_from_expr(&unary.expr, statics),
        syn::Expr::Paren(paren) => rvs_harvest_from_expr(&paren.expr, statics),
        syn::Expr::Tuple(tuple) => Harvest::merge(
            tuple
                .elems
                .iter()
                .map(|e| rvs_harvest_from_expr(e, statics)),
        ),
        syn::Expr::Array(array) => Harvest::merge(
            array
                .elems
                .iter()
                .map(|e| rvs_harvest_from_expr(e, statics)),
        ),
        syn::Expr::Struct(struct_expr) => Harvest::merge(
            struct_expr
                .fields
                .iter()
                .map(|f| rvs_harvest_from_expr(&f.expr, statics)),
        ),
        syn::Expr::Repeat(repeat) => Harvest::merge([
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
            Harvest::merge(parts)
        }
        syn::Expr::Index(index) => Harvest::merge([
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
            .unwrap_or_else(Harvest::empty),
        syn::Expr::Break(brk) => brk
            .expr
            .as_ref()
            .map(|e| rvs_harvest_from_expr(e, statics))
            .unwrap_or_else(Harvest::empty),
        syn::Expr::Group(group) => rvs_harvest_from_expr(&group.expr, statics),
        syn::Expr::Let(let_expr) => rvs_harvest_from_expr(&let_expr.expr, statics),
        syn::Expr::Unsafe(unsafe_expr) => {
            rvs_harvest_from_block(&unsafe_expr.block, statics)
        }
        syn::Expr::Macro(_) => Harvest::empty(),
        syn::Expr::Lit(_)
        | syn::Expr::Continue(_)
        | syn::Expr::Verbatim(_) => Harvest::empty(),
        _ => Harvest::empty(),
    }
}

/// 巡遍一个块中的每一条语句。
fn rvs_harvest_from_block(block: &syn::Block, statics: &[StaticDecl]) -> Harvest {
    let mut result = Harvest::empty();
    for stmt in &block.stmts {
        let sub = match stmt {
            syn::Stmt::Local(local) => local
                .init
                .as_ref()
                .map(|init| rvs_harvest_from_expr(&init.expr, statics))
                .unwrap_or_else(Harvest::empty),
            syn::Stmt::Expr(expr, _) => rvs_harvest_from_expr(expr, statics),
            syn::Stmt::Item(_) | syn::Stmt::Macro(_) => Harvest::empty(),
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
            proc_macro2::TokenTree::Ident(ident)
                if ident == "static" =>
            {
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
                let mut caps = CapabilitySet::rvs_new();
                if let syn::StaticMutability::Mut(_) = s.mutability {
                    caps.rvs_insert(Capability::U);
                }
                caps.rvs_insert(Capability::P);
                decls.push(StaticDecl {
                    name: s.ident.to_string(),
                    required_caps: caps,
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
                        let mut caps = CapabilitySet::rvs_new();
                        caps.rvs_insert(Capability::T);
                        caps.rvs_insert(Capability::P);
                        decls.push(StaticDecl {
                            name,
                            required_caps: caps,
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
    "i8", "i16", "i32", "i64", "i128",
    "u8", "u16", "u32", "u64", "u128",
    "f32", "f64",
    "isize", "usize",
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
    let mut ids = BTreeSet::new();
    rvs_collect_assert_ids_from_block(block, &mut ids);
    ids
}

fn rvs_collect_assert_ids_from_block(block: &syn::Block, ids: &mut BTreeSet<String>) {
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Macro(m) => {
                if rvs_is_debug_assert(&m.mac) {
                    ids.extend(rvs_collect_ident_tokens(&m.mac.tokens));
                }
            }
            syn::Stmt::Expr(expr, _) => rvs_collect_assert_ids_from_expr(expr, ids),
            syn::Stmt::Local(l) => {
                if let Some(init) = &l.init {
                    rvs_collect_assert_ids_from_expr(&init.expr, ids);
                }
            }
            syn::Stmt::Item(_) => {}
        }
    }
}

fn rvs_collect_assert_ids_from_expr(expr: &syn::Expr, ids: &mut BTreeSet<String>) {
    match expr {
        syn::Expr::Macro(m) if rvs_is_debug_assert(&m.mac) => {
            ids.extend(rvs_collect_ident_tokens(&m.mac.tokens));
        }
        syn::Expr::Block(b) => rvs_collect_assert_ids_from_block(&b.block, ids),
        syn::Expr::If(e) => {
            rvs_collect_assert_ids_from_expr(&e.cond, ids);
            rvs_collect_assert_ids_from_block(&e.then_branch, ids);
            if let Some((_, els)) = &e.else_branch {
                rvs_collect_assert_ids_from_expr(els, ids);
            }
        }
        syn::Expr::Match(e) => {
            for arm in &e.arms {
                rvs_collect_assert_ids_from_expr(&arm.body, ids);
            }
        }
        syn::Expr::Loop(e) => rvs_collect_assert_ids_from_block(&e.body, ids),
        syn::Expr::While(e) => {
            rvs_collect_assert_ids_from_expr(&e.cond, ids);
            rvs_collect_assert_ids_from_block(&e.body, ids);
        }
        syn::Expr::ForLoop(e) => {
            rvs_collect_assert_ids_from_expr(&e.expr, ids);
            rvs_collect_assert_ids_from_block(&e.body, ids);
        }
        syn::Expr::Unsafe(e) => rvs_collect_assert_ids_from_block(&e.block, ids),
        syn::Expr::Closure(c) => rvs_collect_assert_ids_from_expr(&c.body, ids),
        syn::Expr::Call(e) => {
            rvs_collect_assert_ids_from_expr(&e.func, ids);
            for a in &e.args {
                rvs_collect_assert_ids_from_expr(a, ids);
            }
        }
        syn::Expr::MethodCall(e) => {
            rvs_collect_assert_ids_from_expr(&e.receiver, ids);
            for a in &e.args {
                rvs_collect_assert_ids_from_expr(a, ids);
            }
        }
        syn::Expr::Assign(e) => {
            rvs_collect_assert_ids_from_expr(&e.left, ids);
            rvs_collect_assert_ids_from_expr(&e.right, ids);
        }
        syn::Expr::Binary(e) => {
            rvs_collect_assert_ids_from_expr(&e.left, ids);
            rvs_collect_assert_ids_from_expr(&e.right, ids);
        }
        syn::Expr::Unary(e) => rvs_collect_assert_ids_from_expr(&e.expr, ids),
        syn::Expr::Paren(e) => rvs_collect_assert_ids_from_expr(&e.expr, ids),
        syn::Expr::Group(e) => rvs_collect_assert_ids_from_expr(&e.expr, ids),
        syn::Expr::Reference(e) => rvs_collect_assert_ids_from_expr(&e.expr, ids),
        syn::Expr::Try(e) => rvs_collect_assert_ids_from_expr(&e.expr, ids),
        syn::Expr::Await(e) => rvs_collect_assert_ids_from_expr(&e.base, ids),
        syn::Expr::Return(e) => {
            if let Some(inner) = &e.expr {
                rvs_collect_assert_ids_from_expr(inner, ids);
            }
        }
        syn::Expr::Break(e) => {
            if let Some(inner) = &e.expr {
                rvs_collect_assert_ids_from_expr(inner, ids);
            }
        }
        syn::Expr::Let(e) => rvs_collect_assert_ids_from_expr(&e.expr, ids),
        syn::Expr::Index(e) => {
            rvs_collect_assert_ids_from_expr(&e.expr, ids);
            rvs_collect_assert_ids_from_expr(&e.index, ids);
        }
        syn::Expr::Field(e) => rvs_collect_assert_ids_from_expr(&e.base, ids),
        syn::Expr::Range(e) => {
            if let Some(s) = &e.start {
                rvs_collect_assert_ids_from_expr(s, ids);
            }
            if let Some(end) = &e.end {
                rvs_collect_assert_ids_from_expr(end, ids);
            }
        }
        syn::Expr::Repeat(e) => {
            rvs_collect_assert_ids_from_expr(&e.expr, ids);
            rvs_collect_assert_ids_from_expr(&e.len, ids);
        }
        syn::Expr::Tuple(e) => {
            for el in &e.elems {
                rvs_collect_assert_ids_from_expr(el, ids);
            }
        }
        syn::Expr::Array(e) => {
            for el in &e.elems {
                rvs_collect_assert_ids_from_expr(el, ids);
            }
        }
        syn::Expr::Struct(e) => {
            for f in &e.fields {
                rvs_collect_assert_ids_from_expr(&f.expr, ids);
            }
        }
        _ => {}
    }
}

/// 从顶层函数定义中萃取信息。
fn rvs_extract_from_item_fn(
    item_fn: &syn::ItemFn,
    statics: &[StaticDecl],
) -> Option<FnDef> {
    let name = item_fn.sig.ident.to_string();
    let (_, caps) = parse_rvs_function(&name)?;
    let line = item_fn.sig.ident.span().start().line;
    let line_count = rvs_calc_line_count(
        item_fn.sig.fn_token.span,
        item_fn.block.brace_token.span.join(),
    );
    let (calls, static_refs) = rvs_collect_calls_and_statics(&item_fn.block, statics);
    let params = rvs_extract_param_names(&item_fn.sig.inputs);
    let debug_asserted_params = rvs_collect_debug_asserted_params(&item_fn.block);

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
    })
}

/// 从 impl 块中的方法萃取信息。
fn rvs_extract_from_impl_fn(
    impl_fn: &syn::ImplItemFn,
    statics: &[StaticDecl],
) -> Option<FnDef> {
    let name = impl_fn.sig.ident.to_string();
    let (_, caps) = parse_rvs_function(&name)?;
    let line = impl_fn.sig.ident.span().start().line;
    let line_count = rvs_calc_line_count(
        impl_fn.sig.fn_token.span,
        impl_fn.block.brace_token.span.join(),
    );
    let (calls, static_refs) = rvs_collect_calls_and_statics(&impl_fn.block, statics);
    let params = rvs_extract_param_names(&impl_fn.sig.inputs);
    let debug_asserted_params = rvs_collect_debug_asserted_params(&impl_fn.block);

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
    })
}

/// 从 trait 定义中的方法签名萃取信息。
fn rvs_extract_from_trait_fn(
    trait_fn: &syn::TraitItemFn,
    statics: &[StaticDecl],
) -> Option<FnDef> {
    let name = trait_fn.sig.ident.to_string();
    let (_, caps) = parse_rvs_function(&name)?;
    let line = trait_fn.sig.ident.span().start().line;
    let (calls, static_refs) = trait_fn
        .default
        .as_ref()
        .map(|block| rvs_collect_calls_and_statics(block, statics))
        .unwrap_or_default();
    let line_count = trait_fn
        .default
        .as_ref()
        .map(|block| {
            rvs_calc_line_count(
                trait_fn.sig.fn_token.span,
                block.brace_token.span.join(),
            )
        })
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
    })
}

/// 从一段源码中萃取所有 rvs_ 函数定义。
/// 顶层函数、impl 方法、trait 方法，一网打尽。
/// 同时搜集文件中的 static 与 thread_local! 声明，
/// 据此检查函数体内的静态变量引用。
#[allow(non_snake_case)]
pub fn rvs_extract_functions_E(source: &str) -> Result<Vec<FnDef>, ExtractError> {
    let file = syn::parse_file(source)
        .map_err(|e| ExtractError::Parse { message: e.to_string() })?;

    let statics = rvs_collect_static_decls_from_items(&file.items);

    let mut functions = Vec::new();

    for item in &file.items {
        match item {
            syn::Item::Fn(item_fn) => {
                if let Some(fn_def) = rvs_extract_from_item_fn(item_fn, &statics) {
                    functions.push(fn_def);
                }
            }
            syn::Item::Impl(item_impl) => {
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item
                        && let Some(fn_def) = rvs_extract_from_impl_fn(impl_fn, &statics)
                    {
                        functions.push(fn_def);
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for trait_item in &item_trait.items {
                    if let syn::TraitItem::Fn(trait_fn) = trait_item
                        && let Some(fn_def) = rvs_extract_from_trait_fn(trait_fn, &statics)
                    {
                        functions.push(fn_def);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(functions)
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("parse error: {message}")]
    Parse { message: String },
}
