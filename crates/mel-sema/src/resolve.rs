use std::collections::HashMap;

use crate::scope::CollectedScopes;
use crate::*;
use mel_ast::{Expr, Item, ProcDef, ShellWord, Stmt};
use mel_syntax::{SourceView, TextRange};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ValueType {
    Int,
    Float,
    String,
    Vector,
    Matrix,
    Array(Box<ValueType>),
    Unknown,
}

struct AssignmentTargetInfo {
    name_range: TextRange,
    declaration_range: TextRange,
    value_type: ValueType,
}

pub(crate) struct Analyzer<'a, R: ?Sized> {
    collected: &'a CollectedScopes,
    source: SourceView<'a>,
    registry: &'a R,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) invoke_resolutions: Vec<InvokeResolution>,
    pub(crate) ident_resolutions: Vec<IdentResolution>,
    pub(crate) normalized_invokes: Vec<NormalizedCommandInvoke>,
    visible_decl_orders: HashMap<ScopeId, usize>,
    visible_variable_decl_orders: HashMap<ScopeId, usize>,
    implicit_variables_by_scope: HashMap<ScopeId, Vec<TextRange>>,
    proc_contexts: Vec<ProcContext>,
}

struct ProcContext {
    name_range: TextRange,
    range: TextRange,
    return_type: Option<ValueType>,
    saw_value_return: bool,
}

enum ResolvedInvokeTarget {
    Proc(ProcSymbolId),
    Command(CommandSchema),
    Unresolved,
}

impl ResolvedInvokeTarget {
    fn proc_symbol(&self) -> Option<ProcSymbolId> {
        match self {
            Self::Proc(symbol_id) => Some(*symbol_id),
            Self::Command(_) | Self::Unresolved => None,
        }
    }

    fn into_callee_resolution(self) -> ResolvedCallee {
        match self {
            Self::Proc(symbol_id) => ResolvedCallee::Proc(symbol_id),
            Self::Command(command) => match command.kind {
                CommandKind::Builtin => ResolvedCallee::BuiltinCommand(command.name.clone()),
                CommandKind::Plugin => ResolvedCallee::PluginCommand(command.name.clone()),
            },
            Self::Unresolved => ResolvedCallee::Unresolved,
        }
    }
}

impl<'a, R> Analyzer<'a, R>
where
    R: CommandRegistry + ?Sized,
{
    pub(crate) fn new(
        collected: &'a CollectedScopes,
        source: SourceView<'a>,
        registry: &'a R,
    ) -> Self {
        Self {
            collected,
            source,
            registry,
            diagnostics: Vec::new(),
            invoke_resolutions: Vec::new(),
            ident_resolutions: Vec::new(),
            normalized_invokes: Vec::new(),
            visible_decl_orders: HashMap::new(),
            visible_variable_decl_orders: HashMap::new(),
            implicit_variables_by_scope: HashMap::new(),
            proc_contexts: Vec::new(),
        }
    }

    pub(crate) fn walk_item(&mut self, item: &Item, current_scope: ScopeId) {
        match item {
            Item::Proc(proc_def) => self.walk_proc_def(proc_def, current_scope),
            Item::Stmt(stmt) => self.walk_stmt(stmt, current_scope),
        }
    }

    fn walk_proc_def(&mut self, proc_def: &ProcDef, current_scope: ScopeId) {
        self.mark_proc_visible(proc_def);
        let body_scope = self.collected.scope_for_stmt(&proc_def.body);
        self.mark_proc_params_visible(proc_def);
        self.proc_contexts.push(ProcContext {
            name_range: proc_def.name_range,
            range: proc_def.range,
            return_type: proc_def
                .return_type
                .as_ref()
                .map(value_type_from_proc_return_type),
            saw_value_return: false,
        });
        self.walk_stmt_in_existing_scope(&proc_def.body, body_scope);
        self.finish_proc_context();
        debug_assert_eq!(
            self.collected.symbol_for_proc(proc_def).owner_scope,
            current_scope
        );
    }

    fn walk_stmt(&mut self, stmt: &Stmt, current_scope: ScopeId) {
        match stmt {
            Stmt::Empty { .. } | Stmt::Break { .. } | Stmt::Continue { .. } => {}
            Stmt::Block { .. } => {
                let block_scope = self.collected.scope_for_stmt(stmt);
                self.walk_stmt_in_existing_scope(stmt, block_scope);
            }
            Stmt::Proc { proc_def, .. } => {
                self.walk_proc_def(proc_def, current_scope);
            }
            Stmt::Expr { expr, .. } => self.walk_expr(expr, current_scope),
            Stmt::VarDecl { decl, .. } => {
                for declarator in &decl.declarators {
                    if let Some(Some(size)) = &declarator.array_size {
                        self.walk_expr(size, current_scope);
                    }

                    if let Some(initializer) = &declarator.initializer {
                        self.walk_expr(initializer, current_scope);
                        self.validate_var_initializer(decl, declarator, initializer, current_scope);
                    }
                }
                self.mark_stmt_variables_visible(stmt);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.walk_expr(condition, current_scope);
                self.walk_stmt_in_child_scope(then_branch);
                if let Some(else_branch) = else_branch {
                    self.walk_stmt_in_child_scope(else_branch);
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                self.walk_expr(condition, current_scope);
                self.walk_stmt_in_child_scope(body);
            }
            Stmt::DoWhile {
                body, condition, ..
            } => {
                self.walk_stmt_in_child_scope(body);
                self.walk_expr(condition, current_scope);
            }
            Stmt::Switch {
                control, clauses, ..
            } => {
                self.walk_expr(control, current_scope);
                for clause in clauses {
                    if let mel_ast::SwitchLabel::Case(expr) = &clause.label {
                        self.walk_expr(expr, current_scope);
                    }
                    let clause_scope = self.collected.scope_for_clause(clause);
                    for stmt in &clause.statements {
                        self.walk_stmt(stmt, clause_scope);
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
                        self.walk_expr(expr, current_scope);
                    }
                }
                if let Some(condition) = condition {
                    self.walk_expr(condition, current_scope);
                }
                if let Some(update) = update {
                    for expr in update {
                        self.walk_expr(expr, current_scope);
                    }
                }
                self.walk_stmt_in_child_scope(body);
            }
            Stmt::ForIn {
                binding,
                iterable,
                body,
                ..
            } => {
                self.walk_expr(iterable, current_scope);
                self.walk_assign_target(binding, current_scope, false);
                self.walk_stmt_in_child_scope(body);
            }
            Stmt::Return { expr, .. } => {
                self.validate_return_stmt(expr.as_ref(), stmt_range(stmt), current_scope);
                if let Some(expr) = expr {
                    self.walk_expr(expr, current_scope);
                }
            }
        }
    }

    fn walk_stmt_in_child_scope(&mut self, stmt: &Stmt) {
        let child_scope = self.collected.scope_for_stmt(stmt);
        self.walk_stmt_in_existing_scope(stmt, child_scope);
    }

    fn walk_stmt_in_existing_scope(&mut self, stmt: &Stmt, current_scope: ScopeId) {
        match stmt {
            Stmt::Block { statements, .. } => {
                for stmt in statements {
                    self.walk_stmt(stmt, current_scope);
                }
            }
            _ => self.walk_stmt(stmt, current_scope),
        }
    }

    fn walk_expr(&mut self, expr: &Expr, current_scope: ScopeId) {
        match expr {
            Expr::Cast { expr, .. } => self.walk_expr(expr, current_scope),
            Expr::Unary { expr, .. } => self.walk_expr(expr, current_scope),
            Expr::PrefixUpdate { expr, .. } | Expr::PostfixUpdate { expr, .. } => {
                self.walk_assign_target(expr, current_scope, true);
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs, current_scope);
                self.walk_expr(rhs, current_scope);
            }
            Expr::Assign { op, lhs, rhs, .. } => {
                self.walk_expr(rhs, current_scope);
                self.validate_assignment_expr(op, lhs, rhs, current_scope);
                self.walk_assign_target(
                    lhs,
                    current_scope,
                    !matches!(op, mel_ast::AssignOp::Assign),
                );
            }
            Expr::Ternary {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                self.walk_expr(condition, current_scope);
                self.walk_expr(then_expr, current_scope);
                self.walk_expr(else_expr, current_scope);
            }
            Expr::Index { target, index, .. } => {
                self.walk_expr(target, current_scope);
                self.walk_expr(index, current_scope);
            }
            Expr::MemberAccess { target, .. } | Expr::ComponentAccess { target, .. } => {
                self.walk_expr(target, current_scope);
            }
            Expr::VectorLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
                for element in elements {
                    self.walk_expr(element, current_scope);
                }
            }
            Expr::Invoke(invoke) => self.walk_invoke(invoke, current_scope),
            Expr::Ident { name_range, range } => {
                let name = self.slice(*name_range);
                let resolution = self.resolve_ident(name, current_scope);
                if matches!(resolution, IdentTarget::Unresolved)
                    && !is_boolean_alias(name)
                    && !self.is_visible_implicit_variable(name, current_scope)
                {
                    self.diagnostics.push(Diagnostic::warning(
                        format!("unresolved variable \"{name}\""),
                        *range,
                    ));
                }
                self.ident_resolutions.push(IdentResolution {
                    range: *range,
                    scope: current_scope,
                    name_range: *name_range,
                    resolution,
                });
            }
            Expr::BareWord { .. } | Expr::Int { .. } | Expr::Float { .. } | Expr::String { .. } => {
            }
        }
    }

    fn walk_assign_target(&mut self, expr: &Expr, current_scope: ScopeId, emit_unresolved: bool) {
        match expr {
            Expr::Ident { name_range, range } => {
                let name = self.slice(*name_range);
                let resolution = self.resolve_ident(name, current_scope);
                if emit_unresolved
                    && matches!(resolution, IdentTarget::Unresolved)
                    && !self.is_visible_implicit_variable(name, current_scope)
                {
                    self.diagnostics.push(Diagnostic::warning(
                        format!("unresolved variable \"{name}\""),
                        *range,
                    ));
                }
                if !emit_unresolved && matches!(resolution, IdentTarget::Unresolved) {
                    self.mark_implicit_variable(*name_range, current_scope);
                }
                self.ident_resolutions.push(IdentResolution {
                    range: *range,
                    scope: current_scope,
                    name_range: *name_range,
                    resolution,
                });
            }
            Expr::Index { target, index, .. } => {
                self.walk_expr(target, current_scope);
                self.walk_expr(index, current_scope);
            }
            Expr::MemberAccess { target, .. } | Expr::ComponentAccess { target, .. } => {
                self.walk_expr(target, current_scope);
            }
            _ => self.walk_expr(expr, current_scope),
        }
    }

    fn walk_invoke(&mut self, invoke: &mel_ast::InvokeExpr, current_scope: ScopeId) {
        let resolution = match &invoke.surface {
            mel_ast::InvokeSurface::Function { head_range, args } => {
                for arg in args {
                    self.walk_expr(arg, current_scope);
                }
                let resolved =
                    self.resolve_named_target_range(*head_range, invoke.range, current_scope);
                self.validate_proc_arity(resolved.proc_symbol(), args.len(), invoke.range);
                resolved.into_callee_resolution()
            }
            mel_ast::InvokeSurface::ShellLike {
                head_range, words, ..
            } => {
                for word in words {
                    self.walk_shell_word(word, current_scope);
                }
                let resolved =
                    self.resolve_named_target_range(*head_range, invoke.range, current_scope);
                if let ResolvedInvokeTarget::Command(ref command) = resolved {
                    let (normalized, diagnostics) = command_norm::normalize_shell_like_invoke(
                        command,
                        current_scope,
                        *head_range,
                        words,
                        invoke.range,
                        self.source,
                    );
                    self.diagnostics.extend(diagnostics);
                    self.normalized_invokes.push(normalized);
                }
                resolved.into_callee_resolution()
            }
        };

        self.invoke_resolutions.push(InvokeResolution {
            range: invoke.range,
            scope: current_scope,
            resolution,
        });
    }

    fn validate_proc_arity(
        &mut self,
        proc_symbol: Option<ProcSymbolId>,
        actual_args: usize,
        call_range: TextRange,
    ) {
        let Some(proc_symbol) = proc_symbol else {
            return;
        };

        let symbol = self.collected.symbol(proc_symbol);
        let expected_args = self
            .collected
            .param_symbols_for_proc_range(symbol.range)
            .len();
        if actual_args == expected_args {
            return;
        }

        let proc_name = self.collected.proc_name(self.source, proc_symbol);
        self.diagnostics.push(
            Diagnostic::error(
                format!(
                    "proc \"{proc_name}\" expects {expected_args} argument(s) but call provides {actual_args}"
                ),
                call_range,
            )
            .with_secondary_label("proc defined here", symbol.name_range),
        );
    }

    fn walk_shell_word(&mut self, word: &ShellWord, current_scope: ScopeId) {
        match word {
            ShellWord::Flag { .. }
            | ShellWord::NumericLiteral { .. }
            | ShellWord::BareWord { .. }
            | ShellWord::QuotedString { .. } => {}
            ShellWord::Variable { expr, .. }
            | ShellWord::GroupedExpr { expr, .. }
            | ShellWord::BraceList { expr, .. }
            | ShellWord::VectorLiteral { expr, .. } => self.walk_expr(expr, current_scope),
            ShellWord::Capture { invoke, .. } => self.walk_invoke(invoke, current_scope),
        }
    }

    fn resolve_named_target(
        &mut self,
        name: &str,
        range: TextRange,
        current_scope: ScopeId,
    ) -> ResolvedInvokeTarget {
        if let Some(symbol) = self.collected.find_visible_local_proc(
            self.source,
            name,
            current_scope,
            &self.visible_decl_orders,
        ) {
            return ResolvedInvokeTarget::Proc(symbol.id);
        }

        if let Some(symbol) = self.collected.find_forward_local_proc(
            self.source,
            name,
            current_scope,
            &self.visible_decl_orders,
        ) {
            self.diagnostics.push(Diagnostic::error(
                format!("local proc \"{name}\" is called before its definition"),
                range,
            ));
            return ResolvedInvokeTarget::Proc(symbol.id);
        }

        if let Some(symbol) = self.collected.find_global_proc(self.source, name) {
            return ResolvedInvokeTarget::Proc(symbol.id);
        }

        if let Some(command) = self.registry.lookup(name) {
            return ResolvedInvokeTarget::Command(command);
        }

        ResolvedInvokeTarget::Unresolved
    }

    fn resolve_named_target_range(
        &mut self,
        name_range: TextRange,
        range: TextRange,
        current_scope: ScopeId,
    ) -> ResolvedInvokeTarget {
        let source = self.source;
        let name = source.slice(name_range);
        self.resolve_named_target(name, range, current_scope)
    }

    fn mark_proc_visible(&mut self, proc_def: &ProcDef) {
        let symbol = self.collected.symbol_for_proc(proc_def);
        if symbol.is_global {
            return;
        }

        let visible_order = self
            .visible_decl_orders
            .entry(symbol.owner_scope)
            .or_insert(0);
        *visible_order = (*visible_order).max(symbol.decl_order);
    }

    fn mark_proc_params_visible(&mut self, proc_def: &ProcDef) {
        for param_id in self.collected.param_symbols_for_proc(proc_def) {
            let symbol = self.collected.variable_symbol(*param_id);
            let visible_order = self
                .visible_variable_decl_orders
                .entry(symbol.owner_scope)
                .or_insert(0);
            *visible_order = (*visible_order).max(symbol.decl_order);
        }
    }

    fn mark_stmt_variables_visible(&mut self, stmt: &Stmt) {
        for variable_id in self.collected.variable_symbols_for_stmt(stmt) {
            let symbol = self.collected.variable_symbol(*variable_id);
            if matches!(symbol.kind, VariableKind::Global) {
                continue;
            }

            let visible_order = self
                .visible_variable_decl_orders
                .entry(symbol.owner_scope)
                .or_insert(0);
            *visible_order = (*visible_order).max(symbol.decl_order);
        }
    }

    fn resolve_ident(&self, name: &str, current_scope: ScopeId) -> IdentTarget {
        if let Some(symbol) = self.collected.find_visible_local_variable(
            self.source,
            name,
            current_scope,
            &self.visible_variable_decl_orders,
        ) {
            return IdentTarget::Variable(symbol.id);
        }

        if let Some(symbol) = self.collected.find_global_variable(self.source, name) {
            return IdentTarget::Variable(symbol.id);
        }

        IdentTarget::Unresolved
    }

    fn mark_implicit_variable(&mut self, name_range: TextRange, current_scope: ScopeId) {
        let source = self.source;
        let name = source.slice(name_range);
        let names = self
            .implicit_variables_by_scope
            .entry(current_scope)
            .or_default();
        if !names
            .iter()
            .any(|candidate| source.slice(*candidate) == name)
        {
            names.push(name_range);
        }
    }

    fn is_visible_implicit_variable(&self, name: &str, current_scope: ScopeId) -> bool {
        let mut scope = Some(current_scope);
        while let Some(scope_id) = scope {
            if self
                .implicit_variables_by_scope
                .get(&scope_id)
                .is_some_and(|names| names.iter().any(|candidate| self.slice(*candidate) == name))
            {
                return true;
            }
            scope = self.collected.scopes.parent(scope_id);
        }
        false
    }

    fn validate_return_stmt(
        &mut self,
        expr: Option<&Expr>,
        range: TextRange,
        current_scope: ScopeId,
    ) {
        let actual = expr.map(|expr| self.infer_expr_type(expr, current_scope));
        let Some(context) = self.proc_contexts.last_mut() else {
            return;
        };
        let context_name = self.source.slice(context.name_range);

        match (&context.return_type, actual.as_ref()) {
            (None, Some(_)) => self.diagnostics.push(Diagnostic::error(
                format!(
                    "proc \"{}\" has no return type but returns a value",
                    context_name
                ),
                range,
            )),
            (Some(expected), Some(actual)) => {
                context.saw_value_return = true;
                if !is_assignable(expected, actual) {
                    self.diagnostics.push(Diagnostic::error(
                        format!(
                            "proc \"{}\" returns {:?} but declares {:?}",
                            context_name, actual, expected
                        ),
                        range,
                    ));
                }
            }
            (Some(_), None) | (None, None) => {}
        }
    }

    fn finish_proc_context(&mut self) {
        let Some(context) = self.proc_contexts.pop() else {
            return;
        };

        if context.return_type.is_some() && !context.saw_value_return {
            self.diagnostics.push(Diagnostic::error(
                format!(
                    "proc \"{}\" declares a return type but never returns a value",
                    self.slice(context.name_range)
                ),
                context.range,
            ));
        }
    }

    fn validate_var_initializer(
        &mut self,
        decl: &mel_ast::VarDecl,
        declarator: &mel_ast::Declarator,
        initializer: &Expr,
        current_scope: ScopeId,
    ) {
        let expected = value_type_from_var_decl(decl, declarator);
        let actual = self.infer_expr_type(initializer, current_scope);
        if !is_assignable(&expected, &actual) {
            self.diagnostics.push(Diagnostic::error(
                format!(
                    "variable \"{}\" has declared type {:?} but initializer is {:?}",
                    self.slice(declarator.name_range),
                    expected,
                    actual
                ),
                initializer.range(),
            ));
        }
    }

    fn validate_assignment_expr(
        &mut self,
        op: &mel_ast::AssignOp,
        lhs: &Expr,
        rhs: &Expr,
        current_scope: ScopeId,
    ) {
        let Some(target_info) = self.infer_assignment_target_info(lhs, current_scope) else {
            return;
        };
        let rhs_ty = self.infer_expr_type(rhs, current_scope);
        if matches!(rhs_ty, ValueType::Unknown) {
            return;
        }

        let actual = match op {
            mel_ast::AssignOp::Assign => rhs_ty,
            mel_ast::AssignOp::AddAssign
            | mel_ast::AssignOp::SubAssign
            | mel_ast::AssignOp::MulAssign
            | mel_ast::AssignOp::DivAssign => {
                let combined =
                    combine_numeric_types(target_info.value_type.clone(), rhs_ty.clone());
                if matches!(combined, ValueType::Unknown) {
                    rhs_ty
                } else {
                    combined
                }
            }
        };

        if !is_assignable(&target_info.value_type, &actual) {
            self.diagnostics.push(
                Diagnostic::error(
                    format!(
                        "variable \"{}\" has declared type {:?} but assigned expression is {:?}",
                        self.slice(target_info.name_range),
                        target_info.value_type,
                        actual
                    ),
                    rhs.range(),
                )
                .with_secondary_label(
                    format!(
                        "\"{}\" declared here with type {:?}",
                        self.slice(target_info.name_range),
                        target_info.value_type
                    ),
                    target_info.declaration_range,
                ),
            );
        }
    }

    fn infer_expr_type(&self, expr: &Expr, current_scope: ScopeId) -> ValueType {
        match expr {
            Expr::Ident { name_range, .. } => {
                self.infer_ident_type(self.slice(*name_range), current_scope)
            }
            Expr::Int { .. } => ValueType::Int,
            Expr::Float { .. } => ValueType::Float,
            Expr::String { .. } | Expr::BareWord { .. } => ValueType::String,
            Expr::Cast { ty, .. } => value_type_from_type_name(ty),
            Expr::VectorLiteral { .. } => ValueType::Vector,
            Expr::ArrayLiteral { elements, .. } => {
                infer_array_literal_type(elements, self, current_scope)
            }
            Expr::Unary { op, expr, .. } => self.infer_unary_type(op, expr, current_scope),
            Expr::PrefixUpdate { expr, .. }
            | Expr::PostfixUpdate { expr, .. }
            | Expr::ComponentAccess { target: expr, .. } => {
                self.infer_expr_type(expr, current_scope)
            }
            Expr::MemberAccess { target, .. } => {
                let _ = self.infer_expr_type(target, current_scope);
                ValueType::Unknown
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                self.infer_binary_type(op, lhs, rhs, current_scope)
            }
            Expr::Assign { lhs, rhs, .. } => combine_numeric_types(
                self.infer_expr_type(lhs, current_scope),
                self.infer_expr_type(rhs, current_scope),
            ),
            Expr::Ternary {
                then_expr,
                else_expr,
                ..
            } => {
                let then_ty = self.infer_expr_type(then_expr, current_scope);
                let else_ty = self.infer_expr_type(else_expr, current_scope);
                if is_assignable(&then_ty, &else_ty) {
                    then_ty
                } else if is_assignable(&else_ty, &then_ty) {
                    else_ty
                } else {
                    ValueType::Unknown
                }
            }
            Expr::Index { target, .. } => match self.infer_expr_type(target, current_scope) {
                ValueType::Array(inner) => *inner,
                _ => ValueType::Unknown,
            },
            Expr::Invoke(invoke) => self.infer_invoke_type(invoke, current_scope),
        }
    }

    fn infer_assignment_target_info(
        &self,
        expr: &Expr,
        current_scope: ScopeId,
    ) -> Option<AssignmentTargetInfo> {
        match expr {
            Expr::Ident { name_range, .. } => {
                let name = self.slice(*name_range);
                match self.resolve_ident(name, current_scope) {
                    IdentTarget::Variable(symbol_id) => {
                        let symbol = self.collected.variable_symbol(symbol_id);
                        let base = value_type_from_type_name(&symbol.ty);
                        Some(AssignmentTargetInfo {
                            name_range: symbol.name_range,
                            declaration_range: symbol.name_range,
                            value_type: if symbol.is_array {
                                ValueType::Array(Box::new(base))
                            } else {
                                base
                            },
                        })
                    }
                    IdentTarget::Unresolved => None,
                }
            }
            Expr::Index { target, .. } => {
                match self.infer_assignment_target_info(target, current_scope) {
                    Some(AssignmentTargetInfo {
                        name_range,
                        declaration_range,
                        value_type: ValueType::Array(inner),
                    }) => Some(AssignmentTargetInfo {
                        name_range,
                        declaration_range,
                        value_type: *inner,
                    }),
                    _ => None,
                }
            }
            Expr::MemberAccess { .. } | Expr::ComponentAccess { .. } => None,
            _ => None,
        }
    }

    fn infer_ident_type(&self, name: &str, current_scope: ScopeId) -> ValueType {
        if is_boolean_alias(name) {
            return ValueType::Int;
        }

        match self.resolve_ident(name, current_scope) {
            IdentTarget::Unresolved => ValueType::Unknown,
            IdentTarget::Variable(symbol_id) => {
                let symbol = self.collected.variable_symbol(symbol_id);
                let base = value_type_from_type_name(&symbol.ty);
                if symbol.is_array {
                    ValueType::Array(Box::new(base))
                } else {
                    base
                }
            }
        }
    }

    fn infer_unary_type(
        &self,
        op: &mel_ast::UnaryOp,
        expr: &Expr,
        current_scope: ScopeId,
    ) -> ValueType {
        match op {
            mel_ast::UnaryOp::Not => {
                let _ = self.infer_expr_type(expr, current_scope);
                ValueType::Int
            }
            mel_ast::UnaryOp::Negate => self.infer_expr_type(expr, current_scope),
        }
    }

    fn infer_binary_type(
        &self,
        op: &mel_ast::BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        current_scope: ScopeId,
    ) -> ValueType {
        let lhs = self.infer_expr_type(lhs, current_scope);
        let rhs = self.infer_expr_type(rhs, current_scope);

        match op {
            mel_ast::BinaryOp::Mul
            | mel_ast::BinaryOp::Div
            | mel_ast::BinaryOp::Rem
            | mel_ast::BinaryOp::Caret
            | mel_ast::BinaryOp::Add
            | mel_ast::BinaryOp::Sub => combine_numeric_types(lhs, rhs),
            mel_ast::BinaryOp::Lt
            | mel_ast::BinaryOp::Le
            | mel_ast::BinaryOp::Gt
            | mel_ast::BinaryOp::Ge
            | mel_ast::BinaryOp::EqEq
            | mel_ast::BinaryOp::NotEq
            | mel_ast::BinaryOp::AndAnd
            | mel_ast::BinaryOp::OrOr => ValueType::Int,
        }
    }

    fn infer_invoke_type(&self, invoke: &mel_ast::InvokeExpr, current_scope: ScopeId) -> ValueType {
        let name = match &invoke.surface {
            mel_ast::InvokeSurface::Function { head_range, .. }
            | mel_ast::InvokeSurface::ShellLike { head_range, .. } => self.slice(*head_range),
        };

        let Some(symbol) = self.collected.find_resolved_proc_symbol(
            self.source,
            name,
            current_scope,
            &self.visible_decl_orders,
        ) else {
            return ValueType::Unknown;
        };

        symbol
            .return_type
            .as_ref()
            .map(value_type_from_proc_return_type)
            .unwrap_or(ValueType::Unknown)
    }

    fn slice(&self, range: TextRange) -> &str {
        self.source.slice(range)
    }
}

fn value_type_from_type_name(ty: &mel_ast::TypeName) -> ValueType {
    match ty {
        mel_ast::TypeName::Int => ValueType::Int,
        mel_ast::TypeName::Float => ValueType::Float,
        mel_ast::TypeName::String => ValueType::String,
        mel_ast::TypeName::Vector => ValueType::Vector,
        mel_ast::TypeName::Matrix => ValueType::Matrix,
    }
}

fn value_type_from_proc_return_type(return_type: &mel_ast::ProcReturnType) -> ValueType {
    let base = value_type_from_type_name(&return_type.ty);
    if return_type.is_array {
        ValueType::Array(Box::new(base))
    } else {
        base
    }
}

fn value_type_from_var_decl(
    decl: &mel_ast::VarDecl,
    declarator: &mel_ast::Declarator,
) -> ValueType {
    let base = value_type_from_type_name(&decl.ty);
    if declarator.array_size.is_some() {
        ValueType::Array(Box::new(base))
    } else {
        base
    }
}

fn is_assignable(expected: &ValueType, actual: &ValueType) -> bool {
    match (expected, actual) {
        (_, ValueType::Unknown) | (ValueType::Unknown, _) => true,
        (ValueType::Float, ValueType::Int) => true,
        (ValueType::Array(expected), ValueType::Array(actual)) => is_assignable(expected, actual),
        _ => expected == actual,
    }
}

fn combine_numeric_types(lhs: ValueType, rhs: ValueType) -> ValueType {
    match (&lhs, &rhs) {
        (ValueType::Float, ValueType::Int)
        | (ValueType::Int, ValueType::Float)
        | (ValueType::Float, ValueType::Float) => ValueType::Float,
        (ValueType::Int, ValueType::Int) => ValueType::Int,
        _ if lhs == rhs => lhs,
        _ => ValueType::Unknown,
    }
}

fn is_boolean_alias(name: &str) -> bool {
    matches!(name, "true" | "false" | "on" | "off")
}

fn infer_array_literal_type<R>(
    elements: &[Expr],
    analyzer: &Analyzer<'_, R>,
    current_scope: ScopeId,
) -> ValueType
where
    R: CommandRegistry + ?Sized,
{
    let mut iter = elements.iter();
    let Some(first) = iter.next() else {
        return ValueType::Unknown;
    };

    let first_ty = analyzer.infer_expr_type(first, current_scope);
    if iter.all(|expr| is_assignable(&first_ty, &analyzer.infer_expr_type(expr, current_scope))) {
        ValueType::Array(Box::new(first_ty))
    } else {
        ValueType::Unknown
    }
}
