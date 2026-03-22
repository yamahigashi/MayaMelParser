use std::collections::{HashMap, HashSet};

use crate::scope::CollectedScopes;
use crate::*;
use mel_ast::{Expr, Item, ProcDef, ShellWord, SourceFile, Stmt, SwitchClause};
use mel_syntax::TextRange;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FlowState {
    explicitly_written: HashSet<VariableSymbolId>,
}

impl FlowState {
    fn meet(&self, other: &Self) -> Self {
        Self {
            explicitly_written: self
                .explicitly_written
                .intersection(&other.explicitly_written)
                .copied()
                .collect(),
        }
    }

    fn mark_written(&mut self, symbol_id: VariableSymbolId) {
        self.explicitly_written.insert(symbol_id);
    }

    fn is_written(&self, symbol_id: VariableSymbolId) -> bool {
        self.explicitly_written.contains(&symbol_id)
    }
}

#[derive(Clone, Debug, Default)]
struct FlowOutcome {
    fallthrough: Option<FlowState>,
    breaks: Option<FlowState>,
    continues: Option<FlowState>,
    returns: Option<FlowState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssignAccess {
    WriteOnly,
    ReadWrite,
}

pub(crate) struct FlowLintAnalyzer<'a> {
    collected: &'a CollectedScopes,
    pub(crate) diagnostics: Vec<Diagnostic>,
    visible_variable_decl_orders: HashMap<ScopeId, usize>,
    emitted_warnings: HashSet<(TextRange, String)>,
}

impl<'a> FlowLintAnalyzer<'a> {
    pub(crate) fn new(collected: &'a CollectedScopes) -> Self {
        Self {
            collected,
            diagnostics: Vec::new(),
            visible_variable_decl_orders: HashMap::new(),
            emitted_warnings: HashSet::new(),
        }
    }

    pub(crate) fn walk_source(&mut self, source: &SourceFile) {
        let root_scope = self.collected.root_scope;
        let mut state = FlowState::default();
        for item in &source.items {
            let outcome = self.walk_item(item, root_scope, state.clone());
            if let Some(next) = outcome.fallthrough {
                state = next;
            }
        }
    }

    fn walk_item(&mut self, item: &Item, current_scope: ScopeId, state: FlowState) -> FlowOutcome {
        match item {
            Item::Proc(proc_def) => {
                self.walk_proc_def(proc_def, current_scope, state.clone());
                FlowOutcome {
                    fallthrough: Some(state),
                    ..FlowOutcome::default()
                }
            }
            Item::Stmt(stmt) => self.walk_stmt(stmt, current_scope, state),
        }
    }

    fn walk_proc_def(&mut self, proc_def: &ProcDef, current_scope: ScopeId, state: FlowState) {
        let body_scope = self.collected.scope_for_stmt(&proc_def.body);
        self.mark_proc_params_visible(proc_def);
        let _ = self.walk_stmt_in_existing_scope(&proc_def.body, body_scope, state);
        debug_assert_eq!(
            self.collected.symbol_for_proc(proc_def).owner_scope,
            current_scope
        );
    }

    fn walk_stmt(&mut self, stmt: &Stmt, current_scope: ScopeId, state: FlowState) -> FlowOutcome {
        match stmt {
            Stmt::Empty { .. } => FlowOutcome {
                fallthrough: Some(state),
                ..FlowOutcome::default()
            },
            Stmt::Break { .. } => FlowOutcome {
                breaks: Some(state),
                ..FlowOutcome::default()
            },
            Stmt::Continue { .. } => FlowOutcome {
                continues: Some(state),
                ..FlowOutcome::default()
            },
            Stmt::Return { expr, .. } => {
                let mut next = state;
                if let Some(expr) = expr {
                    self.walk_expr(expr, current_scope, &mut next);
                }
                FlowOutcome {
                    returns: Some(next),
                    ..FlowOutcome::default()
                }
            }
            Stmt::Proc { proc_def, .. } => {
                self.walk_proc_def(proc_def, current_scope, state.clone());
                FlowOutcome {
                    fallthrough: Some(state),
                    ..FlowOutcome::default()
                }
            }
            Stmt::Expr { expr, .. } => {
                let mut next = state;
                self.walk_expr(expr, current_scope, &mut next);
                FlowOutcome {
                    fallthrough: Some(next),
                    ..FlowOutcome::default()
                }
            }
            Stmt::VarDecl { decl, .. } => {
                self.emit_shadowing_warnings(decl, current_scope);
                let mut next = state;
                for declarator in &decl.declarators {
                    if let Some(Some(size)) = &declarator.array_size {
                        self.walk_expr(size, current_scope, &mut next);
                    }

                    if let Some(initializer) = &declarator.initializer {
                        self.walk_expr(initializer, current_scope, &mut next);
                    }
                }
                self.mark_stmt_variables_visible(stmt);
                for variable_id in self.collected.variable_symbols_for_stmt(stmt) {
                    let symbol = self.collected.variable_symbol(*variable_id);
                    if !tracks_explicit_write(symbol) {
                        continue;
                    }

                    let declarator = decl
                        .declarators
                        .iter()
                        .find(|declarator| declarator.name == symbol.name);
                    if declarator.is_some_and(|declarator| declarator.initializer.is_some()) {
                        next.mark_written(*variable_id);
                    }
                }
                FlowOutcome {
                    fallthrough: Some(next),
                    ..FlowOutcome::default()
                }
            }
            Stmt::Block { .. } => {
                let block_scope = self.collected.scope_for_stmt(stmt);
                self.walk_stmt_in_existing_scope(stmt, block_scope, state)
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let mut condition_state = state.clone();
                self.walk_expr(condition, current_scope, &mut condition_state);

                let then_outcome =
                    self.walk_stmt_in_child_scope(then_branch, condition_state.clone());
                let else_outcome = else_branch
                    .as_deref()
                    .map(|else_branch| {
                        self.walk_stmt_in_child_scope(else_branch, condition_state.clone())
                    })
                    .unwrap_or_else(|| FlowOutcome {
                        fallthrough: Some(condition_state.clone()),
                        ..FlowOutcome::default()
                    });

                FlowOutcome {
                    fallthrough: meet_optional_states(
                        then_outcome.fallthrough,
                        else_outcome.fallthrough,
                    ),
                    breaks: meet_optional_states(then_outcome.breaks, else_outcome.breaks),
                    continues: meet_optional_states(then_outcome.continues, else_outcome.continues),
                    returns: meet_optional_states(then_outcome.returns, else_outcome.returns),
                }
            }
            Stmt::While {
                condition, body, ..
            } => self.walk_while_stmt(condition, body, current_scope, state),
            Stmt::DoWhile {
                body, condition, ..
            } => self.walk_do_while_stmt(body, condition, current_scope, state),
            Stmt::Switch {
                control, clauses, ..
            } => self.walk_switch_stmt(control, clauses, current_scope, state),
            Stmt::For {
                init,
                condition,
                update,
                body,
                ..
            } => self.walk_for_stmt(
                init.as_deref(),
                condition.as_ref(),
                update.as_deref(),
                body,
                current_scope,
                state,
            ),
            Stmt::ForIn {
                binding,
                iterable,
                body,
                ..
            } => self.walk_for_in_stmt(binding, iterable, body, current_scope, state),
        }
    }

    fn walk_stmt_in_child_scope(&mut self, stmt: &Stmt, state: FlowState) -> FlowOutcome {
        let child_scope = self.collected.scope_for_stmt(stmt);
        self.walk_stmt_in_existing_scope(stmt, child_scope, state)
    }

    fn walk_stmt_in_existing_scope(
        &mut self,
        stmt: &Stmt,
        current_scope: ScopeId,
        mut state: FlowState,
    ) -> FlowOutcome {
        match stmt {
            Stmt::Block { statements, .. } => {
                let mut outcome = FlowOutcome::default();
                for stmt in statements {
                    let step = self.walk_stmt(stmt, current_scope, state.clone());
                    outcome.breaks = meet_optional_states(outcome.breaks, step.breaks);
                    outcome.continues = meet_optional_states(outcome.continues, step.continues);
                    outcome.returns = meet_optional_states(outcome.returns, step.returns);

                    let Some(next) = step.fallthrough else {
                        outcome.fallthrough = None;
                        return outcome;
                    };
                    state = next;
                }
                outcome.fallthrough = Some(state);
                outcome
            }
            _ => self.walk_stmt(stmt, current_scope, state),
        }
    }

    fn walk_while_stmt(
        &mut self,
        condition: &Expr,
        body: &Stmt,
        current_scope: ScopeId,
        entry: FlowState,
    ) -> FlowOutcome {
        let mut head_state = entry.clone();
        let mut break_state = None;
        let mut return_state = None;

        loop {
            let mut condition_state = head_state.clone();
            self.walk_expr(condition, current_scope, &mut condition_state);
            let body_outcome = self.walk_stmt_in_child_scope(body, condition_state);
            break_state = meet_optional_states(break_state, body_outcome.breaks);
            return_state = meet_optional_states(return_state, body_outcome.returns);

            let back_edge = meet_optional_states(body_outcome.fallthrough, body_outcome.continues);
            let new_head = if let Some(back_edge) = back_edge {
                entry.meet(&back_edge)
            } else {
                entry.clone()
            };

            if new_head == head_state {
                let mut exit_state = head_state.clone();
                if let Some(break_state) = break_state.clone() {
                    exit_state = exit_state.meet(&break_state);
                }
                return FlowOutcome {
                    fallthrough: Some(exit_state),
                    returns: return_state,
                    ..FlowOutcome::default()
                };
            }

            head_state = new_head;
        }
    }

    fn walk_do_while_stmt(
        &mut self,
        body: &Stmt,
        condition: &Expr,
        current_scope: ScopeId,
        entry: FlowState,
    ) -> FlowOutcome {
        let mut body_entry = entry.clone();
        let mut break_state = None;
        let mut return_state = None;

        loop {
            let body_outcome = self.walk_stmt_in_child_scope(body, body_entry.clone());
            break_state = meet_optional_states(break_state, body_outcome.breaks);
            return_state = meet_optional_states(return_state, body_outcome.returns);

            let Some(mut condition_state) =
                meet_optional_states(body_outcome.fallthrough, body_outcome.continues)
            else {
                return FlowOutcome {
                    fallthrough: break_state,
                    returns: return_state,
                    ..FlowOutcome::default()
                };
            };
            self.walk_expr(condition, current_scope, &mut condition_state);

            let new_body_entry = entry.meet(&condition_state);
            if new_body_entry == body_entry {
                return FlowOutcome {
                    fallthrough: meet_optional_states(Some(condition_state), break_state.clone()),
                    returns: return_state,
                    ..FlowOutcome::default()
                };
            }

            body_entry = new_body_entry;
        }
    }

    fn walk_switch_stmt(
        &mut self,
        control: &Expr,
        clauses: &[SwitchClause],
        current_scope: ScopeId,
        entry: FlowState,
    ) -> FlowOutcome {
        let mut control_state = entry.clone();
        self.walk_expr(control, current_scope, &mut control_state);

        for clause in clauses {
            if let mel_ast::SwitchLabel::Case(expr) = &clause.label {
                let mut label_state = control_state.clone();
                self.walk_expr(expr, current_scope, &mut label_state);
            }
        }

        let mut exit_state = if clauses
            .iter()
            .any(|clause| matches!(clause.label, mel_ast::SwitchLabel::Default { .. }))
        {
            None
        } else {
            Some(control_state.clone())
        };
        let mut continue_state = None;
        let mut return_state = None;

        for start in 0..clauses.len() {
            let outcome = self.walk_switch_suffix(clauses, start, control_state.clone());
            let clause_exit = meet_optional_states(outcome.fallthrough, outcome.breaks);
            exit_state = meet_optional_states(exit_state, clause_exit);
            continue_state = meet_optional_states(continue_state, outcome.continues);
            return_state = meet_optional_states(return_state, outcome.returns);
        }

        FlowOutcome {
            fallthrough: exit_state.or(Some(control_state)),
            continues: continue_state,
            returns: return_state,
            ..FlowOutcome::default()
        }
    }

    fn walk_switch_suffix(
        &mut self,
        clauses: &[SwitchClause],
        start: usize,
        mut state: FlowState,
    ) -> FlowOutcome {
        let mut outcome = FlowOutcome::default();

        for clause in clauses.iter().skip(start) {
            let clause_scope = self.collected.scope_for_clause(clause);
            let clause_outcome =
                self.walk_clause_statements(&clause.statements, clause_scope, state.clone());
            outcome.breaks = meet_optional_states(outcome.breaks, clause_outcome.breaks);
            outcome.continues = meet_optional_states(outcome.continues, clause_outcome.continues);
            outcome.returns = meet_optional_states(outcome.returns, clause_outcome.returns);

            let Some(next) = clause_outcome.fallthrough else {
                outcome.fallthrough = None;
                return outcome;
            };
            state = next;
        }

        outcome.fallthrough = Some(state);
        outcome
    }

    fn walk_clause_statements(
        &mut self,
        statements: &[Stmt],
        clause_scope: ScopeId,
        mut state: FlowState,
    ) -> FlowOutcome {
        let mut outcome = FlowOutcome::default();
        for stmt in statements {
            let step = self.walk_stmt(stmt, clause_scope, state.clone());
            outcome.breaks = meet_optional_states(outcome.breaks, step.breaks);
            outcome.continues = meet_optional_states(outcome.continues, step.continues);
            outcome.returns = meet_optional_states(outcome.returns, step.returns);

            let Some(next) = step.fallthrough else {
                outcome.fallthrough = None;
                return outcome;
            };
            state = next;
        }
        outcome.fallthrough = Some(state);
        outcome
    }

    fn walk_for_stmt(
        &mut self,
        init: Option<&[Expr]>,
        condition: Option<&Expr>,
        update: Option<&[Expr]>,
        body: &Stmt,
        current_scope: ScopeId,
        mut entry: FlowState,
    ) -> FlowOutcome {
        if let Some(init) = init {
            for expr in init {
                self.walk_expr(expr, current_scope, &mut entry);
            }
        }

        let mut head_state = entry.clone();
        let mut break_state = None;
        let mut return_state = None;

        loop {
            let mut condition_state = head_state.clone();
            if let Some(condition) = condition {
                self.walk_expr(condition, current_scope, &mut condition_state);
            }

            let body_outcome = self.walk_stmt_in_child_scope(body, condition_state);
            break_state = meet_optional_states(break_state, body_outcome.breaks);
            return_state = meet_optional_states(return_state, body_outcome.returns);

            let mut back_edge =
                meet_optional_states(body_outcome.fallthrough, body_outcome.continues);
            if let Some(mut back_state) = back_edge.take() {
                if let Some(update) = update {
                    for expr in update {
                        self.walk_expr(expr, current_scope, &mut back_state);
                    }
                }
                back_edge = Some(back_state);
            }

            let new_head = if let Some(back_edge) = back_edge {
                entry.meet(&back_edge)
            } else {
                entry.clone()
            };

            if new_head == head_state {
                let mut exit_state = head_state.clone();
                if let Some(break_state) = break_state.clone() {
                    exit_state = exit_state.meet(&break_state);
                }
                return FlowOutcome {
                    fallthrough: Some(exit_state),
                    returns: return_state,
                    ..FlowOutcome::default()
                };
            }

            head_state = new_head;
        }
    }

    fn walk_for_in_stmt(
        &mut self,
        binding: &Expr,
        iterable: &Expr,
        body: &Stmt,
        current_scope: ScopeId,
        entry: FlowState,
    ) -> FlowOutcome {
        let mut head_state = entry.clone();
        let mut break_state = None;
        let mut return_state = None;

        loop {
            let mut iter_state = head_state.clone();
            self.walk_assign_target(
                binding,
                current_scope,
                &mut iter_state,
                AssignAccess::WriteOnly,
            );
            self.walk_expr(iterable, current_scope, &mut iter_state);

            let body_outcome = self.walk_stmt_in_child_scope(body, iter_state);
            break_state = meet_optional_states(break_state, body_outcome.breaks);
            return_state = meet_optional_states(return_state, body_outcome.returns);

            let back_edge = meet_optional_states(body_outcome.fallthrough, body_outcome.continues);
            let new_head = if let Some(back_edge) = back_edge {
                entry.meet(&back_edge)
            } else {
                entry.clone()
            };

            if new_head == head_state {
                let mut exit_state = head_state.clone();
                if let Some(break_state) = break_state.clone() {
                    exit_state = exit_state.meet(&break_state);
                }
                return FlowOutcome {
                    fallthrough: Some(exit_state),
                    returns: return_state,
                    ..FlowOutcome::default()
                };
            }

            head_state = new_head;
        }
    }

    fn walk_expr(&mut self, expr: &Expr, current_scope: ScopeId, state: &mut FlowState) {
        match expr {
            Expr::Ident { name, range } => {
                self.check_read_before_write(name, *range, current_scope, state)
            }
            Expr::BareWord { .. } | Expr::Int { .. } | Expr::Float { .. } | Expr::String { .. } => {
            }
            Expr::Cast { expr, .. }
            | Expr::Unary { expr, .. }
            | Expr::MemberAccess { target: expr, .. }
            | Expr::ComponentAccess { target: expr, .. } => {
                self.walk_expr(expr, current_scope, state)
            }
            Expr::VectorLiteral { elements, .. } | Expr::ArrayLiteral { elements, .. } => {
                for element in elements {
                    self.walk_expr(element, current_scope, state);
                }
            }
            Expr::Binary { lhs, rhs, .. }
            | Expr::Ternary {
                condition: lhs,
                then_expr: rhs,
                else_expr: _,
                ..
            } => {
                self.walk_expr(lhs, current_scope, state);
                self.walk_expr(rhs, current_scope, state);
                if let Expr::Ternary { else_expr, .. } = expr {
                    self.walk_expr(else_expr, current_scope, state);
                }
            }
            Expr::Index { target, index, .. } => {
                self.walk_expr(target, current_scope, state);
                self.walk_expr(index, current_scope, state);
            }
            Expr::Assign { op, lhs, rhs, .. } => {
                self.walk_expr(rhs, current_scope, state);
                let access = if matches!(op, mel_ast::AssignOp::Assign) {
                    AssignAccess::WriteOnly
                } else {
                    AssignAccess::ReadWrite
                };
                self.walk_assign_target(lhs, current_scope, state, access);
            }
            Expr::PrefixUpdate { expr, .. } | Expr::PostfixUpdate { expr, .. } => {
                self.walk_assign_target(expr, current_scope, state, AssignAccess::ReadWrite);
            }
            Expr::Invoke(invoke) => self.walk_invoke(invoke, current_scope, state),
        }
    }

    fn walk_assign_target(
        &mut self,
        expr: &Expr,
        current_scope: ScopeId,
        state: &mut FlowState,
        access: AssignAccess,
    ) {
        match expr {
            Expr::Ident { name, range } => {
                if matches!(access, AssignAccess::ReadWrite) {
                    self.check_read_before_write(name, *range, current_scope, state);
                }
                if matches!(access, AssignAccess::WriteOnly | AssignAccess::ReadWrite)
                    && let Some(symbol_id) =
                        self.resolve_visible_local_variable_id(name, current_scope)
                {
                    let symbol = self.collected.variable_symbol(symbol_id);
                    if tracks_explicit_write(symbol) {
                        state.mark_written(symbol_id);
                    }
                }
            }
            Expr::Index { target, index, .. } => {
                self.walk_expr(target, current_scope, state);
                self.walk_expr(index, current_scope, state);
            }
            Expr::MemberAccess { target, .. } | Expr::ComponentAccess { target, .. } => {
                self.walk_expr(target, current_scope, state);
            }
            _ => self.walk_expr(expr, current_scope, state),
        }
    }

    fn walk_invoke(
        &mut self,
        invoke: &mel_ast::InvokeExpr,
        current_scope: ScopeId,
        state: &mut FlowState,
    ) {
        match &invoke.surface {
            mel_ast::InvokeSurface::Function { args, .. } => {
                for arg in args {
                    self.walk_expr(arg, current_scope, state);
                }
            }
            mel_ast::InvokeSurface::ShellLike { words, .. } => {
                for word in words {
                    self.walk_shell_word(word, current_scope, state);
                }
            }
        }
    }

    fn walk_shell_word(&mut self, word: &ShellWord, current_scope: ScopeId, state: &mut FlowState) {
        match word {
            ShellWord::Flag { .. }
            | ShellWord::NumericLiteral { .. }
            | ShellWord::BareWord { .. }
            | ShellWord::QuotedString { .. } => {}
            ShellWord::Variable { expr, .. }
            | ShellWord::GroupedExpr { expr, .. }
            | ShellWord::BraceList { expr, .. }
            | ShellWord::VectorLiteral { expr, .. } => self.walk_expr(expr, current_scope, state),
            ShellWord::Capture { invoke, .. } => self.walk_invoke(invoke, current_scope, state),
        }
    }

    fn emit_shadowing_warnings(&mut self, decl: &mel_ast::VarDecl, current_scope: ScopeId) {
        if decl.is_global {
            return;
        }

        for declarator in &decl.declarators {
            if let Some(symbol) =
                self.find_visible_variable_for_shadowing(&declarator.name, current_scope)
            {
                self.push_warning_once(
                    declarator.range,
                    format!(
                        "local variable \"{}\" shadows visible {} variable",
                        declarator.name,
                        shadowed_variable_kind(symbol)
                    ),
                );
            }
        }
    }

    fn check_read_before_write(
        &mut self,
        name: &str,
        range: TextRange,
        current_scope: ScopeId,
        state: &FlowState,
    ) {
        let Some(symbol_id) = self.resolve_visible_local_variable_id(name, current_scope) else {
            return;
        };
        let symbol = self.collected.variable_symbol(symbol_id);
        if !tracks_explicit_write(symbol) || state.is_written(symbol_id) {
            return;
        }

        self.push_warning_once(
            range,
            format!(
                "local variable \"{}\" is read before its first explicit write; MEL would use a default value here",
                name
            ),
        );
    }

    fn push_warning_once(&mut self, range: TextRange, message: String) {
        let key = (range, message.clone());
        if self.emitted_warnings.insert(key) {
            self.diagnostics.push(Diagnostic::warning(message, range));
        }
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

    fn resolve_visible_local_variable_id(
        &self,
        name: &str,
        current_scope: ScopeId,
    ) -> Option<VariableSymbolId> {
        self.collected
            .find_visible_local_variable(name, current_scope, &self.visible_variable_decl_orders)
            .map(|symbol| symbol.id)
    }

    fn find_visible_variable_for_shadowing(
        &self,
        name: &str,
        current_scope: ScopeId,
    ) -> Option<&VariableSymbol> {
        self.collected
            .find_visible_local_variable(name, current_scope, &self.visible_variable_decl_orders)
            .or_else(|| self.collected.find_global_variable(name))
    }
}

fn meet_optional_states(lhs: Option<FlowState>, rhs: Option<FlowState>) -> Option<FlowState> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(lhs.meet(&rhs)),
        (Some(lhs), None) => Some(lhs),
        (None, Some(rhs)) => Some(rhs),
        (None, None) => None,
    }
}

fn tracks_explicit_write(symbol: &VariableSymbol) -> bool {
    matches!(symbol.kind, VariableKind::Local)
        && !symbol.is_array
        && symbol.ty != mel_ast::TypeName::Matrix
}

fn shadowed_variable_kind(symbol: &VariableSymbol) -> &'static str {
    match symbol.kind {
        VariableKind::Parameter => "parameter",
        VariableKind::Local => "local",
        VariableKind::Global => "global",
    }
}
