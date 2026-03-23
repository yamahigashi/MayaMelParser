use std::collections::HashMap;

use crate::*;
use mel_ast::{Item, ProcDef, SourceFile, Stmt, SwitchClause};
use mel_syntax::{TextRange, text_slice};

pub(crate) struct ScopeTree {
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

    pub(crate) fn parent(&self, scope: ScopeId) -> Option<ScopeId> {
        self.parents[scope.0]
    }
}

pub(crate) struct ScopeCollector {
    scopes: ScopeTree,
    proc_symbols: Vec<ProcSymbol>,
    variable_symbols: Vec<VariableSymbol>,
    local_symbols_by_scope: HashMap<ScopeId, Vec<ProcSymbolId>>,
    global_symbol_ids: Vec<ProcSymbolId>,
    next_decl_order_by_scope: HashMap<ScopeId, usize>,
    local_variables_by_scope: HashMap<ScopeId, Vec<VariableSymbolId>>,
    global_variable_ids: Vec<VariableSymbolId>,
    next_variable_decl_order_by_scope: HashMap<ScopeId, usize>,
    scope_by_range: HashMap<TextRange, ScopeId>,
    symbol_by_proc_range: HashMap<TextRange, ProcSymbolId>,
    variable_symbols_by_stmt_range: HashMap<TextRange, Vec<VariableSymbolId>>,
    param_symbols_by_proc_range: HashMap<TextRange, Vec<VariableSymbolId>>,
}

impl ScopeCollector {
    pub(crate) fn collect(source: &SourceFile) -> CollectedScopes {
        let scopes = ScopeTree::new();
        let root_scope = scopes.root_scope();
        let mut collector = Self {
            scopes,
            proc_symbols: Vec::new(),
            variable_symbols: Vec::new(),
            local_symbols_by_scope: HashMap::new(),
            global_symbol_ids: Vec::new(),
            next_decl_order_by_scope: HashMap::new(),
            local_variables_by_scope: HashMap::new(),
            global_variable_ids: Vec::new(),
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
            global_symbol_ids: collector.global_symbol_ids,
            local_variables_by_scope: collector.local_variables_by_scope,
            global_variable_ids: collector.global_variable_ids,
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
            name_range: proc_def.name_range,
            is_global: proc_def.is_global,
            return_type: proc_def.return_type.clone(),
            owner_scope,
            decl_order,
            range: proc_def.range,
        });
        self.symbol_by_proc_range.insert(proc_def.range, symbol_id);

        if proc_def.is_global {
            self.global_symbol_ids.push(symbol_id);
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
                name_range: param.name_range,
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
                name_range: declarator.name_range,
                kind,
                ty: decl.ty.clone(),
                is_array: declarator.array_size.is_some(),
                owner_scope,
                decl_order,
                range: declarator.range,
            });

            match kind {
                VariableKind::Global => {
                    self.global_variable_ids.push(symbol_id);
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

pub(crate) struct CollectedScopes {
    pub(crate) scopes: ScopeTree,
    pub(crate) proc_symbols: Vec<ProcSymbol>,
    pub(crate) variable_symbols: Vec<VariableSymbol>,
    local_symbols_by_scope: HashMap<ScopeId, Vec<ProcSymbolId>>,
    global_symbol_ids: Vec<ProcSymbolId>,
    local_variables_by_scope: HashMap<ScopeId, Vec<VariableSymbolId>>,
    global_variable_ids: Vec<VariableSymbolId>,
    scope_by_range: HashMap<TextRange, ScopeId>,
    symbol_by_proc_range: HashMap<TextRange, ProcSymbolId>,
    variable_symbols_by_stmt_range: HashMap<TextRange, Vec<VariableSymbolId>>,
    param_symbols_by_proc_range: HashMap<TextRange, Vec<VariableSymbolId>>,
    pub(crate) root_scope: ScopeId,
}

impl CollectedScopes {
    pub(crate) fn scope_for_stmt(&self, stmt: &Stmt) -> ScopeId {
        self.scope_by_range[&stmt_range(stmt)]
    }

    pub(crate) fn scope_for_clause(&self, clause: &SwitchClause) -> ScopeId {
        self.scope_by_range[&clause.range]
    }

    pub(crate) fn symbol_for_proc(&self, proc_def: &ProcDef) -> &ProcSymbol {
        let symbol_id = self.symbol_by_proc_range[&proc_def.range];
        self.symbol(symbol_id)
    }

    pub(crate) fn symbol(&self, id: ProcSymbolId) -> &ProcSymbol {
        &self.proc_symbols[id.0]
    }

    pub(crate) fn proc_name<'a>(&self, source_text: &'a str, id: ProcSymbolId) -> &'a str {
        text_slice(source_text, self.symbol(id).name_range)
    }

    pub(crate) fn variable_symbol(&self, id: VariableSymbolId) -> &VariableSymbol {
        &self.variable_symbols[id.0]
    }

    pub(crate) fn variable_name<'a>(&self, source_text: &'a str, id: VariableSymbolId) -> &'a str {
        text_slice(source_text, self.variable_symbol(id).name_range)
    }

    pub(crate) fn find_visible_local_proc(
        &self,
        source_text: &str,
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
                            (self.proc_name(source_text, *symbol_id) == name
                                && symbol.decl_order <= visible_order)
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

    pub(crate) fn find_forward_local_proc(
        &self,
        source_text: &str,
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
                        (self.proc_name(source_text, *symbol_id) == name
                            && symbol.decl_order > visible_order)
                            .then_some(symbol)
                    })
                    .min_by_key(|symbol| symbol.decl_order)
            })
    }

    pub(crate) fn find_global_proc(&self, source_text: &str, name: &str) -> Option<&ProcSymbol> {
        self.global_symbol_ids.iter().find_map(|symbol_id| {
            let symbol = self.symbol(*symbol_id);
            (text_slice(source_text, symbol.name_range) == name).then_some(symbol)
        })
    }

    pub(crate) fn find_resolved_proc_symbol(
        &self,
        source_text: &str,
        name: &str,
        scope: ScopeId,
        visible_decl_orders: &HashMap<ScopeId, usize>,
    ) -> Option<&ProcSymbol> {
        self.find_visible_local_proc(source_text, name, scope, visible_decl_orders)
            .or_else(|| self.find_forward_local_proc(source_text, name, scope, visible_decl_orders))
            .or_else(|| self.find_global_proc(source_text, name))
    }

    pub(crate) fn find_visible_local_variable(
        &self,
        source_text: &str,
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
                            (self.variable_name(source_text, *symbol_id) == name
                                && symbol.decl_order <= visible_order)
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

    pub(crate) fn find_global_variable(
        &self,
        source_text: &str,
        name: &str,
    ) -> Option<&VariableSymbol> {
        self.global_variable_ids.iter().find_map(|symbol_id| {
            let symbol = self.variable_symbol(*symbol_id);
            (text_slice(source_text, symbol.name_range) == name).then_some(symbol)
        })
    }

    pub(crate) fn variable_symbols_for_stmt(&self, stmt: &Stmt) -> &[VariableSymbolId] {
        self.variable_symbols_by_stmt_range
            .get(&stmt_range(stmt))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(crate) fn param_symbols_for_proc(&self, proc_def: &ProcDef) -> &[VariableSymbolId] {
        self.param_symbols_by_proc_range
            .get(&proc_def.range)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}
