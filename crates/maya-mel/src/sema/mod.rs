#![forbid(unsafe_code)]
//! Generic semantic analysis for MEL syntax trees.
//!
//! Most users should start with [`analyze`]. It resolves proc and variable
//! usage, emits diagnostics, and optionally normalizes command-style invokes
//! through a caller-provided [`CommandRegistry`].

pub(crate) mod command_norm;
pub(crate) mod command_schema;
mod flow;
mod resolve;
pub(crate) mod scope;

#[cfg(test)]
mod tests;

pub use command_norm::{
    CommandMode, NormalizedCommandInvoke, NormalizedCommandItem, NormalizedFlag, PositionalArg,
};
pub use command_schema::{
    CommandKind, CommandModeMask, CommandRegistry, CommandSchema, CommandSchemaValidationError,
    CommandSourceKind, EmptyCommandRegistry, FlagArity, FlagArityByMode, FlagSchema,
    PositionalSchema, PositionalSlotSchema, PositionalSourcePolicy, PositionalTailSchema,
    ReturnBehavior, StaticCommandRegistry, ValidatedCommandSchema, ValueShape,
};

use flow::FlowLintAnalyzer;
use resolve::Analyzer;
use scope::ScopeCollector;
use std::sync::Arc;

use mel_ast::{SourceFile, Stmt};
use mel_syntax::{SourceView, TextRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Diagnostic severity emitted by semantic analysis.
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Filter used by diagnostics-only semantic entry points.
pub enum DiagnosticFilter {
    All,
    ErrorsOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Semantic diagnostic with primary and secondary labels.
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: Arc<str>,
    pub range: TextRange,
    pub labels: Vec<DiagnosticLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A labeled span attached to a [`Diagnostic`].
pub struct DiagnosticLabel {
    pub range: TextRange,
    pub message: Arc<str>,
    pub is_primary: bool,
}

impl Diagnostic {
    fn error(message: impl Into<Arc<str>>, range: TextRange) -> Self {
        let message = message.into();
        Self {
            severity: DiagnosticSeverity::Error,
            message: message.clone(),
            range,
            labels: vec![DiagnosticLabel {
                range,
                message,
                is_primary: true,
            }],
        }
    }

    fn warning(message: impl Into<Arc<str>>, range: TextRange) -> Self {
        let message = message.into();
        Self {
            severity: DiagnosticSeverity::Warning,
            message: message.clone(),
            range,
            labels: vec![DiagnosticLabel {
                range,
                message,
                is_primary: true,
            }],
        }
    }

    fn with_secondary_label(mut self, message: impl Into<Arc<str>>, range: TextRange) -> Self {
        self.labels.push(DiagnosticLabel {
            range,
            message: message.into(),
            is_primary: false,
        });
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Stable identifier for a collected lexical scope.
pub struct ScopeId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Stable identifier for a collected proc symbol.
pub struct ProcSymbolId(pub(crate) usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Stable identifier for a collected variable symbol.
pub struct VariableSymbolId(pub(crate) usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSymbol {
    pub id: ProcSymbolId,
    pub name_range: TextRange,
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
    pub name_range: TextRange,
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
    pub resolution: ResolvedCallee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCallee {
    Unresolved,
    Proc(ProcSymbolId),
    BuiltinCommand(Arc<str>),
    PluginCommand(Arc<str>),
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
    pub name_range: TextRange,
    pub resolution: IdentTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Full semantic analysis result.
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub proc_symbols: Vec<ProcSymbol>,
    pub variable_symbols: Vec<VariableSymbol>,
    pub invoke_resolutions: Vec<InvokeResolution>,
    pub ident_resolutions: Vec<IdentResolution>,
    pub normalized_invokes: Vec<NormalizedCommandInvoke>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnalysisMode {
    Full,
    DiagnosticsOnly,
}

#[must_use]
/// Run semantic analysis without a command registry.
pub fn analyze(syntax: &SourceFile, source: SourceView<'_>) -> Analysis {
    analyze_with_registry(syntax, source, &EmptyCommandRegistry)
}

#[must_use]
/// Run semantic analysis with a caller-provided command registry.
pub fn analyze_with_registry<R>(
    syntax: &SourceFile,
    source: SourceView<'_>,
    registry: &R,
) -> Analysis
where
    R: CommandRegistry + ?Sized,
{
    analyze_with_registry_mode(
        syntax,
        source,
        registry,
        AnalysisMode::Full,
        DiagnosticFilter::All,
    )
}

#[must_use]
/// Collect only diagnostics while still using a command registry.
pub fn analyze_diagnostics_with_registry<R>(
    syntax: &SourceFile,
    source: SourceView<'_>,
    registry: &R,
) -> Vec<Diagnostic>
where
    R: CommandRegistry + ?Sized,
{
    analyze_diagnostics_with_registry_filtered(syntax, source, registry, DiagnosticFilter::All)
}

#[must_use]
/// Collect only diagnostics with an explicit [`DiagnosticFilter`].
pub fn analyze_diagnostics_with_registry_filtered<R>(
    syntax: &SourceFile,
    source: SourceView<'_>,
    registry: &R,
    filter: DiagnosticFilter,
) -> Vec<Diagnostic>
where
    R: CommandRegistry + ?Sized,
{
    analyze_with_registry_mode(
        syntax,
        source,
        registry,
        AnalysisMode::DiagnosticsOnly,
        filter,
    )
    .diagnostics
}

fn analyze_with_registry_mode<R>(
    syntax: &SourceFile,
    source: SourceView<'_>,
    registry: &R,
    mode: AnalysisMode,
    filter: DiagnosticFilter,
) -> Analysis
where
    R: CommandRegistry + ?Sized,
{
    let collected = ScopeCollector::collect(syntax);
    let mut analyzer = Analyzer::new(
        &collected,
        source,
        registry,
        matches!(mode, AnalysisMode::Full),
        filter,
    );

    for item in &syntax.items {
        analyzer.walk_item(item, collected.root_scope);
    }

    let mut diagnostics = analyzer.diagnostics;
    if matches!(filter, DiagnosticFilter::All) {
        let mut flow_lint = FlowLintAnalyzer::new(&collected, source);
        flow_lint.walk_source(syntax);
        diagnostics.extend(flow_lint.diagnostics);
    }

    Analysis {
        diagnostics,
        proc_symbols: if matches!(mode, AnalysisMode::Full) {
            collected.proc_symbols.clone()
        } else {
            Vec::new()
        },
        variable_symbols: if matches!(mode, AnalysisMode::Full) {
            collected.variable_symbols.clone()
        } else {
            Vec::new()
        },
        invoke_resolutions: if matches!(mode, AnalysisMode::Full) {
            analyzer.invoke_resolutions
        } else {
            Vec::new()
        },
        ident_resolutions: if matches!(mode, AnalysisMode::Full) {
            analyzer.ident_resolutions
        } else {
            Vec::new()
        },
        normalized_invokes: if matches!(mode, AnalysisMode::Full) {
            analyzer.normalized_invokes
        } else {
            Vec::new()
        },
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
