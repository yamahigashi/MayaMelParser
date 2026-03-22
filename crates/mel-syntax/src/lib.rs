#![forbid(unsafe_code)]
//! Shared syntax primitives for the MEL parser workspace.

pub use text_size::{TextRange, TextSize};

#[must_use]
pub const fn text_size(value: u32) -> TextSize {
    TextSize::new(value)
}

#[must_use]
pub const fn text_range(start: u32, end: u32) -> TextRange {
    TextRange::new(text_size(start), text_size(end))
}

#[must_use]
pub fn range_start(range: TextRange) -> u32 {
    range.start().into()
}

#[must_use]
pub fn range_end(range: TextRange) -> u32 {
    range.end().into()
}

#[must_use]
pub fn range_len(range: TextRange) -> u32 {
    range.len().into()
}

#[must_use]
pub fn text_slice(text: &str, range: TextRange) -> &str {
    &text[range_start(range) as usize..range_end(range) as usize]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Whitespace,
    LineComment,
    BlockComment,
    Ident,
    IntLiteral,
    FloatLiteral,
    StringLiteral,
    Flag,
    Dollar,
    Backquote,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Dot,
    Pipe,
    Comma,
    Semi,
    Assign,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    Plus,
    PlusPlus,
    Minus,
    MinusMinus,
    Star,
    Slash,
    Percent,
    Caret,
    Question,
    Colon,
    EqEq,
    NotEq,
    LtLt,
    Lt,
    Le,
    GtGt,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    Unknown,
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Token {
    pub kind: TokenKind,
    pub range: TextRange,
}

impl Token {
    #[must_use]
    pub const fn new(kind: TokenKind, range: TextRange) -> Self {
        Self { kind, range }
    }
}

impl TokenKind {
    #[must_use]
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexDiagnostic {
    pub message: String,
    pub range: TextRange,
}

impl LexDiagnostic {
    #[must_use]
    pub fn new(message: impl Into<String>, range: TextRange) -> Self {
        Self {
            message: message.into(),
            range,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<LexDiagnostic>,
}

#[cfg(test)]
mod tests {
    use super::{LexDiagnostic, Token, TokenKind, range_len, text_range};

    #[test]
    fn text_range_helpers_keep_offsets() {
        let range = text_range(10, 15);
        assert_eq!(range_len(range), 5);
        assert!(!range.is_empty());
    }

    #[test]
    fn token_constructor_keeps_fields() {
        let token = Token::new(TokenKind::Semi, text_range(1, 2));
        assert_eq!(token.kind, TokenKind::Semi);
        assert_eq!(token.range, text_range(1, 2));
    }

    #[test]
    fn lex_diagnostic_constructor_keeps_fields() {
        let diagnostic = LexDiagnostic::new("bad token", text_range(2, 4));
        assert_eq!(diagnostic.message, "bad token");
        assert_eq!(diagnostic.range, text_range(2, 4));
    }

    #[test]
    fn trivia_kinds_are_marked_as_trivia() {
        assert!(TokenKind::Whitespace.is_trivia());
        assert!(TokenKind::LineComment.is_trivia());
        assert!(TokenKind::BlockComment.is_trivia());
        assert!(!TokenKind::Ident.is_trivia());
    }
}
