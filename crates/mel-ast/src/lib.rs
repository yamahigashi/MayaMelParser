#![forbid(unsafe_code)]
//! Typed AST shapes used by the parser and semantic layers.

use mel_syntax::TextRange;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceFile {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Proc(Box<ProcDef>),
    Stmt(Box<Stmt>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcDef {
    pub return_type: Option<ProcReturnType>,
    pub name: String,
    pub params: Vec<ProcParam>,
    pub body: Stmt,
    pub is_global: bool,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Empty {
        range: TextRange,
    },
    Proc {
        proc_def: Box<ProcDef>,
        range: TextRange,
    },
    Block {
        statements: Vec<Stmt>,
        range: TextRange,
    },
    Expr {
        expr: Expr,
        range: TextRange,
    },
    VarDecl {
        decl: VarDecl,
        range: TextRange,
    },
    If {
        condition: Expr,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
        range: TextRange,
    },
    While {
        condition: Expr,
        body: Box<Stmt>,
        range: TextRange,
    },
    DoWhile {
        body: Box<Stmt>,
        condition: Expr,
        range: TextRange,
    },
    Switch {
        control: Expr,
        clauses: Vec<SwitchClause>,
        range: TextRange,
    },
    For {
        init: Option<Vec<Expr>>,
        condition: Option<Expr>,
        update: Option<Vec<Expr>>,
        body: Box<Stmt>,
        range: TextRange,
    },
    ForIn {
        binding: Expr,
        iterable: Expr,
        body: Box<Stmt>,
        range: TextRange,
    },
    Return {
        expr: Option<Expr>,
        range: TextRange,
    },
    Break {
        range: TextRange,
    },
    Continue {
        range: TextRange,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Ident {
        name: String,
        range: TextRange,
    },
    BareWord {
        text: String,
        range: TextRange,
    },
    Int {
        value: i64,
        range: TextRange,
    },
    Float {
        text: String,
        range: TextRange,
    },
    String {
        text: String,
        range: TextRange,
    },
    Cast {
        ty: TypeName,
        expr: Box<Expr>,
        range: TextRange,
    },
    VectorLiteral {
        elements: Vec<Expr>,
        range: TextRange,
    },
    ArrayLiteral {
        elements: Vec<Expr>,
        range: TextRange,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        range: TextRange,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        range: TextRange,
    },
    Ternary {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
        range: TextRange,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
        range: TextRange,
    },
    MemberAccess {
        target: Box<Expr>,
        member: String,
        range: TextRange,
    },
    ComponentAccess {
        target: Box<Expr>,
        component: VectorComponent,
        range: TextRange,
    },
    Assign {
        op: AssignOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        range: TextRange,
    },
    PrefixUpdate {
        op: UpdateOp,
        expr: Box<Expr>,
        range: TextRange,
    },
    PostfixUpdate {
        op: UpdateOp,
        expr: Box<Expr>,
        range: TextRange,
    },
    Invoke(InvokeExpr),
}

impl Expr {
    #[must_use]
    pub const fn range(&self) -> TextRange {
        match self {
            Self::Ident { range, .. }
            | Self::BareWord { range, .. }
            | Self::Int { range, .. }
            | Self::Float { range, .. }
            | Self::String { range, .. }
            | Self::Cast { range, .. }
            | Self::VectorLiteral { range, .. }
            | Self::ArrayLiteral { range, .. }
            | Self::Unary { range, .. }
            | Self::Binary { range, .. }
            | Self::Ternary { range, .. }
            | Self::Index { range, .. }
            | Self::MemberAccess { range, .. }
            | Self::ComponentAccess { range, .. }
            | Self::Assign { range, .. }
            | Self::PrefixUpdate { range, .. }
            | Self::PostfixUpdate { range, .. } => *range,
            Self::Invoke(invoke) => invoke.range,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    pub is_global: bool,
    pub ty: TypeName,
    pub declarators: Vec<Declarator>,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcParam {
    pub ty: TypeName,
    pub name: String,
    pub is_array: bool,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcReturnType {
    pub ty: TypeName,
    pub is_array: bool,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchClause {
    pub label: SwitchLabel,
    pub statements: Vec<Stmt>,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchLabel {
    Case(Expr),
    Default { range: TextRange },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeName {
    Int,
    Float,
    String,
    Vector,
    Matrix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declarator {
    pub name: String,
    pub array_size: Option<Option<Expr>>,
    pub initializer: Option<Expr>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorComponent {
    X,
    Y,
    Z,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Negate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryOp {
    Mul,
    Div,
    Rem,
    Caret,
    Add,
    Sub,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    NotEq,
    AndAnd,
    OrOr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateOp {
    Increment,
    Decrement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeSurface {
    Function {
        name: String,
        args: Vec<Expr>,
    },
    ShellLike {
        head: String,
        words: Vec<ShellWord>,
        captured: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellWord {
    Flag {
        text: String,
        range: TextRange,
    },
    NumericLiteral {
        text: String,
        range: TextRange,
    },
    BareWord {
        text: String,
        range: TextRange,
    },
    QuotedString {
        text: String,
        range: TextRange,
    },
    Variable {
        expr: Expr,
        range: TextRange,
    },
    GroupedExpr {
        expr: Expr,
        range: TextRange,
    },
    BraceList {
        expr: Expr,
        range: TextRange,
    },
    VectorLiteral {
        expr: Expr,
        range: TextRange,
    },
    Capture {
        invoke: InvokeExpr,
        range: TextRange,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalleeResolution {
    Unresolved,
    Proc(String),
    BuiltinCommand(String),
    PluginCommand(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvokeExpr {
    pub surface: InvokeSurface,
    pub resolution: CalleeResolution,
    pub range: TextRange,
}

impl ProcDef {
    #[must_use]
    pub fn name_text(&self, source_text: &str) -> &str {
        let _ = source_text;
        self.name.as_str()
    }
}

impl ProcParam {
    #[must_use]
    pub fn name_text(&self, source_text: &str) -> &str {
        let _ = source_text;
        self.name.as_str()
    }
}

impl Declarator {
    #[must_use]
    pub fn name_text(&self, source_text: &str) -> &str {
        let _ = source_text;
        self.name.as_str()
    }
}

impl InvokeSurface {
    #[must_use]
    pub fn head_text(&self, source_text: &str) -> Option<&str> {
        match self {
            Self::Function { name, .. } | Self::ShellLike { head: name, .. } => {
                let _ = source_text;
                Some(name.as_str())
            }
        }
    }
}

impl ShellWord {
    #[must_use]
    pub fn text_range(&self) -> Option<TextRange> {
        match self {
            Self::Flag { range, .. }
            | Self::NumericLiteral { range, .. }
            | Self::BareWord { range, .. }
            | Self::QuotedString { range, .. } => Some(*range),
            Self::Variable { .. }
            | Self::GroupedExpr { .. }
            | Self::BraceList { .. }
            | Self::VectorLiteral { .. }
            | Self::Capture { .. } => None,
        }
    }

    #[must_use]
    pub fn text(&self, source_text: &str) -> Option<&str> {
        match self {
            Self::Flag { text, .. }
            | Self::NumericLiteral { text, .. }
            | Self::BareWord { text, .. }
            | Self::QuotedString { text, .. } => {
                let _ = source_text;
                Some(text.as_str())
            }
            Self::Variable { .. }
            | Self::GroupedExpr { .. }
            | Self::BraceList { .. }
            | Self::VectorLiteral { .. }
            | Self::Capture { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CalleeResolution, Expr, InvokeExpr, InvokeSurface, ShellWord};
    use mel_syntax::text_range;

    #[test]
    fn unresolved_invoke_is_constructible() {
        let invoke = InvokeExpr {
            surface: InvokeSurface::ShellLike {
                head: "ls".to_owned(),
                words: vec![
                    ShellWord::NumericLiteral {
                        text: "1".to_owned(),
                        range: text_range(3, 4),
                    },
                    ShellWord::Variable {
                        expr: Expr::Ident {
                            name: "$items".to_owned(),
                            range: text_range(5, 11),
                        },
                        range: text_range(5, 11),
                    },
                ],
                captured: true,
            },
            resolution: CalleeResolution::Unresolved,
            range: text_range(0, 12),
        };

        assert!(matches!(invoke.resolution, CalleeResolution::Unresolved));
    }
}
