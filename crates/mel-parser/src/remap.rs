use mel_ast::{
    Expr, InvokeExpr, InvokeSurface, Item, ShellWord, SourceFile, Stmt, SwitchLabel, VarDecl,
};
use mel_syntax::TextRange;

use crate::{Parse, SharedParse};

pub(crate) trait RangeMapper {
    fn map_range(&self, range: TextRange) -> TextRange;
}

pub(crate) fn remap_parse_ranges_with_mapper(parse: &mut Parse, mapper: &impl RangeMapper) {
    remap_source_file_ranges(&mut parse.syntax, mapper);

    for diagnostic in &mut parse.lex_errors {
        diagnostic.range = mapper.map_range(diagnostic.range);
    }

    for error in &mut parse.errors {
        error.range = mapper.map_range(error.range);
    }
}

pub(crate) fn remap_shared_parse_ranges_with_mapper(
    parse: &mut SharedParse,
    mapper: &impl RangeMapper,
) {
    remap_source_file_ranges(&mut parse.syntax, mapper);

    for diagnostic in &mut parse.lex_errors {
        diagnostic.range = mapper.map_range(diagnostic.range);
    }

    for error in &mut parse.errors {
        error.range = mapper.map_range(error.range);
    }
}

pub(crate) fn remap_source_file_ranges(source: &mut SourceFile, mapper: &impl RangeMapper) {
    for item in &mut source.items {
        match item {
            Item::Proc(proc_def) => remap_proc_def_ranges(proc_def, mapper),
            Item::Stmt(stmt) => remap_stmt_ranges(stmt, mapper),
        }
    }
}

fn remap_proc_def_ranges(proc_def: &mut mel_ast::ProcDef, mapper: &impl RangeMapper) {
    if let Some(return_type) = &mut proc_def.return_type {
        return_type.range = mapper.map_range(return_type.range);
    }

    for param in &mut proc_def.params {
        param.name_range = mapper.map_range(param.name_range);
        param.range = mapper.map_range(param.range);
    }

    remap_stmt_ranges(&mut proc_def.body, mapper);
    proc_def.name_range = mapper.map_range(proc_def.name_range);
    proc_def.range = mapper.map_range(proc_def.range);
}

fn remap_stmt_ranges(stmt: &mut Stmt, mapper: &impl RangeMapper) {
    match stmt {
        Stmt::Empty { range } | Stmt::Break { range } | Stmt::Continue { range } => {
            *range = mapper.map_range(*range);
        }
        Stmt::Proc { proc_def, range } => {
            remap_proc_def_ranges(proc_def, mapper);
            *range = mapper.map_range(*range);
        }
        Stmt::Block { statements, range } => {
            for stmt in statements {
                remap_stmt_ranges(stmt, mapper);
            }
            *range = mapper.map_range(*range);
        }
        Stmt::Expr { expr, range } => {
            remap_expr_ranges(expr, mapper);
            *range = mapper.map_range(*range);
        }
        Stmt::VarDecl { decl, range } => {
            remap_var_decl_ranges(decl, mapper);
            *range = mapper.map_range(*range);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            range,
        } => {
            remap_expr_ranges(condition, mapper);
            remap_stmt_ranges(then_branch, mapper);
            if let Some(else_branch) = else_branch {
                remap_stmt_ranges(else_branch, mapper);
            }
            *range = mapper.map_range(*range);
        }
        Stmt::While {
            condition,
            body,
            range,
        } => {
            remap_expr_ranges(condition, mapper);
            remap_stmt_ranges(body, mapper);
            *range = mapper.map_range(*range);
        }
        Stmt::DoWhile {
            body,
            condition,
            range,
        } => {
            remap_stmt_ranges(body, mapper);
            remap_expr_ranges(condition, mapper);
            *range = mapper.map_range(*range);
        }
        Stmt::Switch {
            control,
            clauses,
            range,
        } => {
            remap_expr_ranges(control, mapper);
            for clause in clauses {
                match &mut clause.label {
                    SwitchLabel::Case(expr) => remap_expr_ranges(expr, mapper),
                    SwitchLabel::Default { range } => *range = mapper.map_range(*range),
                }
                for stmt in &mut clause.statements {
                    remap_stmt_ranges(stmt, mapper);
                }
                clause.range = mapper.map_range(clause.range);
            }
            *range = mapper.map_range(*range);
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
                    remap_expr_ranges(expr, mapper);
                }
            }
            if let Some(condition) = condition {
                remap_expr_ranges(condition, mapper);
            }
            if let Some(update) = update {
                for expr in update {
                    remap_expr_ranges(expr, mapper);
                }
            }
            remap_stmt_ranges(body, mapper);
            *range = mapper.map_range(*range);
        }
        Stmt::ForIn {
            binding,
            iterable,
            body,
            range,
        } => {
            remap_expr_ranges(binding, mapper);
            remap_expr_ranges(iterable, mapper);
            remap_stmt_ranges(body, mapper);
            *range = mapper.map_range(*range);
        }
        Stmt::Return { expr, range } => {
            if let Some(expr) = expr {
                remap_expr_ranges(expr, mapper);
            }
            *range = mapper.map_range(*range);
        }
    }
}

fn remap_var_decl_ranges(decl: &mut VarDecl, mapper: &impl RangeMapper) {
    for declarator in &mut decl.declarators {
        if let Some(Some(size)) = &mut declarator.array_size {
            remap_expr_ranges(size, mapper);
        }
        if let Some(initializer) = &mut declarator.initializer {
            remap_expr_ranges(initializer, mapper);
        }
        declarator.name_range = mapper.map_range(declarator.name_range);
        declarator.range = mapper.map_range(declarator.range);
    }
    decl.range = mapper.map_range(decl.range);
}

fn remap_expr_ranges(expr: &mut Expr, mapper: &impl RangeMapper) {
    match expr {
        Expr::Ident { name_range, range } => {
            *name_range = mapper.map_range(*name_range);
            *range = mapper.map_range(*range);
        }
        Expr::BareWord { text, range }
        | Expr::Float { text, range }
        | Expr::String { text, range } => {
            *text = mapper.map_range(*text);
            *range = mapper.map_range(*range);
        }
        Expr::Int { range, .. } => *range = mapper.map_range(*range),
        Expr::Cast { expr, range, .. } => {
            remap_expr_ranges(expr, mapper);
            *range = mapper.map_range(*range);
        }
        Expr::VectorLiteral { elements, range } | Expr::ArrayLiteral { elements, range } => {
            for element in elements {
                remap_expr_ranges(element, mapper);
            }
            *range = mapper.map_range(*range);
        }
        Expr::Unary { expr, range, .. }
        | Expr::PrefixUpdate { expr, range, .. }
        | Expr::PostfixUpdate { expr, range, .. } => {
            remap_expr_ranges(expr, mapper);
            *range = mapper.map_range(*range);
        }
        Expr::Binary {
            lhs, rhs, range, ..
        }
        | Expr::Assign {
            lhs, rhs, range, ..
        } => {
            remap_expr_ranges(lhs, mapper);
            remap_expr_ranges(rhs, mapper);
            *range = mapper.map_range(*range);
        }
        Expr::Ternary {
            condition,
            then_expr,
            else_expr,
            range,
        } => {
            remap_expr_ranges(condition, mapper);
            remap_expr_ranges(then_expr, mapper);
            remap_expr_ranges(else_expr, mapper);
            *range = mapper.map_range(*range);
        }
        Expr::Index {
            target,
            index,
            range,
        } => {
            remap_expr_ranges(target, mapper);
            remap_expr_ranges(index, mapper);
            *range = mapper.map_range(*range);
        }
        Expr::MemberAccess {
            target,
            member,
            range,
        } => {
            remap_expr_ranges(target, mapper);
            *member = mapper.map_range(*member);
            *range = mapper.map_range(*range);
        }
        Expr::ComponentAccess { target, range, .. } => {
            remap_expr_ranges(target, mapper);
            *range = mapper.map_range(*range);
        }
        Expr::Invoke(invoke) => remap_invoke_ranges(invoke, mapper),
    }
}

fn remap_invoke_ranges(invoke: &mut InvokeExpr, mapper: &impl RangeMapper) {
    match &mut invoke.surface {
        InvokeSurface::Function {
            head_range, args, ..
        } => {
            *head_range = mapper.map_range(*head_range);
            for arg in args {
                remap_expr_ranges(arg, mapper);
            }
        }
        InvokeSurface::ShellLike {
            head_range, words, ..
        } => {
            *head_range = mapper.map_range(*head_range);
            for word in words {
                remap_shell_word_ranges(word, mapper);
            }
        }
    }
    invoke.range = mapper.map_range(invoke.range);
}

fn remap_shell_word_ranges(word: &mut ShellWord, mapper: &impl RangeMapper) {
    match word {
        ShellWord::Flag { text, range }
        | ShellWord::NumericLiteral { text, range }
        | ShellWord::BareWord { text, range }
        | ShellWord::QuotedString { text, range } => {
            *text = mapper.map_range(*text);
            *range = mapper.map_range(*range);
        }
        ShellWord::Variable { expr, range } => {
            remap_expr_ranges(expr, mapper);
            *range = mapper.map_range(*range);
        }
        ShellWord::GroupedExpr { expr, range }
        | ShellWord::BraceList { expr, range }
        | ShellWord::VectorLiteral { expr, range } => {
            remap_expr_ranges(expr, mapper);
            *range = mapper.map_range(*range);
        }
        ShellWord::Capture { invoke, range } => {
            remap_invoke_ranges(invoke, mapper);
            *range = mapper.map_range(*range);
        }
    }
}
