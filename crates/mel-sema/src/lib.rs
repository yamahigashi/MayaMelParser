#![forbid(unsafe_code)]
//! Minimal semantic analysis scaffold.

mod command_norm;
mod command_schema;

use std::collections::HashMap;

pub use command_norm::{
    CommandMode, NormalizedCommandInvoke, NormalizedCommandItem, NormalizedFlag, PositionalArg,
};
pub use command_schema::{
    CommandKind, CommandModeMask, CommandRegistry, CommandSchema, CommandSourceKind,
    EmbeddedCommandRegistry, EmptyCommandRegistry, FlagArity, FlagSchema, ReturnBehavior,
    ValueShape,
};
use mel_ast::{CalleeResolution, Expr, Item, ProcDef, ShellWord, SourceFile, Stmt, SwitchClause};
use mel_syntax::TextRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub range: TextRange,
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

    Analysis {
        diagnostics: analyzer.diagnostics,
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

struct Analyzer<'a, R: ?Sized> {
    collected: &'a CollectedScopes,
    registry: &'a R,
    diagnostics: Vec<Diagnostic>,
    invoke_resolutions: Vec<InvokeResolution>,
    ident_resolutions: Vec<IdentResolution>,
    normalized_invokes: Vec<NormalizedCommandInvoke>,
    visible_decl_orders: HashMap<ScopeId, usize>,
    visible_variable_decl_orders: HashMap<ScopeId, usize>,
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
            Expr::Unary { expr, .. }
            | Expr::PrefixUpdate { expr, .. }
            | Expr::PostfixUpdate { expr, .. } => self.walk_expr(expr, current_scope),
            Expr::Binary { lhs, rhs, .. } | Expr::Assign { lhs, rhs, .. } => {
                self.walk_expr(lhs, current_scope);
                self.walk_expr(rhs, current_scope);
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
            self.diagnostics.push(Diagnostic {
                message: format!("local proc \"{name}\" is called before its definition"),
                range,
            });
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
            (None, Some(_)) => self.diagnostics.push(Diagnostic {
                message: format!(
                    "proc \"{}\" has no return type but returns a value",
                    context.name
                ),
                range,
            }),
            (Some(expected), Some(actual)) => {
                context.saw_value_return = true;
                if !is_assignable(expected, actual) {
                    self.diagnostics.push(Diagnostic {
                        message: format!(
                            "proc \"{}\" returns {:?} but declares {:?}",
                            context.name, actual, expected
                        ),
                        range,
                    });
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
            self.diagnostics.push(Diagnostic {
                message: format!(
                    "proc \"{}\" declares a return type but never returns a value",
                    context.name
                ),
                range: context.range,
            });
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
            self.diagnostics.push(Diagnostic {
                message: format!(
                    "variable \"{}\" has declared type {:?} but initializer is {:?}",
                    declarator.name, expected, actual
                ),
                range: declarator.range,
            });
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
        CommandSourceKind, EmbeddedCommandRegistry, FlagArity, FlagSchema, IdentTarget,
        ReturnBehavior, ValueShape, VariableKind, analyze, analyze_with_registry,
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
            return_behavior: ReturnBehavior::Unknown,
            flags: Vec::new(),
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
            arity,
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
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Assign {
                        op: AssignOp::Assign,
                        lhs: Box::new(Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(12, 18),
                        }),
                        rhs: Box::new(Expr::Invoke(InvokeExpr {
                            surface: InvokeSurface::ShellLike {
                                head: "helper".to_owned(),
                                words: vec![ShellWord::Variable {
                                    expr: Expr::Ident {
                                        name: "$selection".to_owned(),
                                        range: text_range(23, 33),
                                    },
                                    range: text_range(23, 33),
                                }],
                                captured: true,
                            },
                            resolution: CalleeResolution::Unresolved,
                            range: text_range(19, 34),
                        })),
                        range: text_range(12, 34),
                    },
                    range: text_range(12, 35),
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
        command.flags = vec![
            FlagSchema {
                mode_mask: CommandModeMask {
                    create: true,
                    edit: true,
                    query: true,
                },
                value_shapes: Vec::new(),
                ..flag_schema("query", Some("q"), FlagArity::None)
            },
            FlagSchema {
                mode_mask: CommandModeMask {
                    create: true,
                    edit: true,
                    query: true,
                },
                value_shapes: Vec::new(),
                ..flag_schema("edit", Some("e"), FlagArity::None)
            },
            FlagSchema {
                mode_mask: CommandModeMask {
                    create: false,
                    edit: true,
                    query: false,
                },
                value_shapes: vec![ValueShape::String],
                ..flag_schema("label", Some("l"), FlagArity::Exact(1))
            },
        ];
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
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("not available in query mode"))
        );
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
        command.flags = vec![
            FlagSchema {
                value_shapes: Vec::new(),
                ..flag_schema("query", Some("q"), FlagArity::None)
            },
            FlagSchema {
                value_shapes: Vec::new(),
                ..flag_schema("edit", Some("e"), FlagArity::None)
            },
        ];
        let registry = TestRegistry {
            commands: vec![command],
        };

        let analysis = analyze_with_registry(&source, &registry);
        assert_eq!(analysis.normalized_invokes[0].mode, CommandMode::Unknown);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("both query and edit mode flags")
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
