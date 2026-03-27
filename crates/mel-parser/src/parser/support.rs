use super::*;

#[derive(Clone)]
pub(super) enum InfixKind {
    Binary(BinaryOp),
    Assign(AssignOp),
}

pub(super) fn infix_binding_power(kind: TokenKind) -> Option<(u8, u8, InfixKind)> {
    match kind {
        TokenKind::Star => Some((70, 71, InfixKind::Binary(BinaryOp::Mul))),
        TokenKind::Slash => Some((70, 71, InfixKind::Binary(BinaryOp::Div))),
        TokenKind::Percent => Some((70, 71, InfixKind::Binary(BinaryOp::Rem))),
        TokenKind::Caret => Some((70, 71, InfixKind::Binary(BinaryOp::Caret))),
        TokenKind::Plus => Some((60, 61, InfixKind::Binary(BinaryOp::Add))),
        TokenKind::Minus => Some((60, 61, InfixKind::Binary(BinaryOp::Sub))),
        TokenKind::Lt => Some((50, 51, InfixKind::Binary(BinaryOp::Lt))),
        TokenKind::Le => Some((50, 51, InfixKind::Binary(BinaryOp::Le))),
        TokenKind::Gt => Some((50, 51, InfixKind::Binary(BinaryOp::Gt))),
        TokenKind::Ge => Some((50, 51, InfixKind::Binary(BinaryOp::Ge))),
        TokenKind::EqEq => Some((40, 41, InfixKind::Binary(BinaryOp::EqEq))),
        TokenKind::NotEq => Some((40, 41, InfixKind::Binary(BinaryOp::NotEq))),
        TokenKind::AndAnd => Some((30, 31, InfixKind::Binary(BinaryOp::AndAnd))),
        TokenKind::OrOr => Some((20, 21, InfixKind::Binary(BinaryOp::OrOr))),
        TokenKind::Assign => Some((10, 10, InfixKind::Assign(AssignOp::Assign))),
        TokenKind::PlusEq => Some((10, 10, InfixKind::Assign(AssignOp::AddAssign))),
        TokenKind::MinusEq => Some((10, 10, InfixKind::Assign(AssignOp::SubAssign))),
        TokenKind::StarEq => Some((10, 10, InfixKind::Assign(AssignOp::MulAssign))),
        TokenKind::SlashEq => Some((10, 10, InfixKind::Assign(AssignOp::DivAssign))),
        _ => None,
    }
}

pub(super) fn stmt_range(stmt: &Stmt) -> TextRange {
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

pub(super) fn is_type_keyword(keyword: &str) -> bool {
    matches!(keyword, "int" | "float" | "string" | "vector" | "matrix")
}

pub(super) fn parse_type_name(keyword: &str) -> Option<TypeName> {
    match keyword {
        "int" => Some(TypeName::Int),
        "float" => Some(TypeName::Float),
        "string" => Some(TypeName::String),
        "vector" => Some(TypeName::Vector),
        "matrix" => Some(TypeName::Matrix),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IntLiteralError {
    OutOfRange,
}

pub(super) fn parse_int_literal_text(text: &str) -> Result<i64, IntLiteralError> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).map_err(|_| IntLiteralError::OutOfRange)
    } else {
        text.parse::<i64>().map_err(|_| IntLiteralError::OutOfRange)
    }
}

pub(super) fn parse_vector_component_name(name: &str) -> Option<VectorComponent> {
    match name {
        "x" => Some(VectorComponent::X),
        "y" => Some(VectorComponent::Y),
        "z" => Some(VectorComponent::Z),
        _ => None,
    }
}
