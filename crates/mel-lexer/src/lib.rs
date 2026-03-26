#![forbid(unsafe_code)]
//! Minimal lexer scaffold for MEL.
//!
//! This implementation is intentionally small. It exists to anchor crate boundaries
//! and test flow until a richer lexer is introduced.

use mel_syntax::{LexDiagnostic, Lexed, Token, TokenKind, text_range};

pub struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    offset: usize,
    emitted_eof: bool,
    diagnostics: Vec<LexDiagnostic>,
    significant_only: bool,
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(input: &'a str) -> Self {
        Self::with_options(input, false)
    }

    #[must_use]
    pub fn significant(input: &'a str) -> Self {
        Self::with_options(input, true)
    }

    fn with_options(input: &'a str, significant_only: bool) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            offset: 0,
            emitted_eof: false,
            diagnostics: Vec::new(),
            significant_only,
        }
    }

    #[must_use]
    pub fn finish(self) -> Vec<LexDiagnostic> {
        self.diagnostics
    }

    fn next_token_internal(&mut self) -> Option<Token> {
        if self.emitted_eof {
            return None;
        }

        loop {
            if self.offset >= self.bytes.len() {
                self.emitted_eof = true;
                let eof = self.input.len() as u32;
                return Some(Token::new(TokenKind::Eof, text_range(eof, eof)));
            }

            let bytes = self.bytes;
            let mut i = self.offset;
            let token = match bytes[i] {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    let start = i;
                    i = lex_whitespace(bytes, i);
                    Token::new(TokenKind::Whitespace, text_range(start as u32, i as u32))
                }
                b'/' if matches!(bytes.get(i + 1), Some(b'/')) => {
                    let start = i;
                    i = lex_line_comment(bytes, i);
                    Token::new(TokenKind::LineComment, text_range(start as u32, i as u32))
                }
                b'/' if matches!(bytes.get(i + 1), Some(b'*')) => {
                    let start = i;
                    let (end, terminated) = lex_block_comment(bytes, i);
                    i = end;
                    if !terminated {
                        self.diagnostics.push(LexDiagnostic::new(
                            "unterminated block comment",
                            text_range(start as u32, end as u32),
                        ));
                    }
                    Token::new(
                        TokenKind::BlockComment,
                        text_range(start as u32, end as u32),
                    )
                }
                b';' => advance_token(TokenKind::Semi, i, i + 1, &mut i),
                b'(' => advance_token(TokenKind::LParen, i, i + 1, &mut i),
                b')' => advance_token(TokenKind::RParen, i, i + 1, &mut i),
                b'[' => advance_token(TokenKind::LBracket, i, i + 1, &mut i),
                b']' => advance_token(TokenKind::RBracket, i, i + 1, &mut i),
                b'{' => advance_token(TokenKind::LBrace, i, i + 1, &mut i),
                b'}' => advance_token(TokenKind::RBrace, i, i + 1, &mut i),
                b'.' if bytes
                    .get(i + 1)
                    .copied()
                    .is_some_and(|b| b.is_ascii_digit()) =>
                {
                    let start = i;
                    i += 1;
                    while bytes.get(i).copied().is_some_and(|b| b.is_ascii_digit()) {
                        i += 1;
                    }

                    if let Some(end) = lex_exponent_suffix(bytes, i) {
                        i = end;
                    }

                    Token::new(TokenKind::FloatLiteral, text_range(start as u32, i as u32))
                }
                b'.' => advance_token(TokenKind::Dot, i, i + 1, &mut i),
                b',' => advance_token(TokenKind::Comma, i, i + 1, &mut i),
                b'$' => advance_token(TokenKind::Dollar, i, i + 1, &mut i),
                b'`' => advance_token(TokenKind::Backquote, i, i + 1, &mut i),
                b'?' => advance_token(TokenKind::Question, i, i + 1, &mut i),
                b':' => advance_token(TokenKind::Colon, i, i + 1, &mut i),
                b'+' if matches!(bytes.get(i + 1), Some(b'=')) => {
                    advance_token(TokenKind::PlusEq, i, i + 2, &mut i)
                }
                b'+' if matches!(bytes.get(i + 1), Some(b'+')) => {
                    advance_token(TokenKind::PlusPlus, i, i + 2, &mut i)
                }
                b'+' => advance_token(TokenKind::Plus, i, i + 1, &mut i),
                b'*' if matches!(bytes.get(i + 1), Some(b'=')) => {
                    advance_token(TokenKind::StarEq, i, i + 2, &mut i)
                }
                b'*' => advance_token(TokenKind::Star, i, i + 1, &mut i),
                b'/' if matches!(bytes.get(i + 1), Some(b'=')) => {
                    advance_token(TokenKind::SlashEq, i, i + 2, &mut i)
                }
                b'/' => advance_token(TokenKind::Slash, i, i + 1, &mut i),
                b'%' => advance_token(TokenKind::Percent, i, i + 1, &mut i),
                b'^' => advance_token(TokenKind::Caret, i, i + 1, &mut i),
                b'!' if matches!(bytes.get(i + 1), Some(b'=')) => {
                    advance_token(TokenKind::NotEq, i, i + 2, &mut i)
                }
                b'!' => advance_token(TokenKind::Bang, i, i + 1, &mut i),
                b'=' if matches!(bytes.get(i + 1), Some(b'=')) => {
                    advance_token(TokenKind::EqEq, i, i + 2, &mut i)
                }
                b'=' => advance_token(TokenKind::Assign, i, i + 1, &mut i),
                b'<' if matches!(bytes.get(i + 1), Some(b'<')) => {
                    advance_token(TokenKind::LtLt, i, i + 2, &mut i)
                }
                b'<' if matches!(bytes.get(i + 1), Some(b'=')) => {
                    advance_token(TokenKind::Le, i, i + 2, &mut i)
                }
                b'<' => advance_token(TokenKind::Lt, i, i + 1, &mut i),
                b'>' if matches!(bytes.get(i + 1), Some(b'>')) => {
                    advance_token(TokenKind::GtGt, i, i + 2, &mut i)
                }
                b'>' if matches!(bytes.get(i + 1), Some(b'=')) => {
                    advance_token(TokenKind::Ge, i, i + 2, &mut i)
                }
                b'>' => advance_token(TokenKind::Gt, i, i + 1, &mut i),
                b'&' if matches!(bytes.get(i + 1), Some(b'&')) => {
                    advance_token(TokenKind::AndAnd, i, i + 2, &mut i)
                }
                b'|' if matches!(bytes.get(i + 1), Some(b'|')) => {
                    advance_token(TokenKind::OrOr, i, i + 2, &mut i)
                }
                b'|' => advance_token(TokenKind::Pipe, i, i + 1, &mut i),
                b'-' if matches!(bytes.get(i + 1), Some(b'-')) => {
                    advance_token(TokenKind::MinusMinus, i, i + 2, &mut i)
                }
                b'-' if matches!(bytes.get(i + 1), Some(b'=')) => {
                    advance_token(TokenKind::MinusEq, i, i + 2, &mut i)
                }
                b'-' if bytes.get(i + 1).copied().is_some_and(is_ident_start_byte)
                    && can_start_flag(bytes, i) =>
                {
                    let start = i;
                    i += 1;
                    while bytes.get(i).copied().is_some_and(is_ident_continue_byte) {
                        i += 1;
                    }
                    Token::new(TokenKind::Flag, text_range(start as u32, i as u32))
                }
                b'-' => advance_token(TokenKind::Minus, i, i + 1, &mut i),
                b'"' => {
                    let start = i;
                    i += 1;
                    let mut terminated = false;
                    while i < bytes.len() {
                        match bytes[i] {
                            b'\\' => {
                                i += if i + 1 < bytes.len() { 2 } else { 1 };
                            }
                            b'"' => {
                                i += 1;
                                terminated = true;
                                break;
                            }
                            _ => i += 1,
                        }
                    }
                    if !terminated {
                        self.diagnostics.push(LexDiagnostic::new(
                            "unterminated string literal",
                            text_range(start as u32, i as u32),
                        ));
                    }
                    Token::new(TokenKind::StringLiteral, text_range(start as u32, i as u32))
                }
                b'0'..=b'9' => {
                    let start = i;

                    if bytes[i] == b'0'
                        && matches!(bytes.get(i + 1), Some(b'x' | b'X'))
                        && bytes
                            .get(i + 2)
                            .copied()
                            .is_some_and(|b| b.is_ascii_hexdigit())
                    {
                        i += 2;
                        while bytes.get(i).copied().is_some_and(|b| b.is_ascii_hexdigit()) {
                            i += 1;
                        }
                        self.offset = i;
                        let token =
                            Token::new(TokenKind::IntLiteral, text_range(start as u32, i as u32));
                        if self.significant_only && token.kind.is_trivia() {
                            continue;
                        }
                        return Some(token);
                    }

                    i += 1;
                    while bytes.get(i).copied().is_some_and(|b| b.is_ascii_digit()) {
                        i += 1;
                    }

                    let mut kind = TokenKind::IntLiteral;

                    if matches!(bytes.get(i), Some(b'.')) {
                        if bytes
                            .get(i + 1)
                            .copied()
                            .is_some_and(|b| b.is_ascii_digit())
                        {
                            i += 1;
                            while bytes.get(i).copied().is_some_and(|b| b.is_ascii_digit()) {
                                i += 1;
                            }
                            kind = TokenKind::FloatLiteral;
                        } else if can_end_with_trailing_dot_float(bytes, i + 1) {
                            i += 1;
                            kind = TokenKind::FloatLiteral;
                        }
                    }

                    if let Some(end) = lex_exponent_suffix(bytes, i) {
                        i = end;
                        kind = TokenKind::FloatLiteral;
                    }

                    Token::new(kind, text_range(start as u32, i as u32))
                }
                b if is_ident_start_byte(b) => {
                    let start = i;
                    i += 1;
                    while bytes.get(i).copied().is_some_and(is_ident_continue_byte) {
                        i += 1;
                    }
                    Token::new(TokenKind::Ident, text_range(start as u32, i as u32))
                }
                _ => {
                    self.diagnostics.push(LexDiagnostic::new(
                        "unknown character",
                        text_range(i as u32, (i + 1) as u32),
                    ));
                    i += 1;
                    Token::new(TokenKind::Unknown, text_range((i - 1) as u32, i as u32))
                }
            };

            self.offset = i;
            if self.significant_only && token.kind.is_trivia() {
                continue;
            }
            return Some(token);
        }
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_token_internal()
    }
}

#[must_use]
pub fn lexer(input: &str) -> Lexer<'_> {
    Lexer::new(input)
}

#[must_use]
pub fn significant_lexer(input: &str) -> Lexer<'_> {
    Lexer::significant(input)
}

#[must_use]
pub fn lex(input: &str) -> Lexed {
    let mut lexer = lexer(input);
    let tokens = lexer.by_ref().collect();
    let diagnostics = lexer.finish();
    Lexed {
        tokens,
        diagnostics,
    }
}

#[must_use]
pub fn lex_significant(input: &str) -> Lexed {
    let mut lexer = significant_lexer(input);
    let tokens = lexer.by_ref().collect();
    let diagnostics = lexer.finish();
    Lexed {
        tokens,
        diagnostics,
    }
}

fn advance_token(kind: TokenKind, start: usize, end: usize, index: &mut usize) -> Token {
    *index = end;
    Token::new(kind, text_range(start as u32, end as u32))
}

fn lex_whitespace(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while matches!(bytes.get(i), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        i += 1;
    }
    i
}

fn lex_line_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while let Some(byte) = bytes.get(i) {
        if *byte == b'\n' {
            break;
        }
        i += 1;
    }
    i
}

fn lex_block_comment(bytes: &[u8], start: usize) -> (usize, bool) {
    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return (i + 2, true);
        }
        i += 1;
    }
    (bytes.len(), false)
}

fn can_start_flag(bytes: &[u8], index: usize) -> bool {
    index > 0 && bytes[index - 1].is_ascii_whitespace()
}

fn is_ident_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue_byte(byte: u8) -> bool {
    is_ident_start_byte(byte) || byte.is_ascii_digit()
}

fn can_end_with_trailing_dot_float(bytes: &[u8], index: usize) -> bool {
    match bytes.get(index).copied() {
        None => true,
        Some(byte) if byte.is_ascii_whitespace() => true,
        Some(
            b';' | b',' | b')' | b']' | b'}' | b'?' | b':' | b'+' | b'-' | b'*' | b'/' | b'%'
            | b'=' | b'!' | b'<' | b'>' | b'&' | b'|',
        ) => true,
        _ => false,
    }
}

fn lex_exponent_suffix(bytes: &[u8], start: usize) -> Option<usize> {
    let exponent = bytes.get(start).copied()?;
    if !matches!(exponent, b'e' | b'E') {
        return None;
    }

    let mut index = start + 1;
    if matches!(bytes.get(index), Some(b'+' | b'-')) {
        index += 1;
    }

    let first_digit = bytes.get(index).copied()?;
    if !first_digit.is_ascii_digit() {
        return None;
    }

    index += 1;
    while bytes
        .get(index)
        .copied()
        .is_some_and(|byte| byte.is_ascii_digit())
    {
        index += 1;
    }

    Some(index)
}

#[cfg(test)]
mod tests {
    use super::lex;
    use mel_syntax::{TokenKind, range_end, range_start, text_range};

    fn token_kinds(input: &str) -> Vec<TokenKind> {
        lex(input)
            .tokens
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    #[test]
    fn lexes_basic_statement() {
        let kinds = token_kinds(r#"$foo = 1;"#);
        assert_eq!(
            kinds,
            vec![
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::Assign,
                TokenKind::Whitespace,
                TokenKind::IntLiteral,
                TokenKind::Semi,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_compound_assignment_and_updates() {
        let kinds = token_kinds(r#"$foo += 1; $bar -= 2; $baz *= 3; $qux /= 4; $foo++; $foo--;"#);
        assert_eq!(
            kinds,
            vec![
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::PlusEq,
                TokenKind::Whitespace,
                TokenKind::IntLiteral,
                TokenKind::Semi,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::MinusEq,
                TokenKind::Whitespace,
                TokenKind::IntLiteral,
                TokenKind::Semi,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::StarEq,
                TokenKind::Whitespace,
                TokenKind::IntLiteral,
                TokenKind::Semi,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::SlashEq,
                TokenKind::Whitespace,
                TokenKind::IntLiteral,
                TokenKind::Semi,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::PlusPlus,
                TokenKind::Semi,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::MinusMinus,
                TokenKind::Semi,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_backquoted_command() {
        let kinds = token_kinds(r#"`ls -sl`;"#);
        assert_eq!(
            kinds,
            vec![
                TokenKind::Backquote,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::Flag,
                TokenKind::Backquote,
                TokenKind::Semi,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_minus_before_ident_in_expression_as_minus() {
        let kinds = token_kinds(r#"size($path)-size($sceneName);"#);
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::RParen,
                TokenKind::Minus,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::RParen,
                TokenKind::Semi,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn keeps_minus_ident_after_whitespace_as_flag() {
        let kinds = token_kinds("optionVar -q Foo;");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::Flag,
                TokenKind::Whitespace,
                TokenKind::Ident,
                TokenKind::Semi,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_exponent_float_literals() {
        let input = "1.0e-3 1e+3 0.0e0 1E-9";
        let lexed = lex(input);
        let texts: Vec<_> = lexed
            .tokens
            .iter()
            .filter(|token| !token.kind.is_trivia() && token.kind != TokenKind::Eof)
            .map(|token| {
                (
                    &input[range_start(token.range) as usize..range_end(token.range) as usize],
                    token.kind,
                )
            })
            .collect();

        assert_eq!(
            texts,
            vec![
                ("1.0e-3", TokenKind::FloatLiteral),
                ("1e+3", TokenKind::FloatLiteral),
                ("0.0e0", TokenKind::FloatLiteral),
                ("1E-9", TokenKind::FloatLiteral),
            ]
        );
    }

    #[test]
    fn lexes_leading_dot_float_literals() {
        let input = ".7 .001 .5e+2 .";
        let lexed = lex(input);
        let texts: Vec<_> = lexed
            .tokens
            .iter()
            .filter(|token| !token.kind.is_trivia() && token.kind != TokenKind::Eof)
            .map(|token| {
                (
                    &input[range_start(token.range) as usize..range_end(token.range) as usize],
                    token.kind,
                )
            })
            .collect();

        assert_eq!(
            texts,
            vec![
                (".7", TokenKind::FloatLiteral),
                (".001", TokenKind::FloatLiteral),
                (".5e+2", TokenKind::FloatLiteral),
                (".", TokenKind::Dot),
            ]
        );
    }

    #[test]
    fn lexes_trailing_dot_float_literals() {
        let input = "1000. 0. -1000. 1.. 1.foo";
        let lexed = lex(input);
        let texts: Vec<_> = lexed
            .tokens
            .iter()
            .filter(|token| !token.kind.is_trivia() && token.kind != TokenKind::Eof)
            .map(|token| {
                (
                    &input[range_start(token.range) as usize..range_end(token.range) as usize],
                    token.kind,
                )
            })
            .collect();

        assert_eq!(
            texts,
            vec![
                ("1000.", TokenKind::FloatLiteral),
                ("0.", TokenKind::FloatLiteral),
                ("-", TokenKind::Minus),
                ("1000.", TokenKind::FloatLiteral),
                ("1", TokenKind::IntLiteral),
                (".", TokenKind::Dot),
                (".", TokenKind::Dot),
                ("1", TokenKind::IntLiteral),
                (".", TokenKind::Dot),
                ("foo", TokenKind::Ident),
            ]
        );
    }

    #[test]
    fn lexes_hex_integer_literals() {
        let input = "0x8000 0X0001 42";
        let lexed = lex(input);
        let texts: Vec<_> = lexed
            .tokens
            .iter()
            .filter(|token| !token.kind.is_trivia() && token.kind != TokenKind::Eof)
            .map(|token| {
                (
                    &input[range_start(token.range) as usize..range_end(token.range) as usize],
                    token.kind,
                )
            })
            .collect();

        assert_eq!(
            texts,
            vec![
                ("0x8000", TokenKind::IntLiteral),
                ("0X0001", TokenKind::IntLiteral),
                ("42", TokenKind::IntLiteral),
            ]
        );
    }

    #[test]
    fn lexes_caret_operator() {
        let kinds = token_kinds("vector $cross = $a ^ $b;");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::Assign,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::Caret,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Semi,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn malformed_exponent_suffix_stays_split() {
        let kinds = token_kinds("1e+ 1.0e 1e-");
        assert_eq!(
            kinds,
            vec![
                TokenKind::IntLiteral,
                TokenKind::Ident,
                TokenKind::Plus,
                TokenKind::Whitespace,
                TokenKind::FloatLiteral,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::IntLiteral,
                TokenKind::Ident,
                TokenKind::Minus,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_vector_literals_and_components() {
        let kinds = token_kinds(r#"$dir = <<1, 2, 3>>; $x = $dir.x;"#);
        assert_eq!(
            kinds,
            vec![
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::Assign,
                TokenKind::Whitespace,
                TokenKind::LtLt,
                TokenKind::IntLiteral,
                TokenKind::Comma,
                TokenKind::Whitespace,
                TokenKind::IntLiteral,
                TokenKind::Comma,
                TokenKind::Whitespace,
                TokenKind::IntLiteral,
                TokenKind::GtGt,
                TokenKind::Semi,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::Assign,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::Semi,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_single_pipe_for_dag_paths() {
        let kinds = token_kinds("|pSphere1|pSphereShape1.instObjGroups[0]");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Pipe,
                TokenKind::Ident,
                TokenKind::Pipe,
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::LBracket,
                TokenKind::IntLiteral,
                TokenKind::RBracket,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn keeps_double_pipe_as_boolean_or() {
        let kinds = token_kinds("$a || $b");
        assert_eq!(
            kinds,
            vec![
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::OrOr,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn retains_trivia_tokens() {
        let kinds = token_kinds("// lead\n$foo /* mid */ = 1;");
        assert_eq!(
            kinds,
            vec![
                TokenKind::LineComment,
                TokenKind::Whitespace,
                TokenKind::Dollar,
                TokenKind::Ident,
                TokenKind::Whitespace,
                TokenKind::BlockComment,
                TokenKind::Whitespace,
                TokenKind::Assign,
                TokenKind::Whitespace,
                TokenKind::IntLiteral,
                TokenKind::Semi,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn unterminated_string_produces_diagnostic() {
        let lexed = lex("\"unterminated");
        assert_eq!(lexed.tokens.len(), 2);
        assert_eq!(lexed.tokens[0].kind, TokenKind::StringLiteral);
        assert_eq!(lexed.tokens[0].range, text_range(0, 13));
        assert_eq!(lexed.tokens[1].kind, TokenKind::Eof);
        assert_eq!(lexed.tokens[1].range, text_range(13, 13));
        assert_eq!(lexed.diagnostics.len(), 1);
        assert_eq!(lexed.diagnostics[0].message, "unterminated string literal");
        assert_eq!(lexed.diagnostics[0].range, text_range(0, 13));
    }

    #[test]
    fn unterminated_block_comment_produces_diagnostic() {
        let lexed = lex("/* unterminated");
        assert_eq!(lexed.tokens.len(), 2);
        assert_eq!(lexed.tokens[0].kind, TokenKind::BlockComment);
        assert_eq!(lexed.tokens[0].range, text_range(0, 15));
        assert_eq!(lexed.tokens[1].kind, TokenKind::Eof);
        assert_eq!(lexed.tokens[1].range, text_range(15, 15));
        assert_eq!(lexed.diagnostics.len(), 1);
        assert_eq!(lexed.diagnostics[0].message, "unterminated block comment");
        assert_eq!(lexed.diagnostics[0].range, text_range(0, 15));
    }

    #[test]
    fn unknown_character_produces_token_and_diagnostic() {
        let lexed = lex("@");
        assert_eq!(lexed.tokens.len(), 2);
        assert_eq!(lexed.tokens[0].kind, TokenKind::Unknown);
        assert_eq!(lexed.tokens[0].range, text_range(0, 1));
        assert_eq!(lexed.diagnostics.len(), 1);
        assert_eq!(lexed.diagnostics[0].message, "unknown character");
        assert_eq!(lexed.diagnostics[0].range, text_range(0, 1));
    }
}
