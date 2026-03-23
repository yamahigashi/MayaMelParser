use mel_ast::{
    Expr, InvokeExpr, InvokeSurface, Item, ShellWord, SourceFile, Stmt, SwitchLabel, VarDecl,
};

use crate::{Parse, decode::OffsetMap};

pub(crate) fn remap_parse_ranges(parse: &mut Parse, map: &OffsetMap) {
    remap_source_file_ranges(&mut parse.syntax, map);

    for diagnostic in &mut parse.lex_errors {
        diagnostic.range = map.map_range(diagnostic.range);
    }

    for error in &mut parse.errors {
        error.range = map.map_range(error.range);
    }
}

fn remap_source_file_ranges(source: &mut SourceFile, map: &OffsetMap) {
    for item in &mut source.items {
        match item {
            Item::Proc(proc_def) => remap_proc_def_ranges(proc_def, map),
            Item::Stmt(stmt) => remap_stmt_ranges(stmt, map),
        }
    }
}

fn remap_proc_def_ranges(proc_def: &mut mel_ast::ProcDef, map: &OffsetMap) {
    if let Some(return_type) = &mut proc_def.return_type {
        return_type.range = map.map_range(return_type.range);
    }

    for param in &mut proc_def.params {
        param.name_range = map.map_range(param.name_range);
        param.range = map.map_range(param.range);
    }

    remap_stmt_ranges(&mut proc_def.body, map);
    proc_def.name_range = map.map_range(proc_def.name_range);
    proc_def.range = map.map_range(proc_def.range);
}

fn remap_stmt_ranges(stmt: &mut Stmt, map: &OffsetMap) {
    match stmt {
        Stmt::Empty { range } | Stmt::Break { range } | Stmt::Continue { range } => {
            *range = map.map_range(*range);
        }
        Stmt::Proc { proc_def, range } => {
            remap_proc_def_ranges(proc_def, map);
            *range = map.map_range(*range);
        }
        Stmt::Block { statements, range } => {
            for stmt in statements {
                remap_stmt_ranges(stmt, map);
            }
            *range = map.map_range(*range);
        }
        Stmt::Expr { expr, range } => {
            remap_expr_ranges(expr, map);
            *range = map.map_range(*range);
        }
        Stmt::VarDecl { decl, range } => {
            remap_var_decl_ranges(decl, map);
            *range = map.map_range(*range);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            range,
        } => {
            remap_expr_ranges(condition, map);
            remap_stmt_ranges(then_branch, map);
            if let Some(else_branch) = else_branch {
                remap_stmt_ranges(else_branch, map);
            }
            *range = map.map_range(*range);
        }
        Stmt::While {
            condition,
            body,
            range,
        } => {
            remap_expr_ranges(condition, map);
            remap_stmt_ranges(body, map);
            *range = map.map_range(*range);
        }
        Stmt::DoWhile {
            body,
            condition,
            range,
        } => {
            remap_stmt_ranges(body, map);
            remap_expr_ranges(condition, map);
            *range = map.map_range(*range);
        }
        Stmt::Switch {
            control,
            clauses,
            range,
        } => {
            remap_expr_ranges(control, map);
            for clause in clauses {
                match &mut clause.label {
                    SwitchLabel::Case(expr) => remap_expr_ranges(expr, map),
                    SwitchLabel::Default { range } => *range = map.map_range(*range),
                }
                for stmt in &mut clause.statements {
                    remap_stmt_ranges(stmt, map);
                }
                clause.range = map.map_range(clause.range);
            }
            *range = map.map_range(*range);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            range,
        } => {
            if let Some(init) = init {
                for expr in init {
                    remap_expr_ranges(expr, map);
                }
            }
            if let Some(condition) = condition {
                remap_expr_ranges(condition, map);
            }
            if let Some(update) = update {
                for expr in update {
                    remap_expr_ranges(expr, map);
                }
            }
            remap_stmt_ranges(body, map);
            *range = map.map_range(*range);
        }
        Stmt::ForIn {
            binding,
            iterable,
            body,
            range,
        } => {
            remap_expr_ranges(binding, map);
            remap_expr_ranges(iterable, map);
            remap_stmt_ranges(body, map);
            *range = map.map_range(*range);
        }
        Stmt::Return { expr, range } => {
            if let Some(expr) = expr {
                remap_expr_ranges(expr, map);
            }
            *range = map.map_range(*range);
        }
    }
}

fn remap_var_decl_ranges(decl: &mut VarDecl, map: &OffsetMap) {
    for declarator in &mut decl.declarators {
        if let Some(Some(size)) = &mut declarator.array_size {
            remap_expr_ranges(size, map);
        }
        if let Some(initializer) = &mut declarator.initializer {
            remap_expr_ranges(initializer, map);
        }
        declarator.name_range = map.map_range(declarator.name_range);
        declarator.range = map.map_range(declarator.range);
    }
    decl.range = map.map_range(decl.range);
}

fn remap_expr_ranges(expr: &mut Expr, map: &OffsetMap) {
    match expr {
        Expr::Ident { name_range, range } => {
            *name_range = map.map_range(*name_range);
            *range = map.map_range(*range);
        }
        Expr::BareWord { text, range }
        | Expr::Float { text, range }
        | Expr::String { text, range } => {
            *text = map.map_range(*text);
            *range = map.map_range(*range);
        }
        Expr::Int { range, .. } => *range = map.map_range(*range),
        Expr::Cast { expr, range, .. } => {
            remap_expr_ranges(expr, map);
            *range = map.map_range(*range);
        }
        Expr::VectorLiteral { elements, range } | Expr::ArrayLiteral { elements, range } => {
            for element in elements {
                remap_expr_ranges(element, map);
            }
            *range = map.map_range(*range);
        }
        Expr::Unary { expr, range, .. }
        | Expr::PrefixUpdate { expr, range, .. }
        | Expr::PostfixUpdate { expr, range, .. } => {
            remap_expr_ranges(expr, map);
            *range = map.map_range(*range);
        }
        Expr::Binary {
            lhs, rhs, range, ..
        }
        | Expr::Assign {
            lhs, rhs, range, ..
        } => {
            remap_expr_ranges(lhs, map);
            remap_expr_ranges(rhs, map);
            *range = map.map_range(*range);
        }
        Expr::Ternary {
            condition,
            then_expr,
            else_expr,
            range,
        } => {
            remap_expr_ranges(condition, map);
            remap_expr_ranges(then_expr, map);
            remap_expr_ranges(else_expr, map);
            *range = map.map_range(*range);
        }
        Expr::Index {
            target,
            index,
            range,
        } => {
            remap_expr_ranges(target, map);
            remap_expr_ranges(index, map);
            *range = map.map_range(*range);
        }
        Expr::MemberAccess {
            target,
            member,
            range,
        } => {
            remap_expr_ranges(target, map);
            *member = map.map_range(*member);
            *range = map.map_range(*range);
        }
        Expr::ComponentAccess { target, range, .. } => {
            remap_expr_ranges(target, map);
            *range = map.map_range(*range);
        }
        Expr::Invoke(invoke) => remap_invoke_ranges(invoke, map),
    }
}

fn remap_invoke_ranges(invoke: &mut InvokeExpr, map: &OffsetMap) {
    match &mut invoke.surface {
        InvokeSurface::Function {
            head_range, args, ..
        } => {
            *head_range = map.map_range(*head_range);
            for arg in args {
                remap_expr_ranges(arg, map);
            }
        }
        InvokeSurface::ShellLike {
            head_range, words, ..
        } => {
            *head_range = map.map_range(*head_range);
            for word in words {
                remap_shell_word_ranges(word, map);
            }
        }
    }
    invoke.range = map.map_range(invoke.range);
}

fn remap_shell_word_ranges(word: &mut ShellWord, map: &OffsetMap) {
    match word {
        ShellWord::Flag { text, range } => {
            *text = map.map_range(*text);
            *range = map.map_range(*range);
        }
        ShellWord::NumericLiteral { text, range }
        | ShellWord::BareWord { text, range }
        | ShellWord::QuotedString { text, range } => {
            *text = map.map_range(*text);
            *range = map.map_range(*range);
        }
        ShellWord::Variable { expr, range }
        | ShellWord::GroupedExpr { expr, range }
        | ShellWord::BraceList { expr, range }
        | ShellWord::VectorLiteral { expr, range } => {
            remap_expr_ranges(expr, map);
            *range = map.map_range(*range);
        }
        ShellWord::Capture { invoke, range } => {
            remap_invoke_ranges(invoke, map);
            *range = map.map_range(*range);
        }
    }
}
