#![forbid(unsafe_code)]
//! Shared syntax primitives for the MEL parser workspace.

use std::ops::Range;
use std::sync::Arc;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceMapEdit {
    source_start: u32,
    source_end: u32,
    display_start: u32,
    display_end: u32,
}

impl SourceMapEdit {
    #[must_use]
    pub const fn new(
        source_start: u32,
        source_end: u32,
        display_start: u32,
        display_end: u32,
    ) -> Self {
        Self {
            source_start,
            source_end,
            display_start,
            display_end,
        }
    }

    #[must_use]
    pub const fn source_start(self) -> u32 {
        self.source_start
    }

    #[must_use]
    pub const fn source_end(self) -> u32 {
        self.source_end
    }

    #[must_use]
    pub const fn display_start(self) -> u32 {
        self.display_start
    }

    #[must_use]
    pub const fn display_end(self) -> u32 {
        self.display_end
    }

    #[must_use]
    pub const fn delta_after(self) -> i64 {
        self.display_end as i64 - self.source_end as i64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceMapKind {
    Identity {
        len: usize,
    },
    Indexed {
        source_to_display: Arc<[u32]>,
    },
    Sparse {
        source_len: usize,
        display_len: usize,
        edits: Arc<[SourceMapEdit]>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    kind: SourceMapKind,
}

impl SourceMap {
    #[must_use]
    pub fn identity(len: usize) -> Self {
        Self {
            kind: SourceMapKind::Identity { len },
        }
    }

    #[must_use]
    pub fn from_source_to_display(source_to_display: Vec<u32>) -> Self {
        Self::from_shared_source_to_display(source_to_display.into())
    }

    #[must_use]
    pub fn from_shared_source_to_display(source_to_display: Arc<[u32]>) -> Self {
        if source_to_display
            .iter()
            .enumerate()
            .all(|(offset, mapped)| *mapped == u32::try_from(offset).unwrap_or(u32::MAX))
        {
            return Self::identity(source_to_display.len().saturating_sub(1));
        }
        Self {
            kind: SourceMapKind::Indexed { source_to_display },
        }
    }

    #[must_use]
    pub fn from_sparse_edits(
        source_len: usize,
        display_len: usize,
        edits: Arc<[SourceMapEdit]>,
    ) -> Self {
        if source_len == display_len && edits.is_empty() {
            return Self::identity(source_len);
        }
        Self {
            kind: SourceMapKind::Sparse {
                source_len,
                display_len,
                edits,
            },
        }
    }

    #[must_use]
    pub fn display_offset(&self, offset: u32) -> usize {
        match &self.kind {
            SourceMapKind::Identity { len } => usize::try_from(offset).unwrap_or(*len).min(*len),
            SourceMapKind::Indexed { source_to_display } => source_to_display
                .get(offset as usize)
                .copied()
                .or_else(|| source_to_display.last().copied())
                .unwrap_or(offset)
                as usize,
            SourceMapKind::Sparse {
                source_len,
                display_len,
                edits,
            } => sparse_source_to_display(*source_len, *display_len, edits, offset),
        }
    }

    #[must_use]
    pub fn display_range(&self, range: TextRange) -> Range<usize> {
        self.display_offset(range_start(range))..self.display_offset(range_end(range))
    }

    #[must_use]
    pub fn source_offset_for_display(&self, display_offset: usize) -> u32 {
        match &self.kind {
            SourceMapKind::Identity { len } => {
                u32::try_from(display_offset.min(*len)).unwrap_or(u32::MAX)
            }
            SourceMapKind::Indexed { source_to_display } => {
                match source_to_display
                    .binary_search_by(|mapped| mapped.cmp(&(display_offset as u32)))
                {
                    Ok(mut index) => {
                        while index + 1 < source_to_display.len()
                            && source_to_display[index + 1] <= display_offset as u32
                        {
                            index += 1;
                        }
                        u32::try_from(index).unwrap_or(u32::MAX)
                    }
                    Err(0) => 0,
                    Err(index) => u32::try_from(index - 1).unwrap_or(u32::MAX),
                }
            }
            SourceMapKind::Sparse {
                source_len,
                display_len,
                edits,
            } => sparse_display_to_source(*source_len, *display_len, edits, display_offset),
        }
    }

    #[must_use]
    pub fn source_range_from_display_range(&self, range: Range<usize>) -> TextRange {
        text_range(
            self.source_offset_for_display(range.start),
            self.source_offset_for_display(range.end),
        )
    }
}

fn sparse_source_to_display(
    source_len: usize,
    display_len: usize,
    edits: &[SourceMapEdit],
    offset: u32,
) -> usize {
    let clamped = usize::try_from(offset)
        .unwrap_or(source_len)
        .min(source_len) as u32;
    let Some(index) = edits
        .partition_point(|edit| edit.source_start() <= clamped)
        .checked_sub(1)
    else {
        return clamped as usize;
    };
    let edit = edits[index];
    if clamped == edit.source_start() {
        return edit.display_start() as usize;
    }
    if clamped <= edit.source_end() {
        return edit.display_end() as usize;
    }
    let mapped = (clamped as i64 + edit.delta_after()).clamp(0, display_len as i64);
    mapped as usize
}

fn sparse_display_to_source(
    source_len: usize,
    display_len: usize,
    edits: &[SourceMapEdit],
    offset: usize,
) -> u32 {
    let clamped = offset.min(display_len) as u32;
    let Some(index) = edits
        .partition_point(|edit| edit.display_start() <= clamped)
        .checked_sub(1)
    else {
        return clamped;
    };
    let edit = edits[index];
    if clamped == edit.display_start() {
        return edit.source_start();
    }
    if clamped <= edit.display_end() {
        return edit.source_end();
    }
    let mapped = (clamped as i64 - edit.delta_after()).clamp(0, source_len as i64);
    mapped as u32
}

#[derive(Debug, Clone, Copy)]
pub struct SourceView<'a> {
    text: &'a str,
    source_map: &'a SourceMap,
}

impl<'a> SourceView<'a> {
    #[must_use]
    pub fn new(text: &'a str, source_map: &'a SourceMap) -> Self {
        Self { text, source_map }
    }

    #[must_use]
    pub fn text(self) -> &'a str {
        self.text
    }

    #[must_use]
    pub fn source_map(self) -> &'a SourceMap {
        self.source_map
    }

    #[must_use]
    pub fn display_range(self, range: TextRange) -> Range<usize> {
        self.source_map.display_range(range)
    }

    #[must_use]
    pub fn display_slice(self, range: TextRange) -> &'a str {
        &self.text[self.display_range(range)]
    }

    #[must_use]
    pub fn slice(self, range: TextRange) -> &'a str {
        self.display_slice(range)
    }

    #[must_use]
    pub fn source_range_from_display_range(self, range: Range<usize>) -> TextRange {
        self.source_map.source_range_from_display_range(range)
    }
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
    pub message: &'static str,
    pub range: TextRange,
}

impl LexDiagnostic {
    #[must_use]
    pub const fn new(message: &'static str, range: TextRange) -> Self {
        Self { message, range }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<LexDiagnostic>,
}

#[cfg(test)]
mod tests {
    use super::{LexDiagnostic, SourceMap, SourceMapEdit, Token, TokenKind, range_len, text_range};

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

    #[test]
    fn source_map_can_map_display_offsets_back_to_source_offsets() {
        let map = SourceMap::from_source_to_display(vec![0, 3, 3, 4]);
        assert_eq!(map.source_offset_for_display(0), 0);
        assert_eq!(map.source_offset_for_display(3), 2);
        assert_eq!(map.source_offset_for_display(4), 3);
        assert_eq!(map.source_range_from_display_range(0..3), text_range(0, 2));
    }

    #[test]
    fn identity_source_map_avoids_index_materialization() {
        let map = SourceMap::identity(8);
        assert_eq!(map.display_offset(3), 3);
        assert_eq!(map.display_offset(99), 8);
        assert_eq!(map.source_offset_for_display(5), 5);
        assert_eq!(map.source_offset_for_display(99), 8);
        assert_eq!(map.source_range_from_display_range(2..6), text_range(2, 6));
    }

    #[test]
    fn sparse_source_map_handles_positive_delta() {
        let map = SourceMap::from_sparse_edits(4, 5, vec![SourceMapEdit::new(1, 2, 1, 3)].into());
        assert_eq!(map.display_offset(0), 0);
        assert_eq!(map.display_offset(1), 1);
        assert_eq!(map.display_offset(2), 3);
        assert_eq!(map.display_offset(4), 5);
        assert_eq!(map.source_offset_for_display(0), 0);
        assert_eq!(map.source_offset_for_display(1), 1);
        assert_eq!(map.source_offset_for_display(2), 2);
        assert_eq!(map.source_offset_for_display(3), 2);
        assert_eq!(map.source_offset_for_display(5), 4);
    }

    #[test]
    fn sparse_source_map_handles_negative_delta() {
        let map = SourceMap::from_sparse_edits(6, 4, vec![SourceMapEdit::new(1, 5, 1, 3)].into());
        assert_eq!(map.display_offset(0), 0);
        assert_eq!(map.display_offset(1), 1);
        assert_eq!(map.display_offset(2), 3);
        assert_eq!(map.display_offset(5), 3);
        assert_eq!(map.display_offset(6), 4);
        assert_eq!(map.source_offset_for_display(0), 0);
        assert_eq!(map.source_offset_for_display(1), 1);
        assert_eq!(map.source_offset_for_display(2), 5);
        assert_eq!(map.source_offset_for_display(3), 5);
        assert_eq!(map.source_offset_for_display(4), 6);
    }
}
