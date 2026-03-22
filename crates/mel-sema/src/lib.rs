#![forbid(unsafe_code)]
//! Minimal semantic analysis scaffold.

mod command_norm;
mod command_schema;
mod flow;
mod resolve;
mod scope;

#[cfg(test)]
mod tests;

pub use command_norm::{
    CommandMode, NormalizedCommandInvoke, NormalizedCommandItem, NormalizedFlag, PositionalArg,
};
pub use command_schema::{
    CommandKind, CommandModeMask, CommandRegistry, CommandSchema, CommandSourceKind,
    EmptyCommandRegistry, FlagArity, FlagArityByMode, FlagSchema, ReturnBehavior, ValueShape,
};

use flow::FlowLintAnalyzer;
use resolve::Analyzer;
use scope::ScopeCollector;

use mel_ast::{CalleeResolution, SourceFile, Stmt};
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
pub struct ScopeId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcSymbolId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariableSymbolId(pub(crate) usize);

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

#[must_use]
pub fn analyze(source: &SourceFile) -> Analysis {
    analyze_with_registry(source, &EmptyCommandRegistry)
}

#[must_use]
pub fn analyze_with_registry<R>(source: &SourceFile, registry: &R) -> Analysis
where
    R: CommandRegistry + ?Sized,
{
    let collected = ScopeCollector::collect(source);
    let mut analyzer = Analyzer::new(&collected, registry);

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

pub(crate) fn stmt_range(stmt: &Stmt) -> TextRange {
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
