#![forbid(unsafe_code)]
//! Minimal semantic analysis scaffold.

mod command_norm;
mod command_schema;

use std::collections::{HashMap, HashSet};

pub use command_norm::{
    CommandMode, NormalizedCommandInvoke, NormalizedCommandItem, NormalizedFlag, PositionalArg,
    RawShellItem, SetAttrDataReferenceEditsTail, SpecializedCommandForm,
};
pub use command_schema::{
    CommandKind, CommandModeMask, CommandRegistry, CommandSchema, CommandSourceKind,
    EmbeddedCommandRegistry, EmptyCommandRegistry, FlagArity, FlagArityByMode, FlagSchema,
    ReturnBehavior, ValueShape,
};
use mel_ast::{CalleeResolution, Expr, Item, ProcDef, ShellWord, SourceFile, Stmt, SwitchClause};
use mel_syntax::TextRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub range: TextRange,
}

impl Diagnostic {
    fn error(message: impl Into<String>, range: TextRange) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            message: message.into(),
            range,
        }
    }

    fn warning(message: impl Into<String>, range: TextRange) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            range,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcSymbolId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariableSymbolId(usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSymbol {
    pub id: ProcSymbolId,
    pub name: String,
    pub is_global: bool,
    pub return_type: Option<mel_ast::ProcReturnType>,
    pub owner_scope: ScopeId,
    pub decl_order: usize,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableKind {
    Parameter,
    Local,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableSymbol {
    pub id: VariableSymbolId,
    pub name: String,
    pub kind: VariableKind,
    pub ty: mel_ast::TypeName,
    pub is_array: bool,
    pub owner_scope: ScopeId,
    pub decl_order: usize,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvokeResolution {
    pub range: TextRange,
    pub scope: ScopeId,
    pub resolution: CalleeResolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentTarget {
    Unresolved,
    Variable(VariableSymbolId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentResolution {
    pub range: TextRange,
    pub scope: ScopeId,
    pub name: String,
    pub resolution: IdentTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub proc_symbols: Vec<ProcSymbol>,
    pub variable_symbols: Vec<VariableSymbol>,
    pub invoke_resolutions: Vec<InvokeResolution>,
    pub ident_resolutions: Vec<IdentResolution>,
    pub normalized_invokes: Vec<NormalizedCommandInvoke>,
}

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

#[must_use]
pub fn analyze(source: &SourceFile) -> Analysis {
    analyze_with_registry(source, &EmptyCommandRegistry)
}

#[must_use]
pub fn analyze_with_registry<R>(source: &SourceFile, registry: &R) -> Analysis
where
    R: CommandRegistry + ?Sized,
{
    let overlay = command_schema::OverlayCommandRegistry::new(registry);
    let collected = ScopeCollector::collect(source);
    let mut analyzer = Analyzer::new(&collected, &overlay);

    for item in &source.items {
        analyzer.walk_item(item, collected.root_scope);
    }

    let mut flow_lint = FlowLintAnalyzer::new(&collected);
    flow_lint.walk_source(source);

    let mut diagnostics = analyzer.diagnostics;
    diagnostics.extend(flow_lint.diagnostics);

    Analysis {
        diagnostics,
        proc_symbols: collected.proc_symbols.clone(),
        variable_symbols: collected.variable_symbols.clone(),
        invoke_resolutions: analyzer.invoke_resolutions,
        ident_resolutions: analyzer.ident_resolutions,
        normalized_invokes: analyzer.normalized_invokes,
    }
}

#[derive(Debug, Default)]
struct ScopeTree {
    parents: Vec<Option<ScopeId>>,
}

impl ScopeTree {
    fn new() -> Self {
        Self {
            parents: vec![None],
        }
    }

    fn root_scope(&self) -> ScopeId {
        ScopeId(0)
    }

    fn new_child(&mut self, parent: ScopeId) -> ScopeId {
        let id = ScopeId(self.parents.len());
        self.parents.push(Some(parent));
        id
    }

    fn parent(&self, scope: ScopeId) -> Option<ScopeId> {
        self.parents[scope.0]
    }
}

struct ScopeCollector {
    scopes: ScopeTree,
    proc_symbols: Vec<ProcSymbol>,
    variable_symbols: Vec<VariableSymbol>,
    local_symbols_by_scope: HashMap<ScopeId, Vec<ProcSymbolId>>,
    global_symbols_by_name: HashMap<String, ProcSymbolId>,
    next_decl_order_by_scope: HashMap<ScopeId, usize>,
    local_variables_by_scope: HashMap<ScopeId, Vec<VariableSymbolId>>,
    global_variables_by_name: HashMap<String, VariableSymbolId>,
    next_variable_decl_order_by_scope: HashMap<ScopeId, usize>,
    scope_by_range: HashMap<TextRange, ScopeId>,
    symbol_by_proc_range: HashMap<TextRange, ProcSymbolId>,
    variable_symbols_by_stmt_range: HashMap<TextRange, Vec<VariableSymbolId>>,
    param_symbols_by_proc_range: HashMap<TextRange, Vec<VariableSymbolId>>,
}

impl ScopeCollector {
    fn collect(source: &SourceFile) -> CollectedScopes {
        let scopes = ScopeTree::new();
        let root_scope = scopes.root_scope();
        let mut collector = Self {
            scopes,
            proc_symbols: Vec::new(),
            variable_symbols: Vec::new(),
            local_symbols_by_scope: HashMap::new(),
            global_symbols_by_name: HashMap::new(),
            next_decl_order_by_scope: HashMap::new(),
            local_variables_by_scope: HashMap::new(),
            global_variables_by_name: HashMap::new(),
            next_variable_decl_order_by_scope: HashMap::new(),
            scope_by_range: HashMap::new(),
            symbol_by_proc_range: HashMap::new(),
            variable_symbols_by_stmt_range: HashMap::new(),
            param_symbols_by_proc_range: HashMap::new(),
        };

        for item in &source.items {
            collector.collect_item(item, root_scope);
        }

        CollectedScopes {
            scopes: collector.scopes,
            proc_symbols: collector.proc_symbols,
            variable_symbols: collector.variable_symbols,
            local_symbols_by_scope: collector.local_symbols_by_scope,
            global_symbols_by_name: collector.global_symbols_by_name,
            local_variables_by_scope: collector.local_variables_by_scope,
            global_variables_by_name: collector.global_variables_by_name,
            scope_by_range: collector.scope_by_range,
            symbol_by_proc_range: collector.symbol_by_proc_range,
            variable_symbols_by_stmt_range: collector.variable_symbols_by_stmt_range,
            param_symbols_by_proc_range: collector.param_symbols_by_proc_range,
            root_scope,
        }
    }

    fn collect_item(&mut self, item: &Item, current_scope: ScopeId) {
        match item {
            Item::Proc(proc_def) => self.collect_proc_def(proc_def, current_scope),
            Item::Stmt(stmt) => self.collect_stmt(stmt, current_scope),
        }
    }

    fn collect_proc_def(&mut self, proc_def: &ProcDef, owner_scope: ScopeId) {
        let decl_order = self.next_decl_order(owner_scope);
        let symbol_id = ProcSymbolId(self.proc_symbols.len());
        self.proc_symbols.push(ProcSymbol {
            id: symbol_id,
            name: proc_def.name.clone(),
            is_global: proc_def.is_global,
            return_type: proc_def.return_type.clone(),
            owner_scope,
            decl_order,
            range: proc_def.range,
        });
        self.symbol_by_proc_range.insert(proc_def.range, symbol_id);

        if proc_def.is_global {
            self.global_symbols_by_name
                .insert(proc_def.name.clone(), symbol_id);
        } else {
            self.local_symbols_by_scope
                .entry(owner_scope)
                .or_default()
                .push(symbol_id);
        }

        let body_scope = self.new_child_scope(owner_scope, stmt_range(&proc_def.body));
        self.collect_proc_params(proc_def, body_scope);
        self.collect_stmt_in_existing_scope(&proc_def.body, body_scope);
    }

    fn collect_stmt(&mut self, stmt: &Stmt, current_scope: ScopeId) {
        match stmt {
            Stmt::Proc { proc_def, .. } => self.collect_proc_def(proc_def, current_scope),
            Stmt::VarDecl { decl, range } => {
                self.collect_var_decl(decl, *range, current_scope);
            }
            Stmt::Block { .. } => self.collect_stmt_in_child_scope(stmt, current_scope),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.collect_stmt_in_child_scope(then_branch, current_scope);
                if let Some(else_branch) = else_branch {
                    self.collect_stmt_in_child_scope(else_branch, current_scope);
                }
            }
            Stmt::While { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::For { body, .. }
            | Stmt::ForIn { body, .. } => self.collect_stmt_in_child_scope(body, current_scope),
            Stmt::Switch { clauses, .. } => {
                for clause in clauses {
                    let clause_scope = self.new_child_scope(current_scope, clause.range);
                    for stmt in &clause.statements {
                        self.collect_stmt(stmt, clause_scope);
                    }
                }
            }
            Stmt::Empty { .. }
            | Stmt::Expr { .. }
            | Stmt::Return { .. }
            | Stmt::Break { .. }
            | Stmt::Continue { .. } => {}
        }
    }

    fn collect_stmt_in_child_scope(&mut self, stmt: &Stmt, parent_scope: ScopeId) {
        let child_scope = self.new_child_scope(parent_scope, stmt_range(stmt));
        self.collect_stmt_in_existing_scope(stmt, child_scope);
    }

    fn collect_stmt_in_existing_scope(&mut self, stmt: &Stmt, current_scope: ScopeId) {
        match stmt {
            Stmt::Block { statements, .. } => {
                for stmt in statements {
                    self.collect_stmt(stmt, current_scope);
                }
            }
            _ => self.collect_stmt(stmt, current_scope),
        }
    }

    fn next_decl_order(&mut self, scope: ScopeId) -> usize {
        let next = self.next_decl_order_by_scope.entry(scope).or_insert(0);
        *next += 1;
        *next
    }

    fn next_variable_decl_order(&mut self, scope: ScopeId) -> usize {
        let next = self
            .next_variable_decl_order_by_scope
            .entry(scope)
            .or_insert(0);
        *next += 1;
        *next
    }

    fn new_child_scope(&mut self, parent_scope: ScopeId, range: TextRange) -> ScopeId {
        let scope = self.scopes.new_child(parent_scope);
        self.scope_by_range.insert(range, scope);
        scope
    }

    fn collect_proc_params(&mut self, proc_def: &ProcDef, body_scope: ScopeId) {
        let mut param_ids = Vec::new();
        for param in &proc_def.params {
            let symbol_id = VariableSymbolId(self.variable_symbols.len());
            self.variable_symbols.push(VariableSymbol {
                id: symbol_id,
                name: param.name.clone(),
                kind: VariableKind::Parameter,
                ty: param.ty.clone(),
                is_array: param.is_array,
                owner_scope: body_scope,
                decl_order: 0,
                range: param.range,
            });
            self.local_variables_by_scope
                .entry(body_scope)
                .or_default()
                .push(symbol_id);
            param_ids.push(symbol_id);
        }

        self.param_symbols_by_proc_range
            .insert(proc_def.range, param_ids);
    }

    fn collect_var_decl(
        &mut self,
        decl: &mel_ast::VarDecl,
        stmt_range: TextRange,
        current_scope: ScopeId,
    ) {
        let mut symbol_ids = Vec::new();

        for declarator in &decl.declarators {
            let (kind, owner_scope, decl_order) = if decl.is_global {
                (VariableKind::Global, self.scopes.root_scope(), 0)
            } else {
                (
                    VariableKind::Local,
                    current_scope,
                    self.next_variable_decl_order(current_scope),
                )
            };

            let symbol_id = VariableSymbolId(self.variable_symbols.len());
            self.variable_symbols.push(VariableSymbol {
                id: symbol_id,
                name: declarator.name.clone(),
                kind,
                ty: decl.ty.clone(),
                is_array: declarator.array_size.is_some(),
                owner_scope,
                decl_order,
                range: declarator.range,
            });

            match kind {
                VariableKind::Global => {
                    self.global_variables_by_name
                        .insert(declarator.name.clone(), symbol_id);
                }
                VariableKind::Local | VariableKind::Parameter => {
                    self.local_variables_by_scope
                        .entry(current_scope)
                        .or_default()
                        .push(symbol_id);
                }
            }

            symbol_ids.push(symbol_id);
        }

        self.variable_symbols_by_stmt_range
            .insert(stmt_range, symbol_ids);
    }
}

struct CollectedScopes {
    scopes: ScopeTree,
    proc_symbols: Vec<ProcSymbol>,
    variable_symbols: Vec<VariableSymbol>,
    local_symbols_by_scope: HashMap<ScopeId, Vec<ProcSymbolId>>,
    global_symbols_by_name: HashMap<String, ProcSymbolId>,
    local_variables_by_scope: HashMap<ScopeId, Vec<VariableSymbolId>>,
    global_variables_by_name: HashMap<String, VariableSymbolId>,
    scope_by_range: HashMap<TextRange, ScopeId>,
    symbol_by_proc_range: HashMap<TextRange, ProcSymbolId>,
    variable_symbols_by_stmt_range: HashMap<TextRange, Vec<VariableSymbolId>>,
    param_symbols_by_proc_range: HashMap<TextRange, Vec<VariableSymbolId>>,
    root_scope: ScopeId,
}

impl CollectedScopes {
    fn scope_for_stmt(&self, stmt: &Stmt) -> ScopeId {
        self.scope_by_range[&stmt_range(stmt)]
    }

    fn scope_for_clause(&self, clause: &SwitchClause) -> ScopeId {
        self.scope_by_range[&clause.range]
    }

    fn symbol_for_proc(&self, proc_def: &ProcDef) -> &ProcSymbol {
        let symbol_id = self.symbol_by_proc_range[&proc_def.range];
        self.symbol(symbol_id)
    }

    fn symbol(&self, id: ProcSymbolId) -> &ProcSymbol {
        &self.proc_symbols[id.0]
    }

    fn variable_symbol(&self, id: VariableSymbolId) -> &VariableSymbol {
        &self.variable_symbols[id.0]
    }

    fn find_visible_local_proc(
        &self,
        name: &str,
        scope: ScopeId,
        visible_decl_orders: &HashMap<ScopeId, usize>,
    ) -> Option<&ProcSymbol> {
        let mut current_scope = Some(scope);
        while let Some(scope_id) = current_scope {
            let visible_order = visible_decl_orders.get(&scope_id).copied().unwrap_or(0);
            let candidate = self
                .local_symbols_by_scope
                .get(&scope_id)
                .and_then(|symbol_ids| {
                    symbol_ids
                        .iter()
                        .filter_map(|symbol_id| {
                            let symbol = self.symbol(*symbol_id);
                            (symbol.name == name && symbol.decl_order <= visible_order)
                                .then_some(symbol)
                        })
                        .max_by_key(|symbol| symbol.decl_order)
                });

            if candidate.is_some() {
                return candidate;
            }

            current_scope = self.scopes.parent(scope_id);
        }

        None
    }

    fn find_forward_local_proc(
        &self,
        name: &str,
        scope: ScopeId,
        visible_decl_orders: &HashMap<ScopeId, usize>,
    ) -> Option<&ProcSymbol> {
        let visible_order = visible_decl_orders.get(&scope).copied().unwrap_or(0);
        self.local_symbols_by_scope
            .get(&scope)
            .and_then(|symbol_ids| {
                symbol_ids
                    .iter()
                    .filter_map(|symbol_id| {
                        let symbol = self.symbol(*symbol_id);
                        (symbol.name == name && symbol.decl_order > visible_order).then_some(symbol)
                    })
                    .min_by_key(|symbol| symbol.decl_order)
            })
    }

    fn find_global_proc(&self, name: &str) -> Option<&ProcSymbol> {
        self.global_symbols_by_name
            .get(name)
            .map(|symbol_id| self.symbol(*symbol_id))
    }

    fn find_resolved_proc_symbol(
        &self,
        name: &str,
        scope: ScopeId,
        visible_decl_orders: &HashMap<ScopeId, usize>,
    ) -> Option<&ProcSymbol> {
        self.find_visible_local_proc(name, scope, visible_decl_orders)
            .or_else(|| self.find_forward_local_proc(name, scope, visible_decl_orders))
            .or_else(|| self.find_global_proc(name))
    }

    fn find_visible_local_variable(
        &self,
        name: &str,
        scope: ScopeId,
        visible_decl_orders: &HashMap<ScopeId, usize>,
    ) -> Option<&VariableSymbol> {
        let mut current_scope = Some(scope);
        while let Some(scope_id) = current_scope {
            let visible_order = visible_decl_orders.get(&scope_id).copied().unwrap_or(0);
            let candidate = self
                .local_variables_by_scope
                .get(&scope_id)
                .and_then(|symbol_ids| {
                    symbol_ids
                        .iter()
                        .filter_map(|symbol_id| {
                            let symbol = self.variable_symbol(*symbol_id);
                            (symbol.name == name && symbol.decl_order <= visible_order)
                                .then_some(symbol)
                        })
                        .max_by_key(|symbol| symbol.decl_order)
                });

            if candidate.is_some() {
                return candidate;
            }

            current_scope = self.scopes.parent(scope_id);
        }

        None
    }

    fn find_global_variable(&self, name: &str) -> Option<&VariableSymbol> {
        self.global_variables_by_name
            .get(name)
            .map(|symbol_id| self.variable_symbol(*symbol_id))
    }

    fn variable_symbols_for_stmt(&self, stmt: &Stmt) -> &[VariableSymbolId] {
        self.variable_symbols_by_stmt_range
            .get(&stmt_range(stmt))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn param_symbols_for_proc(&self, proc_def: &ProcDef) -> &[VariableSymbolId] {
        self.param_symbols_by_proc_range
            .get(&proc_def.range)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

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

struct FlowLintAnalyzer<'a> {
    collected: &'a CollectedScopes,
    diagnostics: Vec<Diagnostic>,
    visible_variable_decl_orders: HashMap<ScopeId, usize>,
    emitted_warnings: HashSet<(TextRange, String)>,
}

impl<'a> FlowLintAnalyzer<'a> {
    fn new(collected: &'a CollectedScopes) -> Self {
        Self {
            collected,
            diagnostics: Vec::new(),
            visible_variable_decl_orders: HashMap::new(),
            emitted_warnings: HashSet::new(),
        }
    }

    fn walk_source(&mut self, source: &SourceFile) {
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

struct Analyzer<'a, R: ?Sized> {
    collected: &'a CollectedScopes,
    registry: &'a R,
    diagnostics: Vec<Diagnostic>,
    invoke_resolutions: Vec<InvokeResolution>,
    ident_resolutions: Vec<IdentResolution>,
    normalized_invokes: Vec<NormalizedCommandInvoke>,
    visible_decl_orders: HashMap<ScopeId, usize>,
    visible_variable_decl_orders: HashMap<ScopeId, usize>,
    implicit_variables_by_scope: HashMap<ScopeId, Vec<String>>,
    proc_contexts: Vec<ProcContext>,
}

struct ProcContext {
    name: String,
    range: TextRange,
    return_type: Option<ValueType>,
    saw_value_return: bool,
}

enum ResolvedInvokeTarget {
    Proc(String),
    Command(CommandSchema),
    Unresolved,
}

impl ResolvedInvokeTarget {
    fn into_callee_resolution(self) -> CalleeResolution {
        match self {
            Self::Proc(name) => CalleeResolution::Proc(name),
            Self::Command(command) => match command.kind {
                CommandKind::Builtin => CalleeResolution::BuiltinCommand(command.name.clone()),
                CommandKind::Plugin => CalleeResolution::PluginCommand(command.name.clone()),
            },
            Self::Unresolved => CalleeResolution::Unresolved,
        }
    }
}

impl<'a, R> Analyzer<'a, R>
where
    R: CommandRegistry + ?Sized,
{
    fn new(collected: &'a CollectedScopes, registry: &'a R) -> Self {
        Self {
            collected,
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

    fn walk_item(&mut self, item: &Item, current_scope: ScopeId) {
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
            name: proc_def.name.clone(),
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
                self.walk_expr(binding, current_scope);
                self.walk_expr(iterable, current_scope);
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
            Expr::Ident { name, range } => {
                let resolution = self.resolve_ident(name, current_scope);
                if matches!(resolution, IdentTarget::Unresolved)
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
                    name: name.clone(),
                    resolution,
                });
            }
            Expr::BareWord { .. } | Expr::Int { .. } | Expr::Float { .. } | Expr::String { .. } => {
            }
        }
    }

    fn walk_assign_target(&mut self, expr: &Expr, current_scope: ScopeId, emit_unresolved: bool) {
        match expr {
            Expr::Ident { name, range } => {
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
                    self.mark_implicit_variable(name, current_scope);
                }
                self.ident_resolutions.push(IdentResolution {
                    range: *range,
                    scope: current_scope,
                    name: name.clone(),
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
            mel_ast::InvokeSurface::Function { name, args } => {
                for arg in args {
                    self.walk_expr(arg, current_scope);
                }
                self.resolve_invoke(name, invoke.range, current_scope)
            }
            mel_ast::InvokeSurface::ShellLike { head, words, .. } => {
                for word in words {
                    self.walk_shell_word(word, current_scope);
                }
                let resolved = self.resolve_named_target(head, invoke.range, current_scope);
                if let ResolvedInvokeTarget::Command(ref command) = resolved {
                    let (normalized, diagnostics) = command_norm::normalize_shell_like_invoke(
                        command,
                        current_scope,
                        head,
                        words,
                        invoke.range,
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

    fn resolve_invoke(
        &mut self,
        name: &str,
        range: TextRange,
        current_scope: ScopeId,
    ) -> CalleeResolution {
        self.resolve_named_target(name, range, current_scope)
            .into_callee_resolution()
    }

    fn resolve_named_target(
        &mut self,
        name: &str,
        range: TextRange,
        current_scope: ScopeId,
    ) -> ResolvedInvokeTarget {
        if let Some(symbol) =
            self.collected
                .find_visible_local_proc(name, current_scope, &self.visible_decl_orders)
        {
            return ResolvedInvokeTarget::Proc(symbol.name.clone());
        }

        if let Some(symbol) =
            self.collected
                .find_forward_local_proc(name, current_scope, &self.visible_decl_orders)
        {
            self.diagnostics.push(Diagnostic::error(
                format!("local proc \"{name}\" is called before its definition"),
                range,
            ));
            return ResolvedInvokeTarget::Proc(symbol.name.clone());
        }

        if let Some(symbol) = self.collected.find_global_proc(name) {
            return ResolvedInvokeTarget::Proc(symbol.name.clone());
        }

        if let Some(command) = self.registry.lookup(name) {
            return ResolvedInvokeTarget::Command(command);
        }

        ResolvedInvokeTarget::Unresolved
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
            name,
            current_scope,
            &self.visible_variable_decl_orders,
        ) {
            return IdentTarget::Variable(symbol.id);
        }

        if let Some(symbol) = self.collected.find_global_variable(name) {
            return IdentTarget::Variable(symbol.id);
        }

        IdentTarget::Unresolved
    }

    fn mark_implicit_variable(&mut self, name: &str, current_scope: ScopeId) {
        let names = self
            .implicit_variables_by_scope
            .entry(current_scope)
            .or_default();
        if !names.iter().any(|candidate| candidate == name) {
            names.push(name.to_owned());
        }
    }

    fn is_visible_implicit_variable(&self, name: &str, current_scope: ScopeId) -> bool {
        let mut scope = Some(current_scope);
        while let Some(scope_id) = scope {
            if self
                .implicit_variables_by_scope
                .get(&scope_id)
                .is_some_and(|names| names.iter().any(|candidate| candidate == name))
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

        match (&context.return_type, actual.as_ref()) {
            (None, Some(_)) => self.diagnostics.push(Diagnostic::error(
                format!(
                    "proc \"{}\" has no return type but returns a value",
                    context.name
                ),
                range,
            )),
            (Some(expected), Some(actual)) => {
                context.saw_value_return = true;
                if !is_assignable(expected, actual) {
                    self.diagnostics.push(Diagnostic::error(
                        format!(
                            "proc \"{}\" returns {:?} but declares {:?}",
                            context.name, actual, expected
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
                    context.name
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
                    declarator.name, expected, actual
                ),
                declarator.range,
            ));
        }
    }

    fn infer_expr_type(&self, expr: &Expr, current_scope: ScopeId) -> ValueType {
        match expr {
            Expr::Ident { name, .. } => self.infer_ident_type(name, current_scope),
            Expr::Int { .. } => ValueType::Int,
            Expr::Float { .. } => ValueType::Float,
            Expr::String { .. } | Expr::BareWord { .. } => ValueType::String,
            Expr::Cast { ty, .. } => value_type_from_type_name(ty),
            Expr::VectorLiteral { .. } => ValueType::Vector,
            Expr::ArrayLiteral { elements, .. } => {
                infer_array_literal_type(elements, self, current_scope)
            }
            Expr::Unary { expr, .. }
            | Expr::PrefixUpdate { expr, .. }
            | Expr::PostfixUpdate { expr, .. }
            | Expr::ComponentAccess { target: expr, .. } => {
                self.infer_expr_type(expr, current_scope)
            }
            Expr::MemberAccess { target, member, .. } => {
                if matches!(
                    self.infer_expr_type(target, current_scope),
                    ValueType::Vector
                ) && matches!(member.as_str(), "x" | "y" | "z")
                {
                    ValueType::Float
                } else {
                    ValueType::Unknown
                }
            }
            Expr::Binary { lhs, rhs, .. } | Expr::Assign { lhs, rhs, .. } => combine_numeric_types(
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

    fn infer_ident_type(&self, name: &str, current_scope: ScopeId) -> ValueType {
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

    fn infer_invoke_type(&self, invoke: &mel_ast::InvokeExpr, current_scope: ScopeId) -> ValueType {
        let name = match &invoke.surface {
            mel_ast::InvokeSurface::Function { name, .. }
            | mel_ast::InvokeSurface::ShellLike { head: name, .. } => name,
        };

        let Some(symbol) = self.collected.find_resolved_proc_symbol(
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
}

fn stmt_range(stmt: &Stmt) -> TextRange {
    match stmt {
        Stmt::Empty { range }
        | Stmt::Proc { range, .. }
        | Stmt::Block { range, .. }
        | Stmt::Expr { range, .. }
        | Stmt::VarDecl { range, .. }
        | Stmt::If { range, .. }
        | Stmt::While { range, .. }
        | Stmt::DoWhile { range, .. }
        | Stmt::Switch { range, .. }
        | Stmt::For { range, .. }
        | Stmt::ForIn { range, .. }
        | Stmt::Return { range, .. }
        | Stmt::Break { range }
        | Stmt::Continue { range } => *range,
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

#[cfg(test)]
mod tests {
    use super::{
        CommandKind, CommandMode, CommandModeMask, CommandRegistry, CommandSchema,
        CommandSourceKind, DiagnosticSeverity, EmbeddedCommandRegistry, FlagArity, FlagArityByMode,
        FlagSchema, IdentTarget, ReturnBehavior, SpecializedCommandForm, ValueShape, VariableKind,
        analyze, analyze_with_registry,
    };
    use mel_ast::{
        AssignOp, CalleeResolution, Declarator, Expr, InvokeExpr, InvokeSurface, Item, ProcDef,
        ProcParam, ShellWord, SourceFile, Stmt, TypeName, VarDecl,
    };
    use mel_syntax::text_range;

    struct TestRegistry {
        commands: Vec<CommandSchema>,
    }

    impl CommandRegistry for TestRegistry {
        fn lookup(&self, name: &str) -> Option<CommandSchema> {
            self.commands.iter().find(|info| info.name == name).cloned()
        }
    }

    fn command_schema(name: &str, kind: CommandKind) -> CommandSchema {
        CommandSchema {
            name: name.to_owned(),
            kind,
            source_kind: CommandSourceKind::Command,
            mode_mask: CommandModeMask {
                create: true,
                edit: true,
                query: true,
            },
            return_behavior: ReturnBehavior::Unknown,
            flags: Vec::new(),
        }
    }

    fn uniform_arity(arity: FlagArity) -> FlagArityByMode {
        FlagArityByMode {
            create: arity,
            edit: arity,
            query: arity,
        }
    }

    fn flag_schema(long_name: &str, short_name: Option<&str>, arity: FlagArity) -> FlagSchema {
        FlagSchema {
            long_name: long_name.to_owned(),
            short_name: short_name.map(str::to_owned),
            mode_mask: CommandModeMask {
                create: true,
                edit: true,
                query: true,
            },
            arity_by_mode: uniform_arity(arity),
            value_shapes: vec![ValueShape::Unknown],
            allows_multiple: false,
        }
    }

    fn resolved_variable(
        analysis: &super::Analysis,
        index: usize,
    ) -> Option<&super::VariableSymbol> {
        match analysis.ident_resolutions[index].resolution {
            IdentTarget::Unresolved => None,
            IdentTarget::Variable(symbol_id) => Some(&analysis.variable_symbols[symbol_id.0]),
        }
    }

    fn warning_messages(analysis: &super::Analysis) -> Vec<&str> {
        analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            .map(|diagnostic| diagnostic.message.as_str())
            .collect()
    }

    #[test]
    fn function_local_proc_forward_reference_reports_diagnostic() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Invoke(InvokeExpr {
                        surface: InvokeSurface::Function {
                            name: "helper".to_owned(),
                            args: Vec::new(),
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
                        range: text_range(17, 19),
                    },
                    is_global: false,
                    range: text_range(10, 19),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].message,
            "local proc \"helper\" is called before its definition"
        );
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::Proc("helper".to_owned())
        );
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
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::Proc("helper".to_owned())
        );
    }

    #[test]
    fn ancestor_scope_local_proc_is_visible_in_nested_block() {
        let source = SourceFile {
            items: vec![
                Item::Proc(Box::new(ProcDef {
                    return_type: None,
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: Vec::new(),
                        range: text_range(12, 14),
                    },
                    is_global: false,
                    range: text_range(0, 14),
                })),
                Item::Stmt(Box::new(Stmt::Block {
                    statements: vec![Stmt::Expr {
                        expr: Expr::Invoke(InvokeExpr {
                            surface: InvokeSurface::Function {
                                name: "helper".to_owned(),
                                args: Vec::new(),
                            },
                            resolution: CalleeResolution::Unresolved,
                            range: text_range(17, 25),
                        }),
                        range: text_range(17, 26),
                    }],
                    range: text_range(15, 27),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty());
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::Proc("helper".to_owned())
        );
    }

    #[test]
    fn block_local_proc_does_not_leak_to_parent_scope() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::Block {
                    statements: vec![Stmt::Proc {
                        proc_def: Box::new(ProcDef {
                            return_type: None,
                            name: "helper".to_owned(),
                            params: Vec::new(),
                            body: Stmt::Block {
                                statements: Vec::new(),
                                range: text_range(17, 19),
                            },
                            is_global: false,
                            range: text_range(8, 19),
                        }),
                        range: text_range(8, 19),
                    }],
                    range: text_range(0, 20),
                })),
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Invoke(InvokeExpr {
                        surface: InvokeSurface::Function {
                            name: "helper".to_owned(),
                            args: Vec::new(),
                        },
                        resolution: CalleeResolution::Unresolved,
                        range: text_range(21, 29),
                    }),
                    range: text_range(21, 30),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty());
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::Unresolved
        );
    }

    #[test]
    fn shell_like_calls_resolve_to_local_proc_without_diagnostic() {
        let source = SourceFile {
            items: vec![
                Item::Proc(Box::new(ProcDef {
                    return_type: None,
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: Vec::new(),
                        range: text_range(9, 11),
                    },
                    is_global: false,
                    range: text_range(0, 11),
                })),
                Item::Stmt(Box::new(Stmt::VarDecl {
                    decl: VarDecl {
                        is_global: false,
                        ty: TypeName::String,
                        declarators: vec![Declarator {
                            name: "$selection".to_owned(),
                            array_size: None,
                            initializer: Some(Expr::String {
                                text: "\"pSphere1\"".to_owned(),
                                range: text_range(19, 29),
                            }),
                            range: text_range(12, 29),
                        }],
                        range: text_range(12, 30),
                    },
                    range: text_range(12, 30),
                })),
                Item::Stmt(Box::new(Stmt::VarDecl {
                    decl: VarDecl {
                        is_global: false,
                        ty: TypeName::String,
                        declarators: vec![Declarator {
                            name: "$value".to_owned(),
                            array_size: None,
                            initializer: None,
                            range: text_range(31, 37),
                        }],
                        range: text_range(31, 38),
                    },
                    range: text_range(31, 38),
                })),
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Assign {
                        op: AssignOp::Assign,
                        lhs: Box::new(Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(39, 45),
                        }),
                        rhs: Box::new(Expr::Invoke(InvokeExpr {
                            surface: InvokeSurface::ShellLike {
                                head: "helper".to_owned(),
                                words: vec![ShellWord::Variable {
                                    expr: Expr::Ident {
                                        name: "$selection".to_owned(),
                                        range: text_range(50, 60),
                                    },
                                    range: text_range(50, 60),
                                }],
                                captured: true,
                            },
                            resolution: CalleeResolution::Unresolved,
                            range: text_range(46, 61),
                        })),
                        range: text_range(39, 61),
                    },
                    range: text_range(39, 62),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty());
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::Proc("helper".to_owned())
        );
    }

    #[test]
    fn shell_like_calls_without_proc_or_registry_remain_unresolved() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "unknown".to_owned(),
                        words: Vec::new(),
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 7),
                }),
                range: text_range(0, 8),
            }))],
        };

        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty());
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::Unresolved
        );
    }

    #[test]
    fn shell_like_local_proc_forward_reference_reports_diagnostic() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Invoke(InvokeExpr {
                        surface: InvokeSurface::ShellLike {
                            head: "helper".to_owned(),
                            words: vec![ShellWord::NumericLiteral {
                                text: "7".to_owned(),
                                range: text_range(7, 8),
                            }],
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
                        range: text_range(17, 19),
                    },
                    is_global: false,
                    range: text_range(10, 19),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].message,
            "local proc \"helper\" is called before its definition"
        );
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::Proc("helper".to_owned())
        );
    }

    #[test]
    fn shell_like_global_proc_resolves_without_diagnostic() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Invoke(InvokeExpr {
                        surface: InvokeSurface::ShellLike {
                            head: "helper".to_owned(),
                            words: vec![ShellWord::QuotedString {
                                text: "\"value\"".to_owned(),
                                range: text_range(7, 14),
                            }],
                            captured: false,
                        },
                        resolution: CalleeResolution::Unresolved,
                        range: text_range(0, 14),
                    }),
                    range: text_range(0, 15),
                })),
                Item::Proc(Box::new(ProcDef {
                    return_type: None,
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: Vec::new(),
                        range: text_range(23, 25),
                    },
                    is_global: true,
                    range: text_range(16, 25),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty());
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::Proc("helper".to_owned())
        );
    }

    #[test]
    fn builtin_command_resolves_with_registry() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::Function {
                        name: "sphere".to_owned(),
                        args: Vec::new(),
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 8),
                }),
                range: text_range(0, 9),
            }))],
        };

        let registry = TestRegistry {
            commands: vec![command_schema("sphere", CommandKind::Builtin)],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert!(analysis.diagnostics.is_empty());
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::BuiltinCommand("sphere".to_owned())
        );
    }

    #[test]
    fn plugin_command_resolves_with_registry() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "foo".to_owned(),
                        words: Vec::new(),
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 3),
                }),
                range: text_range(0, 4),
            }))],
        };

        let registry = TestRegistry {
            commands: vec![command_schema("foo", CommandKind::Plugin)],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert!(analysis.diagnostics.is_empty());
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::PluginCommand("foo".to_owned())
        );
    }

    #[test]
    fn proc_resolution_takes_precedence_over_registry_command() {
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
                        range: text_range(0, 6),
                    }),
                    range: text_range(0, 7),
                })),
                Item::Proc(Box::new(ProcDef {
                    return_type: None,
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: Vec::new(),
                        range: text_range(15, 17),
                    },
                    is_global: true,
                    range: text_range(8, 17),
                })),
            ],
        };

        let registry = TestRegistry {
            commands: vec![command_schema("helper", CommandKind::Builtin)],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert!(analysis.diagnostics.is_empty());
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::Proc("helper".to_owned())
        );
    }

    #[test]
    fn analyze_uses_embedded_catalog_by_default() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::Function {
                        name: "sphere".to_owned(),
                        args: Vec::new(),
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 8),
                }),
                range: text_range(0, 9),
            }))],
        };

        let analysis = analyze(&source);
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::BuiltinCommand("sphere".to_owned())
        );
    }

    #[test]
    fn embedded_catalog_keeps_script_source_kind() {
        let schema = EmbeddedCommandRegistry::new()
            .lookup("addNewShelfTab")
            .expect("embedded schema for addNewShelfTab");
        assert_eq!(schema.kind, CommandKind::Builtin);
        assert_eq!(schema.source_kind, CommandSourceKind::Script);
    }

    #[test]
    fn embedded_catalog_synthesizes_mode_flags_from_command_mode_mask() {
        let schema = EmbeddedCommandRegistry::new()
            .lookup("addAttr")
            .expect("embedded schema for addAttr");
        assert!(
            schema
                .flags
                .iter()
                .any(|flag| flag.long_name == "create" && flag.short_name.as_deref() == Some("c"))
        );
        assert!(
            schema
                .flags
                .iter()
                .any(|flag| flag.long_name == "edit" && flag.short_name.as_deref() == Some("e"))
        );
        assert!(
            schema
                .flags
                .iter()
                .any(|flag| flag.long_name == "query" && flag.short_name.as_deref() == Some("q"))
        );
    }

    #[test]
    fn shell_like_command_normalization_tracks_query_mode_and_invalid_flag_usage() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "frameLayout".to_owned(),
                        words: vec![
                            ShellWord::Flag {
                                text: "-query".to_owned(),
                                range: text_range(12, 18),
                            },
                            ShellWord::Flag {
                                text: "-label".to_owned(),
                                range: text_range(19, 25),
                            },
                            ShellWord::QuotedString {
                                text: "\"title\"".to_owned(),
                                range: text_range(26, 33),
                            },
                        ],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 33),
                }),
                range: text_range(0, 34),
            }))],
        };

        let mut command = command_schema("frameLayout", CommandKind::Builtin);
        command.mode_mask = CommandModeMask {
            create: true,
            edit: true,
            query: true,
        };
        command.flags = vec![FlagSchema {
            mode_mask: CommandModeMask {
                create: false,
                edit: true,
                query: false,
            },
            value_shapes: vec![ValueShape::String],
            ..flag_schema("label", Some("l"), FlagArity::Exact(1))
        }];
        let registry = TestRegistry {
            commands: vec![command],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert_eq!(
            analysis.invoke_resolutions[0].resolution,
            CalleeResolution::BuiltinCommand("frameLayout".to_owned())
        );
        assert_eq!(analysis.normalized_invokes.len(), 1);
        assert_eq!(analysis.normalized_invokes[0].mode, CommandMode::Query);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.message.contains("not available in query mode")
        }));
    }

    #[test]
    fn shell_like_command_unknown_flag_is_warning() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "frameLayout".to_owned(),
                        words: vec![ShellWord::Flag {
                            text: "-mystery".to_owned(),
                            range: text_range(12, 20),
                        }],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 20),
                }),
                range: text_range(0, 21),
            }))],
        };

        let registry = TestRegistry {
            commands: vec![command_schema("frameLayout", CommandKind::Builtin)],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.message.contains("unknown flag")
        }));
    }

    #[test]
    fn shell_like_command_normalization_reports_mode_conflict() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "frameLayout".to_owned(),
                        words: vec![
                            ShellWord::Flag {
                                text: "-edit".to_owned(),
                                range: text_range(12, 17),
                            },
                            ShellWord::Flag {
                                text: "-query".to_owned(),
                                range: text_range(18, 24),
                            },
                        ],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 24),
                }),
                range: text_range(0, 25),
            }))],
        };

        let mut command = command_schema("frameLayout", CommandKind::Builtin);
        command.mode_mask = CommandModeMask {
            create: true,
            edit: true,
            query: true,
        };
        let registry = TestRegistry {
            commands: vec![command],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert_eq!(analysis.normalized_invokes[0].mode, CommandMode::Unknown);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("combine create/edit/query mode flags")
        }));
    }

    #[test]
    fn shell_like_command_query_mode_uses_query_specific_flag_arity() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "frameLayout".to_owned(),
                        words: vec![
                            ShellWord::Flag {
                                text: "-query".to_owned(),
                                range: text_range(12, 18),
                            },
                            ShellWord::Flag {
                                text: "-label".to_owned(),
                                range: text_range(19, 25),
                            },
                            ShellWord::QuotedString {
                                text: "\"title\"".to_owned(),
                                range: text_range(26, 33),
                            },
                        ],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 33),
                }),
                range: text_range(0, 34),
            }))],
        };

        let mut command = command_schema("frameLayout", CommandKind::Builtin);
        command.flags = vec![FlagSchema {
            arity_by_mode: FlagArityByMode {
                create: FlagArity::Exact(1),
                edit: FlagArity::Exact(1),
                query: FlagArity::None,
            },
            value_shapes: vec![ValueShape::String],
            ..flag_schema("label", Some("l"), FlagArity::Exact(1))
        }];
        let registry = TestRegistry {
            commands: vec![command],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert!(analysis.diagnostics.is_empty());
        let items = &analysis.normalized_invokes[0].items;
        assert!(matches!(
            &items[1],
            super::NormalizedCommandItem::Flag(super::NormalizedFlag {
                canonical_name: Some(name),
                args,
                ..
            }) if name == "label" && args.is_empty()
        ));
        assert!(matches!(
            &items[2],
            super::NormalizedCommandItem::Positional(super::PositionalArg {
                word: ShellWord::QuotedString { text, .. },
                ..
            }) if text == "\"title\""
        ));
    }

    #[test]
    fn shell_like_command_range_arity_allows_optional_second_arg_to_be_omitted() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "frameLayout".to_owned(),
                        words: vec![
                            ShellWord::Flag {
                                text: "-label".to_owned(),
                                range: text_range(12, 18),
                            },
                            ShellWord::QuotedString {
                                text: "\"title\"".to_owned(),
                                range: text_range(19, 26),
                            },
                        ],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 26),
                }),
                range: text_range(0, 27),
            }))],
        };

        let mut command = command_schema("frameLayout", CommandKind::Builtin);
        command.flags = vec![FlagSchema {
            arity_by_mode: FlagArityByMode {
                create: FlagArity::Range { min: 1, max: 2 },
                edit: FlagArity::Range { min: 1, max: 2 },
                query: FlagArity::None,
            },
            value_shapes: vec![ValueShape::String, ValueShape::String],
            ..flag_schema("label", Some("l"), FlagArity::Exact(1))
        }];
        let registry = TestRegistry {
            commands: vec![command],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert!(analysis.diagnostics.is_empty());
        let items = &analysis.normalized_invokes[0].items;
        assert!(matches!(
            &items[0],
            super::NormalizedCommandItem::Flag(super::NormalizedFlag {
                canonical_name: Some(name),
                args,
                ..
            }) if name == "label" && args.len() == 1
        ));
    }

    #[test]
    fn shell_like_command_range_arity_allows_optional_second_arg_to_be_present() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "frameLayout".to_owned(),
                        words: vec![
                            ShellWord::Flag {
                                text: "-label".to_owned(),
                                range: text_range(12, 18),
                            },
                            ShellWord::QuotedString {
                                text: "\"title\"".to_owned(),
                                range: text_range(19, 26),
                            },
                            ShellWord::QuotedString {
                                text: "\"tooltip\"".to_owned(),
                                range: text_range(27, 36),
                            },
                        ],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 36),
                }),
                range: text_range(0, 37),
            }))],
        };

        let mut command = command_schema("frameLayout", CommandKind::Builtin);
        command.flags = vec![FlagSchema {
            arity_by_mode: FlagArityByMode {
                create: FlagArity::Range { min: 1, max: 2 },
                edit: FlagArity::Range { min: 1, max: 2 },
                query: FlagArity::None,
            },
            value_shapes: vec![ValueShape::String, ValueShape::String],
            ..flag_schema("label", Some("l"), FlagArity::Exact(1))
        }];
        let registry = TestRegistry {
            commands: vec![command],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert!(analysis.diagnostics.is_empty());
        let items = &analysis.normalized_invokes[0].items;
        assert!(matches!(
            &items[0],
            super::NormalizedCommandItem::Flag(super::NormalizedFlag {
                canonical_name: Some(name),
                args,
                ..
            }) if name == "label" && args.len() == 2
        ));
    }

    #[test]
    fn shell_like_command_range_arity_reports_missing_required_argument() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "frameLayout".to_owned(),
                        words: vec![ShellWord::Flag {
                            text: "-label".to_owned(),
                            range: text_range(12, 18),
                        }],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 18),
                }),
                range: text_range(0, 19),
            }))],
        };

        let mut command = command_schema("frameLayout", CommandKind::Builtin);
        command.flags = vec![FlagSchema {
            arity_by_mode: FlagArityByMode {
                create: FlagArity::Range { min: 1, max: 2 },
                edit: FlagArity::Range { min: 1, max: 2 },
                query: FlagArity::None,
            },
            value_shapes: vec![ValueShape::String, ValueShape::String],
            ..flag_schema("label", Some("l"), FlagArity::Exact(1))
        }];
        let registry = TestRegistry {
            commands: vec![command],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error
                && diagnostic.message.contains("expects 1 to 2 argument(s)")
        }));
    }

    #[test]
    fn set_attr_data_reference_edits_tail_is_preserved_losslessly() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "setAttr".to_owned(),
                        words: vec![
                            ShellWord::QuotedString {
                                text: "\".ed\"".to_owned(),
                                range: text_range(8, 13),
                            },
                            ShellWord::Flag {
                                text: "-type".to_owned(),
                                range: text_range(14, 19),
                            },
                            ShellWord::QuotedString {
                                text: "\"dataReferenceEdits\"".to_owned(),
                                range: text_range(20, 40),
                            },
                            ShellWord::QuotedString {
                                text: "\"rootRN\"".to_owned(),
                                range: text_range(41, 49),
                            },
                            ShellWord::QuotedString {
                                text: "\"\"".to_owned(),
                                range: text_range(50, 52),
                            },
                            ShellWord::NumericLiteral {
                                text: "5".to_owned(),
                                range: text_range(53, 54),
                            },
                            ShellWord::QuotedString {
                                text: "\"node.placeHolderList[0]\"".to_owned(),
                                range: text_range(55, 81),
                            },
                            ShellWord::BareWord {
                                text: "|world|ctrl".to_owned(),
                                range: text_range(82, 93),
                            },
                            ShellWord::QuotedString {
                                text: "\" -type \\\"double3\\\" 0 0 1\"".to_owned(),
                                range: text_range(94, 120),
                            },
                            ShellWord::QuotedString {
                                text: "\" -av\"".to_owned(),
                                range: text_range(121, 128),
                            },
                        ],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 128),
                }),
                range: text_range(0, 129),
            }))],
        };

        let mut command = command_schema("setAttr", CommandKind::Builtin);
        command.flags = vec![flag_schema("type", Some("typ"), FlagArity::Exact(1))];
        let registry = TestRegistry {
            commands: vec![command],
        };

        let analysis = analyze_with_registry(&source, &registry);
        let Some(SpecializedCommandForm::SetAttrDataReferenceEdits(payload)) =
            analysis.normalized_invokes[0].special_form.as_ref()
        else {
            panic!("expected dataReferenceEdits special form");
        };

        assert_eq!(payload.command_head, "setAttr");
        assert!(matches!(
            payload.attr_path.word,
            ShellWord::QuotedString { ref text, .. } if text == "\".ed\""
        ));
        assert_eq!(payload.type_flag_text, "-type");
        assert!(matches!(
            payload.type_name.word,
            ShellWord::QuotedString { ref text, .. } if text == "\"dataReferenceEdits\""
        ));
        assert_eq!(payload.raw_tail_items.len(), 7);
        assert!(matches!(
            payload.raw_tail_items[0].word,
            ShellWord::QuotedString { ref text, .. } if text == "\"rootRN\""
        ));
        assert!(matches!(
            payload.raw_tail_items[1].word,
            ShellWord::QuotedString { ref text, .. } if text == "\"\""
        ));
        assert!(matches!(
            payload.raw_tail_items[2].word,
            ShellWord::NumericLiteral { ref text, .. } if text == "5"
        ));
        assert!(matches!(
            payload.raw_tail_items[3].word,
            ShellWord::QuotedString { ref text, .. } if text.contains("placeHolderList[0]")
        ));
        assert!(matches!(
            payload.raw_tail_items[4].word,
            ShellWord::BareWord { ref text, .. } if text == "|world|ctrl"
        ));
        assert!(matches!(
            payload.raw_tail_items[5].word,
            ShellWord::QuotedString { ref text, .. } if text == "\" -type \\\"double3\\\" 0 0 1\""
        ));
        assert!(matches!(
            payload.raw_tail_items[6].word,
            ShellWord::QuotedString { ref text, .. } if text == "\" -av\""
        ));
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn shell_like_command_without_mode_flag_reports_unavailable_create_mode() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "queryOnly".to_owned(),
                        words: vec![ShellWord::BareWord {
                            text: "node1".to_owned(),
                            range: text_range(10, 15),
                        }],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 15),
                }),
                range: text_range(0, 16),
            }))],
        };

        let mut command = command_schema("queryOnly", CommandKind::Builtin);
        command.mode_mask = CommandModeMask {
            create: false,
            edit: false,
            query: true,
        };
        let registry = TestRegistry {
            commands: vec![command],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert_eq!(analysis.normalized_invokes[0].mode, CommandMode::Create);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("command \"queryOnly\" is not available in create mode")
        }));
    }

    #[test]
    fn proc_params_resolve_inside_proc_body() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: vec![ProcParam {
                    ty: TypeName::String,
                    name: "$name".to_owned(),
                    is_array: false,
                    range: text_range(12, 24),
                }],
                body: Stmt::Block {
                    statements: vec![Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$name".to_owned(),
                            range: text_range(29, 34),
                        },
                        range: text_range(29, 35),
                    }],
                    range: text_range(26, 36),
                },
                is_global: false,
                range: text_range(0, 36),
            }))],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.variable_symbols.len(), 1);
        assert_eq!(analysis.variable_symbols[0].kind, VariableKind::Parameter);
        assert_eq!(
            resolved_variable(&analysis, 0).map(|symbol| symbol.name.as_str()),
            Some("$name")
        );
    }

    #[test]
    fn local_variables_become_visible_after_declaration() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Ident {
                        name: "$value".to_owned(),
                        range: text_range(0, 6),
                    },
                    range: text_range(0, 7),
                })),
                Item::Stmt(Box::new(Stmt::VarDecl {
                    decl: VarDecl {
                        is_global: false,
                        ty: TypeName::Int,
                        declarators: vec![Declarator {
                            name: "$value".to_owned(),
                            array_size: None,
                            initializer: Some(Expr::Int {
                                value: 1,
                                range: text_range(18, 19),
                            }),
                            range: text_range(12, 19),
                        }],
                        range: text_range(8, 20),
                    },
                    range: text_range(8, 20),
                })),
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Ident {
                        name: "$value".to_owned(),
                        range: text_range(21, 27),
                    },
                    range: text_range(21, 28),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.variable_symbols.len(), 1);
        assert_eq!(analysis.ident_resolutions.len(), 2);
        assert!(resolved_variable(&analysis, 0).is_none());
        assert_eq!(
            resolved_variable(&analysis, 1).map(|symbol| symbol.name.as_str()),
            Some("$value")
        );
    }

    #[test]
    fn local_variables_shadow_globals_inside_proc_scope() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::VarDecl {
                    decl: VarDecl {
                        is_global: true,
                        ty: TypeName::String,
                        declarators: vec![Declarator {
                            name: "$value".to_owned(),
                            array_size: None,
                            initializer: None,
                            range: text_range(0, 20),
                        }],
                        range: text_range(0, 21),
                    },
                    range: text_range(0, 21),
                })),
                Item::Proc(Box::new(ProcDef {
                    return_type: None,
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: vec![
                            Stmt::VarDecl {
                                decl: VarDecl {
                                    is_global: false,
                                    ty: TypeName::Int,
                                    declarators: vec![Declarator {
                                        name: "$value".to_owned(),
                                        array_size: None,
                                        initializer: Some(Expr::Int {
                                            value: 1,
                                            range: text_range(42, 43),
                                        }),
                                        range: text_range(36, 43),
                                    }],
                                    range: text_range(32, 44),
                                },
                                range: text_range(32, 44),
                            },
                            Stmt::Expr {
                                expr: Expr::Ident {
                                    name: "$value".to_owned(),
                                    range: text_range(45, 51),
                                },
                                range: text_range(45, 52),
                            },
                        ],
                        range: text_range(30, 53),
                    },
                    is_global: false,
                    range: text_range(22, 53),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.variable_symbols.len(), 2);
        let resolved = resolved_variable(&analysis, 0).expect("local variable should resolve");
        assert_eq!(resolved.kind, VariableKind::Local);
        assert_eq!(resolved.name, "$value");
    }

    #[test]
    fn block_local_variable_does_not_leak_to_parent_scope() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$value".to_owned(),
                                    array_size: None,
                                    initializer: Some(Expr::Int {
                                        value: 1,
                                        range: text_range(13, 14),
                                    }),
                                    range: text_range(7, 14),
                                }],
                                range: text_range(3, 15),
                            },
                            range: text_range(3, 15),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$value".to_owned(),
                                range: text_range(16, 22),
                            },
                            range: text_range(16, 23),
                        },
                    ],
                    range: text_range(0, 24),
                })),
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Ident {
                        name: "$value".to_owned(),
                        range: text_range(25, 31),
                    },
                    range: text_range(25, 32),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert_eq!(
            resolved_variable(&analysis, 0).map(|symbol| symbol.kind),
            Some(VariableKind::Local)
        );
        assert!(resolved_variable(&analysis, 1).is_none());
    }

    #[test]
    fn local_variable_read_before_first_explicit_write_is_warning() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$value".to_owned(),
                                    array_size: None,
                                    initializer: None,
                                    range: text_range(17, 23),
                                }],
                                range: text_range(13, 24),
                            },
                            range: text_range(13, 24),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$value".to_owned(),
                                range: text_range(25, 31),
                            },
                            range: text_range(25, 32),
                        },
                    ],
                    range: text_range(12, 33),
                },
                is_global: false,
                range: text_range(0, 33),
            }))],
        };

        let analysis = analyze(&source);
        let warnings = warning_messages(&analysis);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0],
            "local variable \"$value\" is read before its first explicit write; MEL would use a default value here"
        );
    }

    #[test]
    fn initialized_local_variable_does_not_warn() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$value".to_owned(),
                                    array_size: None,
                                    initializer: Some(Expr::Int {
                                        value: 1,
                                        range: text_range(25, 26),
                                    }),
                                    range: text_range(17, 26),
                                }],
                                range: text_range(13, 27),
                            },
                            range: text_range(13, 27),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$value".to_owned(),
                                range: text_range(28, 34),
                            },
                            range: text_range(28, 35),
                        },
                    ],
                    range: text_range(12, 36),
                },
                is_global: false,
                range: text_range(0, 36),
            }))],
        };

        let analysis = analyze(&source);
        assert!(warning_messages(&analysis).is_empty());
    }

    #[test]
    fn proc_param_read_does_not_warn() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: vec![ProcParam {
                    ty: TypeName::String,
                    name: "$name".to_owned(),
                    is_array: false,
                    range: text_range(12, 24),
                }],
                body: Stmt::Block {
                    statements: vec![Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$name".to_owned(),
                            range: text_range(29, 34),
                        },
                        range: text_range(29, 35),
                    }],
                    range: text_range(26, 36),
                },
                is_global: false,
                range: text_range(0, 36),
            }))],
        };

        let analysis = analyze(&source);
        assert!(warning_messages(&analysis).is_empty());
    }

    #[test]
    fn if_without_else_keeps_maybe_unwritten_state() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$value".to_owned(),
                                    array_size: None,
                                    initializer: None,
                                    range: text_range(17, 23),
                                }],
                                range: text_range(13, 24),
                            },
                            range: text_range(13, 24),
                        },
                        Stmt::If {
                            condition: Expr::Int {
                                value: 1,
                                range: text_range(29, 30),
                            },
                            then_branch: Box::new(Stmt::Block {
                                statements: vec![Stmt::Expr {
                                    expr: Expr::Assign {
                                        op: AssignOp::Assign,
                                        lhs: Box::new(Expr::Ident {
                                            name: "$value".to_owned(),
                                            range: text_range(36, 42),
                                        }),
                                        rhs: Box::new(Expr::Int {
                                            value: 1,
                                            range: text_range(45, 46),
                                        }),
                                        range: text_range(36, 46),
                                    },
                                    range: text_range(36, 47),
                                }],
                                range: text_range(32, 49),
                            }),
                            else_branch: None,
                            range: text_range(25, 49),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$value".to_owned(),
                                range: text_range(50, 56),
                            },
                            range: text_range(50, 57),
                        },
                    ],
                    range: text_range(12, 58),
                },
                is_global: false,
                range: text_range(0, 58),
            }))],
        };

        let analysis = analyze(&source);
        assert!(warning_messages(&analysis).iter().any(|message| {
            *message
                == "local variable \"$value\" is read before its first explicit write; MEL would use a default value here"
        }));
    }

    #[test]
    fn if_else_assigning_both_branches_does_not_warn() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$value".to_owned(),
                                    array_size: None,
                                    initializer: None,
                                    range: text_range(17, 23),
                                }],
                                range: text_range(13, 24),
                            },
                            range: text_range(13, 24),
                        },
                        Stmt::If {
                            condition: Expr::Int {
                                value: 1,
                                range: text_range(29, 30),
                            },
                            then_branch: Box::new(Stmt::Block {
                                statements: vec![Stmt::Expr {
                                    expr: Expr::Assign {
                                        op: AssignOp::Assign,
                                        lhs: Box::new(Expr::Ident {
                                            name: "$value".to_owned(),
                                            range: text_range(36, 42),
                                        }),
                                        rhs: Box::new(Expr::Int {
                                            value: 1,
                                            range: text_range(45, 46),
                                        }),
                                        range: text_range(36, 46),
                                    },
                                    range: text_range(36, 47),
                                }],
                                range: text_range(32, 49),
                            }),
                            else_branch: Some(Box::new(Stmt::Block {
                                statements: vec![Stmt::Expr {
                                    expr: Expr::Assign {
                                        op: AssignOp::Assign,
                                        lhs: Box::new(Expr::Ident {
                                            name: "$value".to_owned(),
                                            range: text_range(57, 63),
                                        }),
                                        rhs: Box::new(Expr::Int {
                                            value: 2,
                                            range: text_range(66, 67),
                                        }),
                                        range: text_range(57, 67),
                                    },
                                    range: text_range(57, 68),
                                }],
                                range: text_range(53, 70),
                            })),
                            range: text_range(25, 70),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$value".to_owned(),
                                range: text_range(71, 77),
                            },
                            range: text_range(71, 78),
                        },
                    ],
                    range: text_range(12, 79),
                },
                is_global: false,
                range: text_range(0, 79),
            }))],
        };

        let analysis = analyze(&source);
        assert!(warning_messages(&analysis).is_empty());
    }

    #[test]
    fn while_loop_write_only_does_not_make_post_read_definite() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$value".to_owned(),
                                    array_size: None,
                                    initializer: None,
                                    range: text_range(17, 23),
                                }],
                                range: text_range(13, 24),
                            },
                            range: text_range(13, 24),
                        },
                        Stmt::While {
                            condition: Expr::Int {
                                value: 1,
                                range: text_range(32, 33),
                            },
                            body: Box::new(Stmt::Block {
                                statements: vec![Stmt::Expr {
                                    expr: Expr::Assign {
                                        op: AssignOp::Assign,
                                        lhs: Box::new(Expr::Ident {
                                            name: "$value".to_owned(),
                                            range: text_range(39, 45),
                                        }),
                                        rhs: Box::new(Expr::Int {
                                            value: 1,
                                            range: text_range(48, 49),
                                        }),
                                        range: text_range(39, 49),
                                    },
                                    range: text_range(39, 50),
                                }],
                                range: text_range(35, 52),
                            }),
                            range: text_range(25, 52),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$value".to_owned(),
                                range: text_range(53, 59),
                            },
                            range: text_range(53, 60),
                        },
                    ],
                    range: text_range(12, 61),
                },
                is_global: false,
                range: text_range(0, 61),
            }))],
        };

        let analysis = analyze(&source);
        assert!(warning_messages(&analysis).iter().any(|message| {
            *message
                == "local variable \"$value\" is read before its first explicit write; MEL would use a default value here"
        }));
    }

    #[test]
    fn do_while_unconditional_write_allows_post_read() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$value".to_owned(),
                                    array_size: None,
                                    initializer: None,
                                    range: text_range(17, 23),
                                }],
                                range: text_range(13, 24),
                            },
                            range: text_range(13, 24),
                        },
                        Stmt::DoWhile {
                            body: Box::new(Stmt::Block {
                                statements: vec![Stmt::Expr {
                                    expr: Expr::Assign {
                                        op: AssignOp::Assign,
                                        lhs: Box::new(Expr::Ident {
                                            name: "$value".to_owned(),
                                            range: text_range(31, 37),
                                        }),
                                        rhs: Box::new(Expr::Int {
                                            value: 1,
                                            range: text_range(40, 41),
                                        }),
                                        range: text_range(31, 41),
                                    },
                                    range: text_range(31, 42),
                                }],
                                range: text_range(27, 44),
                            }),
                            condition: Expr::Int {
                                value: 0,
                                range: text_range(51, 52),
                            },
                            range: text_range(25, 53),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$value".to_owned(),
                                range: text_range(54, 60),
                            },
                            range: text_range(54, 61),
                        },
                    ],
                    range: text_range(12, 62),
                },
                is_global: false,
                range: text_range(0, 62),
            }))],
        };

        let analysis = analyze(&source);
        assert!(warning_messages(&analysis).is_empty());
    }

    #[test]
    fn arrays_and_matrices_are_treated_as_initialized() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$items".to_owned(),
                                    array_size: Some(None),
                                    initializer: None,
                                    range: text_range(17, 24),
                                }],
                                range: text_range(13, 25),
                            },
                            range: text_range(13, 25),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$items".to_owned(),
                                range: text_range(26, 32),
                            },
                            range: text_range(26, 33),
                        },
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Matrix,
                                declarators: vec![Declarator {
                                    name: "$matrix".to_owned(),
                                    array_size: None,
                                    initializer: None,
                                    range: text_range(34, 41),
                                }],
                                range: text_range(34, 42),
                            },
                            range: text_range(34, 42),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$matrix".to_owned(),
                                range: text_range(43, 50),
                            },
                            range: text_range(43, 51),
                        },
                    ],
                    range: text_range(12, 52),
                },
                is_global: false,
                range: text_range(0, 52),
            }))],
        };

        let analysis = analyze(&source);
        assert!(warning_messages(&analysis).is_empty());
    }

    #[test]
    fn compound_assignment_before_write_is_warning() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$value".to_owned(),
                                    array_size: None,
                                    initializer: None,
                                    range: text_range(17, 23),
                                }],
                                range: text_range(13, 24),
                            },
                            range: text_range(13, 24),
                        },
                        Stmt::Expr {
                            expr: Expr::Assign {
                                op: AssignOp::AddAssign,
                                lhs: Box::new(Expr::Ident {
                                    name: "$value".to_owned(),
                                    range: text_range(25, 31),
                                }),
                                rhs: Box::new(Expr::Int {
                                    value: 1,
                                    range: text_range(35, 36),
                                }),
                                range: text_range(25, 36),
                            },
                            range: text_range(25, 37),
                        },
                    ],
                    range: text_range(12, 38),
                },
                is_global: false,
                range: text_range(0, 38),
            }))],
        };

        let analysis = analyze(&source);
        assert!(warning_messages(&analysis).iter().any(|message| {
            *message
                == "local variable \"$value\" is read before its first explicit write; MEL would use a default value here"
        }));
    }

    #[test]
    fn local_shadowing_warnings_cover_parameter_local_and_global() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::VarDecl {
                    decl: VarDecl {
                        is_global: true,
                        ty: TypeName::String,
                        declarators: vec![Declarator {
                            name: "$global".to_owned(),
                            array_size: None,
                            initializer: None,
                            range: text_range(0, 20),
                        }],
                        range: text_range(0, 21),
                    },
                    range: text_range(0, 21),
                })),
                Item::Proc(Box::new(ProcDef {
                    return_type: None,
                    name: "helper".to_owned(),
                    params: vec![ProcParam {
                        ty: TypeName::String,
                        name: "$param".to_owned(),
                        is_array: false,
                        range: text_range(34, 47),
                    }],
                    body: Stmt::Block {
                        statements: vec![
                            Stmt::VarDecl {
                                decl: VarDecl {
                                    is_global: false,
                                    ty: TypeName::Int,
                                    declarators: vec![Declarator {
                                        name: "$param".to_owned(),
                                        array_size: None,
                                        initializer: Some(Expr::Int {
                                            value: 1,
                                            range: text_range(59, 60),
                                        }),
                                        range: text_range(51, 60),
                                    }],
                                    range: text_range(48, 61),
                                },
                                range: text_range(48, 61),
                            },
                            Stmt::VarDecl {
                                decl: VarDecl {
                                    is_global: false,
                                    ty: TypeName::Int,
                                    declarators: vec![Declarator {
                                        name: "$local".to_owned(),
                                        array_size: None,
                                        initializer: Some(Expr::Int {
                                            value: 1,
                                            range: text_range(72, 73),
                                        }),
                                        range: text_range(64, 73),
                                    }],
                                    range: text_range(62, 74),
                                },
                                range: text_range(62, 74),
                            },
                            Stmt::Block {
                                statements: vec![Stmt::VarDecl {
                                    decl: VarDecl {
                                        is_global: false,
                                        ty: TypeName::Int,
                                        declarators: vec![Declarator {
                                            name: "$local".to_owned(),
                                            array_size: None,
                                            initializer: Some(Expr::Int {
                                                value: 2,
                                                range: text_range(87, 88),
                                            }),
                                            range: text_range(79, 88),
                                        }],
                                        range: text_range(75, 89),
                                    },
                                    range: text_range(75, 89),
                                }],
                                range: text_range(74, 90),
                            },
                            Stmt::VarDecl {
                                decl: VarDecl {
                                    is_global: false,
                                    ty: TypeName::Int,
                                    declarators: vec![Declarator {
                                        name: "$global".to_owned(),
                                        array_size: None,
                                        initializer: Some(Expr::Int {
                                            value: 3,
                                            range: text_range(102, 103),
                                        }),
                                        range: text_range(93, 103),
                                    }],
                                    range: text_range(91, 104),
                                },
                                range: text_range(91, 104),
                            },
                        ],
                        range: text_range(47, 105),
                    },
                    is_global: false,
                    range: text_range(22, 105),
                })),
            ],
        };

        let analysis = analyze(&source);
        let warnings = warning_messages(&analysis);
        assert!(warnings.contains(&"local variable \"$param\" shadows visible parameter variable"));
        assert!(warnings.contains(&"local variable \"$local\" shadows visible local variable"));
        assert!(warnings.contains(&"local variable \"$global\" shadows visible global variable"));
    }

    #[test]
    fn unresolved_variable_is_reported_as_warning() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Ident {
                    name: "$missing".to_owned(),
                    range: text_range(0, 8),
                },
                range: text_range(0, 9),
            }))],
        };

        let analysis = analyze(&source);
        let warnings = warning_messages(&analysis);
        assert!(warnings.contains(&"unresolved variable \"$missing\""));
        assert!(resolved_variable(&analysis, 0).is_none());
    }

    #[test]
    fn unresolved_variable_plain_assignment_target_is_not_reported() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Expr {
                        expr: Expr::Assign {
                            op: AssignOp::Assign,
                            lhs: Box::new(Expr::Ident {
                                name: "$missing".to_owned(),
                                range: text_range(13, 21),
                            }),
                            rhs: Box::new(Expr::Int {
                                value: 1,
                                range: text_range(24, 25),
                            }),
                            range: text_range(13, 25),
                        },
                        range: text_range(13, 26),
                    }],
                    range: text_range(12, 27),
                },
                is_global: false,
                range: text_range(0, 27),
            }))],
        };

        let analysis = analyze(&source);
        let unresolved_count = warning_messages(&analysis)
            .into_iter()
            .filter(|message| *message == "unresolved variable \"$missing\"")
            .count();
        assert_eq!(unresolved_count, 0);
        assert!(resolved_variable(&analysis, 0).is_none());
    }

    #[test]
    fn unresolved_variable_compound_assignment_target_is_reported() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Expr {
                        expr: Expr::Assign {
                            op: AssignOp::AddAssign,
                            lhs: Box::new(Expr::Ident {
                                name: "$missing".to_owned(),
                                range: text_range(13, 21),
                            }),
                            rhs: Box::new(Expr::Int {
                                value: 1,
                                range: text_range(25, 26),
                            }),
                            range: text_range(13, 26),
                        },
                        range: text_range(13, 27),
                    }],
                    range: text_range(12, 28),
                },
                is_global: false,
                range: text_range(0, 28),
            }))],
        };

        let analysis = analyze(&source);
        let unresolved_count = warning_messages(&analysis)
            .into_iter()
            .filter(|message| *message == "unresolved variable \"$missing\"")
            .count();
        assert_eq!(unresolved_count, 1);
    }

    #[test]
    fn void_proc_returning_value_reports_diagnostic() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Return {
                        expr: Some(Expr::Int {
                            value: 1,
                            range: text_range(16, 17),
                        }),
                        range: text_range(9, 18),
                    }],
                    range: text_range(8, 19),
                },
                is_global: false,
                range: text_range(0, 19),
            }))],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].message,
            "proc \"helper\" has no return type but returns a value"
        );
    }

    #[test]
    fn typed_proc_without_value_return_reports_diagnostic() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: Some(mel_ast::ProcReturnType {
                    ty: TypeName::Int,
                    is_array: false,
                    range: text_range(5, 8),
                }),
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Return {
                        expr: None,
                        range: text_range(16, 23),
                    }],
                    range: text_range(15, 24),
                },
                is_global: false,
                range: text_range(0, 24),
            }))],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].message,
            "proc \"helper\" declares a return type but never returns a value"
        );
    }

    #[test]
    fn typed_proc_value_return_in_nested_proc_does_not_satisfy_outer_proc() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: Some(mel_ast::ProcReturnType {
                    ty: TypeName::Int,
                    is_array: false,
                    range: text_range(5, 8),
                }),
                name: "outer".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Proc {
                        proc_def: Box::new(ProcDef {
                            return_type: Some(mel_ast::ProcReturnType {
                                ty: TypeName::Int,
                                is_array: false,
                                range: text_range(24, 27),
                            }),
                            name: "inner".to_owned(),
                            params: Vec::new(),
                            body: Stmt::Block {
                                statements: vec![Stmt::Return {
                                    expr: Some(Expr::Int {
                                        value: 1,
                                        range: text_range(42, 43),
                                    }),
                                    range: text_range(35, 44),
                                }],
                                range: text_range(34, 45),
                            },
                            is_global: false,
                            range: text_range(19, 45),
                        }),
                        range: text_range(19, 45),
                    }],
                    range: text_range(14, 46),
                },
                is_global: false,
                range: text_range(0, 46),
            }))],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].message,
            "proc \"outer\" declares a return type but never returns a value"
        );
    }

    #[test]
    fn typed_proc_return_type_mismatch_reports_diagnostic() {
        let source = SourceFile {
            items: vec![Item::Proc(Box::new(ProcDef {
                return_type: Some(mel_ast::ProcReturnType {
                    ty: TypeName::Int,
                    is_array: false,
                    range: text_range(5, 8),
                }),
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Return {
                        expr: Some(Expr::String {
                            text: "\"bad\"".to_owned(),
                            range: text_range(16, 21),
                        }),
                        range: text_range(9, 22),
                    }],
                    range: text_range(8, 23),
                },
                is_global: false,
                range: text_range(0, 23),
            }))],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].message,
            "proc \"helper\" returns String but declares Int"
        );
    }

    #[test]
    fn var_initializer_type_mismatch_reports_diagnostic() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::VarDecl {
                decl: VarDecl {
                    is_global: false,
                    ty: TypeName::String,
                    declarators: vec![Declarator {
                        name: "$value".to_owned(),
                        array_size: None,
                        initializer: Some(Expr::Int {
                            value: 1,
                            range: text_range(16, 17),
                        }),
                        range: text_range(8, 17),
                    }],
                    range: text_range(0, 18),
                },
                range: text_range(0, 18),
            }))],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].message,
            "variable \"$value\" has declared type String but initializer is Int"
        );
    }

    #[test]
    fn proc_invoke_return_type_flows_into_initializer_check() {
        let source = SourceFile {
            items: vec![
                Item::Proc(Box::new(ProcDef {
                    return_type: Some(mel_ast::ProcReturnType {
                        ty: TypeName::String,
                        is_array: false,
                        range: text_range(5, 11),
                    }),
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: vec![Stmt::Return {
                            expr: Some(Expr::String {
                                text: "\"bad\"".to_owned(),
                                range: text_range(24, 29),
                            }),
                            range: text_range(17, 30),
                        }],
                        range: text_range(16, 31),
                    },
                    is_global: false,
                    range: text_range(0, 31),
                })),
                Item::Stmt(Box::new(Stmt::VarDecl {
                    decl: VarDecl {
                        is_global: false,
                        ty: TypeName::Int,
                        declarators: vec![Declarator {
                            name: "$value".to_owned(),
                            array_size: None,
                            initializer: Some(Expr::Invoke(InvokeExpr {
                                surface: InvokeSurface::Function {
                                    name: "helper".to_owned(),
                                    args: Vec::new(),
                                },
                                resolution: CalleeResolution::Unresolved,
                                range: text_range(44, 52),
                            })),
                            range: text_range(36, 52),
                        }],
                        range: text_range(32, 53),
                    },
                    range: text_range(32, 53),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].message,
            "variable \"$value\" has declared type Int but initializer is String"
        );
    }

    #[test]
    fn proc_invoke_return_type_flows_into_return_check() {
        let source = SourceFile {
            items: vec![
                Item::Proc(Box::new(ProcDef {
                    return_type: Some(mel_ast::ProcReturnType {
                        ty: TypeName::String,
                        is_array: false,
                        range: text_range(5, 11),
                    }),
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: vec![Stmt::Return {
                            expr: Some(Expr::String {
                                text: "\"bad\"".to_owned(),
                                range: text_range(24, 29),
                            }),
                            range: text_range(17, 30),
                        }],
                        range: text_range(16, 31),
                    },
                    is_global: false,
                    range: text_range(0, 31),
                })),
                Item::Proc(Box::new(ProcDef {
                    return_type: Some(mel_ast::ProcReturnType {
                        ty: TypeName::Int,
                        is_array: false,
                        range: text_range(37, 40),
                    }),
                    name: "outer".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: vec![Stmt::Return {
                            expr: Some(Expr::Invoke(InvokeExpr {
                                surface: InvokeSurface::Function {
                                    name: "helper".to_owned(),
                                    args: Vec::new(),
                                },
                                resolution: CalleeResolution::Unresolved,
                                range: text_range(56, 64),
                            })),
                            range: text_range(49, 65),
                        }],
                        range: text_range(48, 66),
                    },
                    is_global: false,
                    range: text_range(32, 66),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert_eq!(analysis.diagnostics.len(), 1);
        assert_eq!(
            analysis.diagnostics[0].message,
            "proc \"outer\" returns String but declares Int"
        );
    }

    #[test]
    fn proc_invoke_return_type_allows_matching_initializer() {
        let source = SourceFile {
            items: vec![
                Item::Proc(Box::new(ProcDef {
                    return_type: Some(mel_ast::ProcReturnType {
                        ty: TypeName::Int,
                        is_array: false,
                        range: text_range(5, 8),
                    }),
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: vec![Stmt::Return {
                            expr: Some(Expr::Int {
                                value: 1,
                                range: text_range(21, 22),
                            }),
                            range: text_range(14, 23),
                        }],
                        range: text_range(13, 24),
                    },
                    is_global: false,
                    range: text_range(0, 24),
                })),
                Item::Stmt(Box::new(Stmt::VarDecl {
                    decl: VarDecl {
                        is_global: false,
                        ty: TypeName::Int,
                        declarators: vec![Declarator {
                            name: "$value".to_owned(),
                            array_size: None,
                            initializer: Some(Expr::Invoke(InvokeExpr {
                                surface: InvokeSurface::Function {
                                    name: "helper".to_owned(),
                                    args: Vec::new(),
                                },
                                resolution: CalleeResolution::Unresolved,
                                range: text_range(37, 45),
                            })),
                            range: text_range(29, 45),
                        }],
                        range: text_range(25, 46),
                    },
                    range: text_range(25, 46),
                })),
            ],
        };

        let analysis = analyze(&source);
        assert!(analysis.diagnostics.is_empty());
    }
}
