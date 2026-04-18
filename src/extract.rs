use crate::capability::{parse_rvs_function, CapabilitySet};

/// 被调用者的蛛丝马迹：名与行。
#[derive(Debug, Clone)]
pub struct CalleeInfo {
    pub name: String,
    pub line: usize,
}

/// 函数之全貌：名、能力、所调、所在行、所占行数。
#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub capabilities: CapabilitySet,
    pub calls: Vec<CalleeInfo>,
    pub line: usize,
    pub line_count: usize,
}

/// 从直接调用中捉拿函数调用。
/// 不论 rvs 与否，悉数收录，由下游甄别。
fn rvs_collect_from_expr_call(call: &syn::ExprCall) -> Vec<CalleeInfo> {
    let mut calls = Vec::new();
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
            calls.push(CalleeInfo { name, line });
        }
    }
    calls.extend(rvs_collect_from_expr(&call.func));
    for arg in &call.args {
        calls.extend(rvs_collect_from_expr(arg));
    }
    calls
}

/// 从方法调用中捉拿函数调用。
/// 方法名即调用名，不问归属。
fn rvs_collect_from_expr_method_call(call: &syn::ExprMethodCall) -> Vec<CalleeInfo> {
    let mut calls = Vec::new();
    let name = call.method.to_string();
    let line = call.method.span().start().line;
    calls.push(CalleeInfo { name, line });
    calls.extend(rvs_collect_from_expr(&call.receiver));
    for arg in &call.args {
        calls.extend(rvs_collect_from_expr(arg));
    }
    calls
}

/// 巡遍表达式，不论深浅，逢调用必捉。
fn rvs_collect_from_expr(expr: &syn::Expr) -> Vec<CalleeInfo> {
    match expr {
        syn::Expr::Call(call) => rvs_collect_from_expr_call(call),
        syn::Expr::MethodCall(call) => rvs_collect_from_expr_method_call(call),
        syn::Expr::Block(block) => rvs_collect_from_block(&block.block),
        syn::Expr::If(if_expr) => [
            rvs_collect_from_expr(&if_expr.cond),
            rvs_collect_from_block(&if_expr.then_branch),
            if_expr
                .else_branch
                .as_ref()
                .map(|(_, e)| rvs_collect_from_expr(e))
                .unwrap_or_default(),
        ]
        .concat(),
        syn::Expr::Match(match_expr) => {
            let mut calls = Vec::new();
            calls.extend(rvs_collect_from_expr(&match_expr.expr));
            for arm in &match_expr.arms {
                calls.extend(rvs_collect_from_expr(&arm.body));
            }
            calls
        }
        syn::Expr::Loop(loop_expr) => rvs_collect_from_block(&loop_expr.body),
        syn::Expr::While(while_expr) => [
            rvs_collect_from_expr(&while_expr.cond),
            rvs_collect_from_block(&while_expr.body),
        ]
        .concat(),
        syn::Expr::ForLoop(for_expr) => [
            rvs_collect_from_expr(&for_expr.expr),
            rvs_collect_from_block(&for_expr.body),
        ]
        .concat(),
        syn::Expr::Closure(closure) => rvs_collect_from_expr(&closure.body),
        syn::Expr::Assign(assign) => [
            rvs_collect_from_expr(&assign.left),
            rvs_collect_from_expr(&assign.right),
        ]
        .concat(),
        syn::Expr::Binary(binary) => [
            rvs_collect_from_expr(&binary.left),
            rvs_collect_from_expr(&binary.right),
        ]
        .concat(),
        syn::Expr::Unary(unary) => rvs_collect_from_expr(&unary.expr),
        syn::Expr::Paren(paren) => rvs_collect_from_expr(&paren.expr),
        syn::Expr::Tuple(tuple) => tuple
            .elems
            .iter()
            .flat_map(|e| rvs_collect_from_expr(e))
            .collect(),
        syn::Expr::Array(array) => array
            .elems
            .iter()
            .flat_map(|e| rvs_collect_from_expr(e))
            .collect(),
        syn::Expr::Struct(struct_expr) => struct_expr
            .fields
            .iter()
            .flat_map(|f| rvs_collect_from_expr(&f.expr))
            .collect(),
        syn::Expr::Repeat(repeat) => [
            rvs_collect_from_expr(&repeat.expr),
            rvs_collect_from_expr(&repeat.len),
        ]
        .concat(),
        syn::Expr::Range(range) => {
            let mut calls = Vec::new();
            if let Some(start) = &range.start {
                calls.extend(rvs_collect_from_expr(start));
            }
            if let Some(end) = &range.end {
                calls.extend(rvs_collect_from_expr(end));
            }
            calls
        }
        syn::Expr::Index(index) => [
            rvs_collect_from_expr(&index.expr),
            rvs_collect_from_expr(&index.index),
        ]
        .concat(),
        syn::Expr::Field(field) => rvs_collect_from_expr(&field.base),
        syn::Expr::Reference(reference) => rvs_collect_from_expr(&reference.expr),
        syn::Expr::Try(try_expr) => rvs_collect_from_expr(&try_expr.expr),
        syn::Expr::Await(await_expr) => rvs_collect_from_expr(&await_expr.base),
        syn::Expr::Return(ret) => ret
            .expr
            .as_ref()
            .map(|e| rvs_collect_from_expr(e))
            .unwrap_or_default(),
        syn::Expr::Break(brk) => brk
            .expr
            .as_ref()
            .map(|e| rvs_collect_from_expr(e))
            .unwrap_or_default(),
        syn::Expr::Group(group) => rvs_collect_from_expr(&group.expr),
        syn::Expr::Let(let_expr) => rvs_collect_from_expr(&let_expr.expr),
        syn::Expr::Unsafe(unsafe_expr) => rvs_collect_from_block(&unsafe_expr.block),
        syn::Expr::Macro(macro_expr) => {
            let _ = macro_expr;
            Vec::new()
        }
        syn::Expr::Path(_)
        | syn::Expr::Lit(_)
        | syn::Expr::Continue(_)
        | syn::Expr::Verbatim(_) => Vec::new(),
        _ => Vec::new(),
    }
}

/// 巡遍一个块中的每一条语句。
fn rvs_collect_from_block(block: &syn::Block) -> Vec<CalleeInfo> {
    let mut calls = Vec::new();
    for stmt in &block.stmts {
        match stmt {
            syn::Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    calls.extend(rvs_collect_from_expr(&init.expr));
                }
            }
            syn::Stmt::Expr(expr, _) => {
                calls.extend(rvs_collect_from_expr(expr));
            }
            syn::Stmt::Item(_) => {}
            _ => {}
        }
    }
    calls
}

/// 巡遍一个块，收集其中所有调用。
/// 入口：给一个块，还你一张清单。
pub fn rvs_collect_calls(block: &syn::Block) -> Vec<CalleeInfo> {
    rvs_collect_from_block(block)
}

/// 取首尾行号之差加一，即为函数所占行数。
fn rvs_calc_line_count(start_span: proc_macro2::Span, end_span: proc_macro2::Span) -> usize {
    let start_line = start_span.start().line;
    let end_line = end_span.end().line;
    debug_assert!(end_line >= start_line, "函数尾行不应在首行之前");
    end_line - start_line + 1
}

/// 从顶层函数定义中萃取信息。
/// 非 rvs_ 函数，视而不见。
fn rvs_extract_from_item_fn(item_fn: &syn::ItemFn) -> Option<FnDef> {
    let name = item_fn.sig.ident.to_string();
    let (_, caps) = parse_rvs_function(&name)?;
    let line = item_fn.sig.ident.span().start().line;
    let line_count = rvs_calc_line_count(
        item_fn.sig.fn_token.span,
        item_fn.block.brace_token.span.join(),
    );
    let calls = rvs_collect_calls(&item_fn.block);

    Some(FnDef {
        name,
        capabilities: caps,
        calls,
        line,
        line_count,
    })
}

/// 从 impl 块中的方法萃取信息。
fn rvs_extract_from_impl_fn(impl_fn: &syn::ImplItemFn) -> Option<FnDef> {
    let name = impl_fn.sig.ident.to_string();
    let (_, caps) = parse_rvs_function(&name)?;
    let line = impl_fn.sig.ident.span().start().line;
    let line_count = rvs_calc_line_count(
        impl_fn.sig.fn_token.span,
        impl_fn.block.brace_token.span.join(),
    );
    let calls = rvs_collect_calls(&impl_fn.block);

    Some(FnDef {
        name,
        capabilities: caps,
        calls,
        line,
        line_count,
    })
}

/// 从 trait 定义中的方法签名萃取信息。
/// 无默认实现的方法，所调为空。
fn rvs_extract_from_trait_fn(trait_fn: &syn::TraitItemFn) -> Option<FnDef> {
    let name = trait_fn.sig.ident.to_string();
    let (_, caps) = parse_rvs_function(&name)?;
    let line = trait_fn.sig.ident.span().start().line;
    let calls = trait_fn
        .default
        .as_ref()
        .map(|block| rvs_collect_calls(block))
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

    Some(FnDef {
        name,
        capabilities: caps,
        calls,
        line,
        line_count,
    })
}

/// 从一段源码中萃取所有 rvs_ 函数定义。
/// 顶层函数、impl 方法、trait 方法，一网打尽。
///
/// 乱麻理成方阵，字节各归其位。
/// 过得去的，放行；过不去的，退回。
#[allow(non_snake_case)]
pub fn rvs_extract_functions_E(source: &str) -> Result<Vec<FnDef>, ExtractError> {
    let file = syn::parse_file(source)
        .map_err(|e| ExtractError::Parse { message: e.to_string() })?;

    let mut functions = Vec::new();

    for item in &file.items {
        match item {
            syn::Item::Fn(item_fn) => {
                if let Some(fn_def) = rvs_extract_from_item_fn(item_fn) {
                    functions.push(fn_def);
                }
            }
            syn::Item::Impl(item_impl) => {
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(impl_fn) = impl_item {
                        if let Some(fn_def) = rvs_extract_from_impl_fn(impl_fn) {
                            functions.push(fn_def);
                        }
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for trait_item in &item_trait.items {
                    if let syn::TraitItem::Fn(trait_fn) = trait_item {
                        if let Some(fn_def) = rvs_extract_from_trait_fn(trait_fn) {
                            functions.push(fn_def);
                        }
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
