#![forbid(unsafe_code)]
//! Minimal semantic analysis scaffold.

use std::collections::HashSet;

use mel_ast::{Expr, Item, ShellWord, SourceFile, Stmt};
use mel_syntax::TextRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
}

#[must_use]
pub fn analyze(source: &SourceFile) -> Analysis {
    let mut global_procs = HashSet::new();
    let mut all_local_procs = HashSet::new();
    for item in &source.items {
        if let Item::Proc(proc_def) = item {
            collect_proc_def_names(proc_def, &mut global_procs, &mut all_local_procs);
        }

        if let Item::Stmt(stmt) = item {
            collect_stmt_proc_names(stmt, &mut global_procs, &mut all_local_procs);
        }
    }

    let mut state = AnalysisState {
        diagnostics: Vec::new(),
        global_procs,
        all_local_procs,
        visible_local_procs: HashSet::new(),
    };

    for item in &source.items {
        walk_item(item, &mut state);
        if let Item::Proc(proc_def) = item {
            if !proc_def.is_global {
                state.visible_local_procs.insert(proc_def.name.clone());
            }
        }
    }

    Analysis {
        diagnostics: state.diagnostics,
    }
}

struct AnalysisState {
    diagnostics: Vec<Diagnostic>,
    global_procs: HashSet<String>,
    all_local_procs: HashSet<String>,
    visible_local_procs: HashSet<String>,
}

fn walk_item(item: &Item, state: &mut AnalysisState) {
    match item {
        Item::Proc(proc_def) => {
            let mut scope = ProcScope {
                global_procs: &state.global_procs,
                all_local_procs: &state.all_local_procs,
                visible_local_procs: state.visible_local_procs.clone(),
                diagnostics: &mut state.diagnostics,
            };
            if !proc_def.is_global {
                scope.visible_local_procs.insert(proc_def.name.clone());
            }
            walk_stmt(&proc_def.body, &mut scope);
        }
        Item::Stmt(stmt) => {
            let mut scope = ProcScope {
                global_procs: &state.global_procs,
                all_local_procs: &state.all_local_procs,
                visible_local_procs: state.visible_local_procs.clone(),
                diagnostics: &mut state.diagnostics,
            };
            walk_stmt(stmt, &mut scope);
        }
    }
}

struct ProcScope<'a> {
    global_procs: &'a HashSet<String>,
    all_local_procs: &'a HashSet<String>,
    visible_local_procs: HashSet<String>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

fn walk_stmt(stmt: &Stmt, scope: &mut ProcScope<'_>) {
    match stmt {
        Stmt::Empty { .. } | Stmt::Break { .. } | Stmt::Continue { .. } => {}
        Stmt::Block { statements, .. } => {
            for stmt in statements {
                walk_stmt(stmt, scope);
                if let Stmt::Proc { proc_def, .. } = stmt {
                    if !proc_def.is_global {
                        scope.visible_local_procs.insert(proc_def.name.clone());
                    }
                }
            }
        }
        Stmt::Proc { proc_def, .. } => {
            let mut nested_scope = ProcScope {
                global_procs: scope.global_procs,
                all_local_procs: scope.all_local_procs,
                visible_local_procs: scope.visible_local_procs.clone(),
                diagnostics: scope.diagnostics,
            };
            if !proc_def.is_global {
                nested_scope
                    .visible_local_procs
                    .insert(proc_def.name.clone());
            }
            walk_stmt(&proc_def.body, &mut nested_scope);
        }
        Stmt::Expr { expr, .. } => walk_expr(expr, scope),
        Stmt::VarDecl { decl, .. } => {
            for declarator in &decl.declarators {
                if let Some(Some(size)) = &declarator.array_size {
                    walk_expr(size, scope);
                }

                if let Some(initializer) = &declarator.initializer {
                    walk_expr(initializer, scope);
                }
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            walk_expr(condition, scope);
            walk_stmt(then_branch, scope);
            if let Some(else_branch) = else_branch {
                walk_stmt(else_branch, scope);
            }
        }
        Stmt::While {
            condition, body, ..
        } => {
            walk_expr(condition, scope);
            walk_stmt(body, scope);
        }
        Stmt::DoWhile {
            body, condition, ..
        } => {
            walk_stmt(body, scope);
            walk_expr(condition, scope);
        }
        Stmt::Switch {
            control, clauses, ..
        } => {
            walk_expr(control, scope);
            for clause in clauses {
                if let mel_ast::SwitchLabel::Case(expr) = &clause.label {
                    walk_expr(expr, scope);
                }
                for stmt in &clause.statements {
                    walk_stmt(stmt, scope);
                }
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            ..
        } => {
            if let Some(init) = init {
                for expr in init {
                    walk_expr(expr, scope);
                }
            }
            if let Some(condition) = condition {
                walk_expr(condition, scope);
            }
            if let Some(update) = update {
                for expr in update {
                    walk_expr(expr, scope);
                }
            }
            walk_stmt(body, scope);
        }
        Stmt::ForIn {
            binding,
            iterable,
            body,
            ..
        } => {
            walk_expr(binding, scope);
            walk_expr(iterable, scope);
            walk_stmt(body, scope);
        }
        Stmt::Return { expr, .. } => {
            if let Some(expr) = expr {
                walk_expr(expr, scope);
            }
        }
    }
}

fn collect_proc_def_names(
    proc_def: &mel_ast::ProcDef,
    global_procs: &mut HashSet<String>,
    all_local_procs: &mut HashSet<String>,
) {
    if proc_def.is_global {
        global_procs.insert(proc_def.name.clone());
    } else {
        all_local_procs.insert(proc_def.name.clone());
    }

    collect_stmt_proc_names(&proc_def.body, global_procs, all_local_procs);
}

fn collect_stmt_proc_names(
    stmt: &Stmt,
    global_procs: &mut HashSet<String>,
    all_local_procs: &mut HashSet<String>,
) {
    match stmt {
        Stmt::Proc { proc_def, .. } => {
            collect_proc_def_names(proc_def, global_procs, all_local_procs);
        }
        Stmt::Block { statements, .. } => {
            for stmt in statements {
                collect_stmt_proc_names(stmt, global_procs, all_local_procs);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_stmt_proc_names(then_branch, global_procs, all_local_procs);
            if let Some(else_branch) = else_branch {
                collect_stmt_proc_names(else_branch, global_procs, all_local_procs);
            }
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForIn { body, .. } => {
            collect_stmt_proc_names(body, global_procs, all_local_procs);
        }
        Stmt::Switch { clauses, .. } => {
            for clause in clauses {
                for stmt in &clause.statements {
                    collect_stmt_proc_names(stmt, global_procs, all_local_procs);
                }
            }
        }
        Stmt::Empty { .. }
        | Stmt::Expr { .. }
        | Stmt::VarDecl { .. }
        | Stmt::Return { .. }
        | Stmt::Break { .. }
        | Stmt::Continue { .. } => {}
    }
}

fn walk_expr(expr: &Expr, scope: &mut ProcScope<'_>) {
    match expr {
        Expr::Cast { expr, .. } => {
            walk_expr(expr, scope);
        }
        Expr::Unary { expr, .. }
        | Expr::PrefixUpdate { expr, .. }
        | Expr::PostfixUpdate { expr, .. } => {
            walk_expr(expr, scope);
        }
        Expr::Binary { lhs, rhs, .. } | Expr::Assign { lhs, rhs, .. } => {
            walk_expr(lhs, scope);
            walk_expr(rhs, scope);
        }
        Expr::Ternary {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            walk_expr(condition, scope);
            walk_expr(then_expr, scope);
            walk_expr(else_expr, scope);
        }
        Expr::Index { target, index, .. } => {
            walk_expr(target, scope);
            walk_expr(index, scope);
        }
        Expr::MemberAccess { target, .. } => walk_expr(target, scope),
        Expr::ComponentAccess { target, .. } => walk_expr(target, scope),
        Expr::VectorLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
            for element in elements {
                walk_expr(element, scope);
            }
        }
        Expr::Invoke(invoke) => match &invoke.surface {
            mel_ast::InvokeSurface::Function { name, args } => {
                for arg in args {
                    walk_expr(arg, scope);
                }
                maybe_report_forward_local_proc(name, invoke.range, scope);
            }
            mel_ast::InvokeSurface::ShellLike { head, words, .. } => {
                for word in words {
                    walk_shell_word(word, scope);
                }
                maybe_report_forward_local_proc(head, invoke.range, scope);
            }
        },
        Expr::Ident { .. }
        | Expr::BareWord { .. }
        | Expr::Int { .. }
        | Expr::Float { .. }
        | Expr::String { .. } => {}
    }
}

fn walk_shell_word(word: &ShellWord, scope: &mut ProcScope<'_>) {
    match word {
        ShellWord::Flag { .. }
        | ShellWord::NumericLiteral { .. }
        | ShellWord::BareWord { .. }
        | ShellWord::QuotedString { .. } => {}
        ShellWord::Variable { expr, .. }
        | ShellWord::GroupedExpr { expr, .. }
        | ShellWord::BraceList { expr, .. }
        | ShellWord::VectorLiteral { expr, .. } => {
            walk_expr(expr, scope);
        }
        ShellWord::Capture { invoke, .. } => {
            walk_expr(&Expr::Invoke(invoke.clone()), scope);
        }
    }
}

fn maybe_report_forward_local_proc(name: &str, range: TextRange, scope: &mut ProcScope<'_>) {
    let _ = (
        name,
        range,
        scope.global_procs,
        scope.all_local_procs,
        &scope.visible_local_procs,
    );
    // Forward local-proc validation is deferred until proc resolution becomes
    // precise enough to distinguish true MEL proc calls from other unresolved
    // invocation surfaces.
}

#[cfg(test)]
mod tests {
    use super::analyze;
    use mel_ast::{
        AssignOp, CalleeResolution, Expr, InvokeExpr, InvokeSurface, Item, ProcDef, ShellWord,
        SourceFile, Stmt,
    };
    use mel_syntax::text_range;

    #[test]
    fn local_proc_forward_reference_is_deferred() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Invoke(InvokeExpr {
                        surface: InvokeSurface::ShellLike {
                            head: "helper".to_owned(),
                            words: Vec::new(),
                            captured: false,
                        },
                        resolution: CalleeResolution::Unresolved,
                        range: text_range(0, 8),
                    }),
                    range: text_range(0, 9),
                })),
                Item::Proc(Box::new(ProcDef {
                    return_type: None,
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: Vec::new(),
                        range: text_range(10, 12),
                    },
                    is_global: false,
                    range: text_range(10, 12),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn proc_body_traversal_respects_visible_local_proc() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Expr {
                        expr: Expr::Invoke(InvokeExpr {
                            surface: InvokeSurface::Function {
                                name: "helper".to_owned(),
                                args: vec![Expr::Int {
                                    value: 1,
                                    range: text_range(20, 21),
                                }],
                            },
                            resolution: CalleeResolution::Unresolved,
                            range: text_range(13, 22),
                        }),
                        range: text_range(13, 23),
                    }],
                    range: text_range(12, 24),
                },
                is_global: false,
                range: text_range(0, 24),
            }))],
        };

        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn unknown_commands_do_not_create_proc_diagnostics() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Assign {
                    op: AssignOp::Assign,
                    lhs: Box::new(Expr::Ident {
                        name: "$value".to_owned(),
                        range: text_range(0, 6),
                    }),
                    rhs: Box::new(Expr::Invoke(InvokeExpr {
                        surface: InvokeSurface::ShellLike {
                            head: "ls".to_owned(),
                            words: vec![ShellWord::Variable {
                                expr: Expr::Ident {
                                    name: "$selection".to_owned(),
                                    range: text_range(11, 21),
                                },
                                range: text_range(11, 21),
                            }],
                            captured: true,
                        },
                        resolution: CalleeResolution::Unresolved,
                        range: text_range(7, 22),
                    })),
                    range: text_range(0, 22),
                },
                range: text_range(0, 23),
            }))],
        };

        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty());
    }
}
