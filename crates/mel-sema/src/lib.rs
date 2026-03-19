#![forbid(unsafe_code)]
//! Minimal semantic analysis scaffold.

use std::collections::HashMap;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSymbol {
    pub id: ProcSymbolId,
    pub name: String,
    pub is_global: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub proc_symbols: Vec<ProcSymbol>,
    pub invoke_resolutions: Vec<InvokeResolution>,
}

#[must_use]
pub fn analyze(source: &SourceFile) -> Analysis {
    let collected = ScopeCollector::collect(source);
    let mut analyzer = Analyzer::new(&collected);

    for item in &source.items {
        analyzer.walk_item(item, collected.root_scope);
    }

    Analysis {
        diagnostics: analyzer.diagnostics,
        proc_symbols: collected.proc_symbols.clone(),
        invoke_resolutions: analyzer.invoke_resolutions,
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
    local_symbols_by_scope: HashMap<ScopeId, Vec<ProcSymbolId>>,
    global_symbols_by_name: HashMap<String, ProcSymbolId>,
    next_decl_order_by_scope: HashMap<ScopeId, usize>,
    scope_by_range: HashMap<TextRange, ScopeId>,
    symbol_by_proc_range: HashMap<TextRange, ProcSymbolId>,
}

impl ScopeCollector {
    fn collect(source: &SourceFile) -> CollectedScopes {
        let scopes = ScopeTree::new();
        let root_scope = scopes.root_scope();
        let mut collector = Self {
            scopes,
            proc_symbols: Vec::new(),
            local_symbols_by_scope: HashMap::new(),
            global_symbols_by_name: HashMap::new(),
            next_decl_order_by_scope: HashMap::new(),
            scope_by_range: HashMap::new(),
            symbol_by_proc_range: HashMap::new(),
        };

        for item in &source.items {
            collector.collect_item(item, root_scope);
        }

        CollectedScopes {
            scopes: collector.scopes,
            proc_symbols: collector.proc_symbols,
            local_symbols_by_scope: collector.local_symbols_by_scope,
            global_symbols_by_name: collector.global_symbols_by_name,
            scope_by_range: collector.scope_by_range,
            symbol_by_proc_range: collector.symbol_by_proc_range,
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
        self.collect_stmt_in_existing_scope(&proc_def.body, body_scope);
    }

    fn collect_stmt(&mut self, stmt: &Stmt, current_scope: ScopeId) {
        match stmt {
            Stmt::Proc { proc_def, .. } => self.collect_proc_def(proc_def, current_scope),
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
            | Stmt::VarDecl { .. }
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

    fn new_child_scope(&mut self, parent_scope: ScopeId, range: TextRange) -> ScopeId {
        let scope = self.scopes.new_child(parent_scope);
        self.scope_by_range.insert(range, scope);
        scope
    }
}

struct CollectedScopes {
    scopes: ScopeTree,
    proc_symbols: Vec<ProcSymbol>,
    local_symbols_by_scope: HashMap<ScopeId, Vec<ProcSymbolId>>,
    global_symbols_by_name: HashMap<String, ProcSymbolId>,
    scope_by_range: HashMap<TextRange, ScopeId>,
    symbol_by_proc_range: HashMap<TextRange, ProcSymbolId>,
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
}

struct Analyzer<'a> {
    collected: &'a CollectedScopes,
    diagnostics: Vec<Diagnostic>,
    invoke_resolutions: Vec<InvokeResolution>,
    visible_decl_orders: HashMap<ScopeId, usize>,
}

impl<'a> Analyzer<'a> {
    fn new(collected: &'a CollectedScopes) -> Self {
        Self {
            collected,
            diagnostics: Vec::new(),
            invoke_resolutions: Vec::new(),
            visible_decl_orders: HashMap::new(),
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
        self.walk_stmt_in_existing_scope(&proc_def.body, body_scope);
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
                self.mark_proc_visible(proc_def);
                let body_scope = self.collected.scope_for_stmt(&proc_def.body);
                self.walk_stmt_in_existing_scope(&proc_def.body, body_scope);
            }
            Stmt::Expr { expr, .. } => self.walk_expr(expr, current_scope),
            Stmt::VarDecl { decl, .. } => {
                for declarator in &decl.declarators {
                    if let Some(Some(size)) = &declarator.array_size {
                        self.walk_expr(size, current_scope);
                    }

                    if let Some(initializer) = &declarator.initializer {
                        self.walk_expr(initializer, current_scope);
                    }
                }
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
            Expr::Ident { .. }
            | Expr::BareWord { .. }
            | Expr::Int { .. }
            | Expr::Float { .. }
            | Expr::String { .. } => {}
        }
    }

    fn walk_invoke(&mut self, invoke: &mel_ast::InvokeExpr, current_scope: ScopeId) {
        let resolution = match &invoke.surface {
            mel_ast::InvokeSurface::Function { name, args } => {
                for arg in args {
                    self.walk_expr(arg, current_scope);
                }
                self.resolve_function_invoke(name, invoke.range, current_scope)
            }
            mel_ast::InvokeSurface::ShellLike { words, .. } => {
                for word in words {
                    self.walk_shell_word(word, current_scope);
                }
                CalleeResolution::Unresolved
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

    fn resolve_function_invoke(
        &mut self,
        name: &str,
        range: TextRange,
        current_scope: ScopeId,
    ) -> CalleeResolution {
        if let Some(symbol) =
            self.collected
                .find_visible_local_proc(name, current_scope, &self.visible_decl_orders)
        {
            return CalleeResolution::Proc(symbol.name.clone());
        }

        if let Some(symbol) =
            self.collected
                .find_forward_local_proc(name, current_scope, &self.visible_decl_orders)
        {
            self.diagnostics.push(Diagnostic {
                message: format!("local proc \"{name}\" is called before its definition"),
                range,
            });
            return CalleeResolution::Proc(symbol.name.clone());
        }

        if let Some(symbol) = self.collected.find_global_proc(name) {
            return CalleeResolution::Proc(symbol.name.clone());
        }

        CalleeResolution::Unresolved
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

#[cfg(test)]
mod tests {
    use super::analyze;
    use mel_ast::{
        AssignOp, CalleeResolution, Expr, InvokeExpr, InvokeSurface, Item, ProcDef, ShellWord,
        SourceFile, Stmt,
    };
    use mel_syntax::text_range;

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
    fn shell_like_calls_remain_unresolved_without_proc_diagnostic() {
        let source = SourceFile {
            items: vec![
                Item::Stmt(Box::new(Stmt::Expr {
                    expr: Expr::Assign {
                        op: AssignOp::Assign,
                        lhs: Box::new(Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(0, 6),
                        }),
                        rhs: Box::new(Expr::Invoke(InvokeExpr {
                            surface: InvokeSurface::ShellLike {
                                head: "helper".to_owned(),
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
                })),
                Item::Proc(Box::new(ProcDef {
                    return_type: None,
                    name: "helper".to_owned(),
                    params: Vec::new(),
                    body: Stmt::Block {
                        statements: Vec::new(),
                        range: text_range(31, 33),
                    },
                    is_global: false,
                    range: text_range(24, 33),
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
}
