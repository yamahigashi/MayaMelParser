#![forbid(unsafe_code)]
//! Minimal parser scaffold.
//!
//! This parser keeps the MEL surface intentionally small, but it now supports
//! byte-safe file inputs, a Pratt expression layer, command-style invocations,
//! indexing, and the first loop statements.

use encoding_rs::{Encoding, GBK, SHIFT_JIS};
use std::{borrow::Cow, fs, io, ops::Range, path::Path};

use mel_ast::{
    AssignOp, BinaryOp, CalleeResolution, Declarator, Expr, InvokeExpr, InvokeSurface, Item,
    ProcDef, ProcParam, ProcReturnType, ShellWord, SourceFile, Stmt, SwitchClause, SwitchLabel,
    TypeName, UnaryOp, UpdateOp, VarDecl, VectorComponent,
};
use mel_lexer::lex;
use mel_syntax::{LexDiagnostic, TextRange, Token, TokenKind, range_end, range_start, text_range};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeDiagnostic {
    pub message: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceEncoding {
    Utf8,
    Cp932,
    Gbk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParseMode {
    #[default]
    Strict,
    AllowTrailingStmtWithoutSemi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParseOptions {
    pub mode: ParseMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    pub syntax: SourceFile,
    pub source_text: String,
    pub source_map: SourceMap,
    pub source_encoding: SourceEncoding,
    pub decode_errors: Vec<DecodeDiagnostic>,
    pub lex_errors: Vec<LexDiagnostic>,
    pub errors: Vec<ParseError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    source_to_display: Vec<u32>,
}

impl SourceMap {
    fn identity(len: usize) -> Self {
        let source_to_display = (0..=len)
            .map(|offset| u32::try_from(offset).unwrap_or(u32::MAX))
            .collect();
        Self { source_to_display }
    }

    fn from_offset_map(offset_map: &OffsetMap) -> Self {
        Self {
            source_to_display: offset_map.source_to_decoded.clone(),
        }
    }

    #[must_use]
    pub fn display_offset(&self, offset: u32) -> usize {
        self.source_to_display
            .get(offset as usize)
            .copied()
            .or_else(|| self.source_to_display.last().copied())
            .unwrap_or(offset) as usize
    }

    #[must_use]
    pub fn display_range(&self, range: TextRange) -> Range<usize> {
        self.display_offset(range_start(range))..self.display_offset(range_end(range))
    }
}

#[must_use]
pub fn parse_source(input: &str) -> Parse {
    parse_source_with_options(input, ParseOptions::default())
}

#[must_use]
pub fn parse_source_with_options(input: &str, options: ParseOptions) -> Parse {
    let mut parse = Parser::new(input, options).parse();
    parse.source_text = input.to_owned();
    parse.source_map = SourceMap::identity(input.len());
    parse.source_encoding = SourceEncoding::Utf8;
    parse.decode_errors = Vec::new();
    parse
}

#[must_use]
pub fn parse_bytes(input: &[u8]) -> Parse {
    let decoded = decode_source_auto(input);
    let source_text = decoded.text.into_owned();
    let mut parse = parse_source(&source_text);
    parse.source_text = source_text;
    parse.source_map = SourceMap::from_offset_map(&decoded.offset_map);
    parse.source_encoding = decoded.encoding;
    remap_parse_ranges(&mut parse, &decoded.offset_map);
    parse.decode_errors = decoded.diagnostics;
    parse
}

#[must_use]
pub fn parse_bytes_with_encoding(input: &[u8], encoding: SourceEncoding) -> Parse {
    let decoded = decode_source_with_encoding(input, encoding);
    let source_text = decoded.text.into_owned();
    let mut parse = parse_source(&source_text);
    parse.source_text = source_text;
    parse.source_map = SourceMap::from_offset_map(&decoded.offset_map);
    parse.source_encoding = decoded.encoding;
    remap_parse_ranges(&mut parse, &decoded.offset_map);
    parse.decode_errors = decoded.diagnostics;
    parse
}

pub fn parse_file(path: impl AsRef<Path>) -> io::Result<Parse> {
    let bytes = fs::read(path)?;
    Ok(parse_bytes(&bytes))
}

pub fn parse_file_with_encoding(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
) -> io::Result<Parse> {
    let bytes = fs::read(path)?;
    Ok(parse_bytes_with_encoding(&bytes, encoding))
}

struct DecodedSource<'a> {
    encoding: SourceEncoding,
    text: Cow<'a, str>,
    offset_map: OffsetMap,
    diagnostics: Vec<DecodeDiagnostic>,
}

#[derive(Debug, Clone)]
struct OffsetMap {
    decoded_to_source: Vec<u32>,
    source_to_decoded: Vec<u32>,
}

impl OffsetMap {
    fn identity(len: usize) -> Self {
        let boundaries: Vec<u32> = (0..=len)
            .map(|offset| u32::try_from(offset).unwrap_or(u32::MAX))
            .collect();
        Self {
            decoded_to_source: boundaries.clone(),
            source_to_decoded: boundaries,
        }
    }

    fn from_decoded_text(text: &str, source_len: usize, encoding: SourceEncoding) -> Option<Self> {
        let mut decoded_to_source = vec![0; text.len() + 1];
        let mut source_to_decoded = vec![0; source_len + 1];
        let mut decoded_offset = 0usize;
        let mut source_offset = 0usize;

        for ch in text.chars() {
            let decoded_len = ch.len_utf8();
            let source_char_len = source_len_for_char(ch, encoding)?;
            let source_end = source_offset.saturating_add(source_char_len);
            let decoded_end = decoded_offset.saturating_add(decoded_len);
            for step in 1..=decoded_len {
                decoded_to_source[decoded_offset + step] =
                    u32::try_from(source_end).unwrap_or(u32::MAX);
            }
            for step in 1..=source_char_len {
                source_to_decoded[source_offset + step] =
                    u32::try_from(decoded_end).unwrap_or(u32::MAX);
            }
            decoded_offset += decoded_len;
            source_offset = source_end;
        }

        if source_offset != source_len {
            return None;
        }

        decoded_to_source[text.len()] = u32::try_from(source_len).unwrap_or(u32::MAX);
        source_to_decoded[source_len] = u32::try_from(text.len()).unwrap_or(u32::MAX);
        Some(Self {
            decoded_to_source,
            source_to_decoded,
        })
    }

    fn map_offset(&self, offset: u32) -> u32 {
        self.decoded_to_source
            .get(offset as usize)
            .copied()
            .or_else(|| self.decoded_to_source.last().copied())
            .unwrap_or(offset)
    }

    fn map_range(&self, range: TextRange) -> TextRange {
        text_range(
            self.map_offset(range_start(range)),
            self.map_offset(range_end(range)),
        )
    }
}

fn decode_source_auto(input: &[u8]) -> DecodedSource<'_> {
    if let Ok(text) = std::str::from_utf8(input) {
        return DecodedSource {
            encoding: SourceEncoding::Utf8,
            text: Cow::Borrowed(text),
            offset_map: OffsetMap::identity(text.len()),
            diagnostics: Vec::new(),
        };
    }

    for encoding in [SourceEncoding::Cp932, SourceEncoding::Gbk] {
        let decoded = decode_source_with_encoding(input, encoding);
        if decoded.diagnostics.is_empty() {
            return decoded;
        }
    }

    decode_lossy_utf8(input)
}

fn decode_source_with_encoding(input: &[u8], encoding: SourceEncoding) -> DecodedSource<'_> {
    if matches!(encoding, SourceEncoding::Utf8) {
        return match std::str::from_utf8(input) {
            Ok(text) => DecodedSource {
                encoding,
                text: Cow::Borrowed(text),
                offset_map: OffsetMap::identity(text.len()),
                diagnostics: Vec::new(),
            },
            Err(error) => decode_lossy_utf8_with_error(input, error.valid_up_to() as u32, error),
        };
    }

    let (text, _, had_errors) = encoding_rs_encoding(encoding).decode(input);
    let offset_map = OffsetMap::from_decoded_text(text.as_ref(), input.len(), encoding)
        .unwrap_or_else(|| OffsetMap::identity(text.len()));
    let diagnostics = if had_errors {
        vec![DecodeDiagnostic {
            message: format!(
                "source is not valid {}; decoded with replacement",
                encoding.label()
            ),
            range: text_range(0, input.len() as u32),
        }]
    } else {
        Vec::new()
    };

    DecodedSource {
        encoding,
        text,
        offset_map,
        diagnostics,
    }
}

fn decode_lossy_utf8(input: &[u8]) -> DecodedSource<'_> {
    match std::str::from_utf8(input) {
        Ok(text) => DecodedSource {
            encoding: SourceEncoding::Utf8,
            text: Cow::Borrowed(text),
            offset_map: OffsetMap::identity(text.len()),
            diagnostics: Vec::new(),
        },
        Err(error) => decode_lossy_utf8_with_error(input, error.valid_up_to() as u32, error),
    }
}

fn decode_lossy_utf8_with_error(
    input: &[u8],
    start: u32,
    error: std::str::Utf8Error,
) -> DecodedSource<'_> {
    let end = error
        .error_len()
        .map_or(input.len() as u32, |len| start + len as u32);
    let (text, offset_map) = decode_lossy_utf8_text_and_offset_map(input);

    DecodedSource {
        encoding: SourceEncoding::Utf8,
        offset_map,
        text: Cow::Owned(text),
        diagnostics: vec![DecodeDiagnostic {
            message: "source is not valid UTF-8; decoded lossily".to_owned(),
            range: text_range(start, end),
        }],
    }
}

fn decode_lossy_utf8_text_and_offset_map(input: &[u8]) -> (String, OffsetMap) {
    let mut text = String::new();
    let mut decoded_to_source = vec![0];
    let mut source_to_decoded = vec![0; input.len() + 1];
    let mut source_offset = 0usize;

    while source_offset < input.len() {
        match std::str::from_utf8(&input[source_offset..]) {
            Ok(valid) => {
                for ch in valid.chars() {
                    append_decoded_char_mapping(
                        &mut text,
                        &mut decoded_to_source,
                        &mut source_to_decoded,
                        source_offset,
                        ch.len_utf8(),
                        ch,
                    );
                    source_offset += ch.len_utf8();
                }
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    let valid =
                        std::str::from_utf8(&input[source_offset..source_offset + valid_up_to])
                            .unwrap_or_default();
                    for ch in valid.chars() {
                        append_decoded_char_mapping(
                            &mut text,
                            &mut decoded_to_source,
                            &mut source_to_decoded,
                            source_offset,
                            ch.len_utf8(),
                            ch,
                        );
                        source_offset += ch.len_utf8();
                    }
                }

                let invalid_len = error.error_len().unwrap_or(input.len() - source_offset);
                append_decoded_char_mapping(
                    &mut text,
                    &mut decoded_to_source,
                    &mut source_to_decoded,
                    source_offset,
                    invalid_len,
                    char::REPLACEMENT_CHARACTER,
                );
                source_offset += invalid_len;
            }
        }
    }

    (
        text,
        OffsetMap {
            decoded_to_source,
            source_to_decoded,
        },
    )
}

fn append_decoded_char_mapping(
    text: &mut String,
    decoded_to_source: &mut Vec<u32>,
    source_to_decoded: &mut [u32],
    source_start: usize,
    source_len: usize,
    ch: char,
) {
    let decoded_start = text.len();
    let source_end = source_start + source_len;

    text.push(ch);
    let decoded_end = text.len();
    decoded_to_source.resize(decoded_end + 1, source_end as u32);
    for mapped in decoded_to_source
        .iter_mut()
        .take(decoded_end + 1)
        .skip(decoded_start + 1)
    {
        *mapped = source_end as u32;
    }

    for mapped in source_to_decoded
        .iter_mut()
        .take(source_end + 1)
        .skip(source_start + 1)
    {
        *mapped = decoded_end as u32;
    }
}

impl SourceEncoding {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Cp932 => "cp932",
            Self::Gbk => "gbk",
        }
    }
}

fn encoding_rs_encoding(encoding: SourceEncoding) -> &'static Encoding {
    match encoding {
        SourceEncoding::Utf8 => encoding_rs::UTF_8,
        SourceEncoding::Cp932 => SHIFT_JIS,
        SourceEncoding::Gbk => GBK,
    }
}

fn source_len_for_char(ch: char, encoding: SourceEncoding) -> Option<usize> {
    if matches!(encoding, SourceEncoding::Utf8) {
        return Some(ch.len_utf8());
    }

    let mut text = String::new();
    text.push(ch);
    let (encoded, _, had_errors) = encoding_rs_encoding(encoding).encode(&text);
    (!had_errors).then(|| encoded.len())
}

struct Parser<'a> {
    input: &'a str,
    options: ParseOptions,
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<ParseError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StmtContext {
    TopLevel,
    Nested,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, options: ParseOptions) -> Self {
        Self {
            input,
            options,
            tokens: Vec::new(),
            pos: 0,
            errors: Vec::new(),
        }
    }

    fn parse(mut self) -> Parse {
        let lexed = lex(self.input);
        self.tokens = lexed.tokens;

        let mut items = Vec::new();
        while !self.at(TokenKind::Eof) {
            if let Some(item) = self.parse_item() {
                items.push(item);
            } else {
                let range = self.current().range;
                self.error("unexpected token while parsing item", range);
                self.bump();
            }
        }

        Parse {
            syntax: SourceFile { items },
            source_text: String::new(),
            source_map: SourceMap::identity(self.input.len()),
            source_encoding: SourceEncoding::Utf8,
            decode_errors: Vec::new(),
            lex_errors: lexed.diagnostics,
            errors: self.errors,
        }
    }

    fn parse_item(&mut self) -> Option<Item> {
        if self.at_keyword("global") && self.peek_keyword() == Some("proc") {
            return self.parse_proc_def().map(Box::new).map(Item::Proc);
        }

        if self.at_keyword("proc") {
            return self.parse_proc_def().map(Box::new).map(Item::Proc);
        }

        self.parse_stmt(StmtContext::TopLevel)
            .map(Box::new)
            .map(Item::Stmt)
    }

    fn parse_proc_def(&mut self) -> Option<ProcDef> {
        let start = range_start(self.current().range);
        let is_global = self.eat_keyword("global").is_some();
        self.eat_keyword("proc")?;
        let return_type = self.parse_proc_return_type();
        let name_token = self.eat(TokenKind::Ident);

        let name_token = if let Some(token) = name_token {
            token
        } else {
            let range = self.current().range;
            self.error("expected proc name before parameter list", range);
            return Some(ProcDef {
                return_type,
                name: String::new(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: Vec::new(),
                    range,
                },
                is_global,
                range: text_range(start, range_end(range)),
            });
        };

        let mut params = Vec::new();
        if self.eat(TokenKind::LParen).is_some() {
            if !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                loop {
                    match self.parse_proc_param() {
                        Some(param) => params.push(param),
                        None => {
                            self.error("expected proc parameter", self.current().range);
                            self.recover_to_proc_param_boundary();
                        }
                    }

                    if self.eat(TokenKind::Comma).is_some() {
                        continue;
                    }
                    break;
                }
            }

            if self.eat(TokenKind::RParen).is_none() {
                let range = self.current().range;
                self.error("expected ')' after proc parameter list", range);
            }
        } else {
            let range = self.current().range;
            self.error("expected '(' after proc name", range);
        }

        let body = if let Some(stmt) = self.parse_block_stmt() {
            stmt
        } else {
            let range = self.current().range;
            self.error("expected proc body block", range);
            let end = self
                .eat(TokenKind::Semi)
                .map_or(range_end(range), |semi| range_end(semi.range));
            Stmt::Block {
                statements: Vec::new(),
                range: text_range(range_start(range), end),
            }
        };
        let end = range_end(stmt_range(&body));

        Some(ProcDef {
            return_type,
            name: self.token_text(name_token).to_owned(),
            params,
            body,
            is_global,
            range: text_range(start, end),
        })
    }

    fn parse_proc_return_type(&mut self) -> Option<ProcReturnType> {
        if !self.at(TokenKind::Ident) || !is_type_keyword(self.token_text(self.current())) {
            return None;
        }

        let has_array_suffix = self.peek_kind() == Some(TokenKind::LBracket)
            && self.nth_kind_after_current(2) == Some(TokenKind::RBracket)
            && self.nth_kind_after_current(3) == Some(TokenKind::Ident);
        let has_scalar_suffix = self.peek_kind() == Some(TokenKind::Ident);

        if !has_scalar_suffix && !has_array_suffix {
            return None;
        }

        let type_token = self.bump();
        let ty = parse_type_name(self.token_text(type_token))?;
        let mut end = range_end(type_token.range);
        let mut is_array = false;

        if has_array_suffix {
            is_array = true;
            end = self.parse_proc_return_array_suffix();
        }

        Some(ProcReturnType {
            ty,
            is_array,
            range: text_range(range_start(type_token.range), end),
        })
    }

    fn parse_proc_param(&mut self) -> Option<ProcParam> {
        let type_token = self.eat(TokenKind::Ident)?;
        let ty = parse_type_name(self.token_text(type_token))?;

        let start = range_start(type_token.range);
        let dollar = if let Some(token) = self.eat(TokenKind::Dollar) {
            token
        } else {
            let range = self.current().range;
            self.error("expected '$' before proc parameter name", range);
            return Some(ProcParam {
                ty,
                name: String::new(),
                is_array: false,
                range: type_token.range,
            });
        };

        let ident = if let Some(token) = self.eat(TokenKind::Ident) {
            token
        } else {
            let range = self.current().range;
            self.error("expected identifier after '$'", range);
            return Some(ProcParam {
                ty,
                name: self.token_text(dollar).to_owned(),
                is_array: false,
                range: text_range(start, range_end(dollar.range)),
            });
        };

        let mut end = range_end(ident.range);
        let mut is_array = false;
        if self.at(TokenKind::LBracket) {
            is_array = true;
            end = self.parse_proc_param_array_suffix();
        }

        Some(ProcParam {
            ty,
            name: format!("{}{}", self.token_text(dollar), self.token_text(ident)),
            is_array,
            range: text_range(start, end),
        })
    }

    fn parse_proc_param_array_suffix(&mut self) -> u32 {
        let open = self.bump();
        if !self.at(TokenKind::RBracket) {
            let range = self.current().range;
            self.error("proc parameter arrays cannot specify a size", range);
            while !self.at(TokenKind::RBracket)
                && !self.at(TokenKind::Comma)
                && !self.at(TokenKind::RParen)
                && !self.at(TokenKind::Eof)
            {
                self.bump();
            }
        }

        if let Some(close) = self.eat(TokenKind::RBracket) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected ']' after proc parameter array suffix", range);
            range_end(open.range).max(range_end(range))
        }
    }

    fn parse_proc_return_array_suffix(&mut self) -> u32 {
        let open = self.bump();
        if !self.at(TokenKind::RBracket) {
            let range = self.current().range;
            self.error("proc return arrays cannot specify a size", range);
            while !self.at(TokenKind::RBracket)
                && !self.at(TokenKind::Ident)
                && !self.at(TokenKind::Eof)
            {
                self.bump();
            }
        }

        if let Some(close) = self.eat(TokenKind::RBracket) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected ']' after proc return array suffix", range);
            range_end(open.range).max(range_end(range))
        }
    }

    fn parse_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        if let Some(token) = self.eat(TokenKind::Semi) {
            return Some(Stmt::Empty { range: token.range });
        }

        if self.at_keyword("global") && self.peek_keyword() == Some("proc") {
            let proc_def = self.parse_proc_def()?;
            let range = proc_def.range;
            return Some(Stmt::Proc {
                proc_def: Box::new(proc_def),
                range,
            });
        }

        if self.at_keyword("proc") {
            let proc_def = self.parse_proc_def()?;
            let range = proc_def.range;
            return Some(Stmt::Proc {
                proc_def: Box::new(proc_def),
                range,
            });
        }

        if self.at(TokenKind::LBrace) {
            return self.parse_block_stmt();
        }

        if self.at_keyword("if") {
            return self.parse_if_stmt();
        }

        if self.at_keyword("while") {
            return self.parse_while_stmt();
        }

        if self.at_keyword("do") {
            return self.parse_do_while_stmt();
        }

        if self.at_keyword("switch") {
            return self.parse_switch_stmt();
        }

        if self.at_keyword("for") {
            return self.parse_for_stmt();
        }

        if self.starts_var_decl() {
            return self.parse_var_decl_stmt(context);
        }

        if self.at_keyword("return") {
            return self.parse_return_stmt(context);
        }

        if self.at_keyword("break") {
            return self.parse_break_stmt(context);
        }

        if self.at_keyword("continue") {
            return self.parse_continue_stmt(context);
        }

        if self.starts_command_stmt() {
            return self.parse_command_stmt(context);
        }

        self.parse_expr_stmt(context)
    }

    fn parse_block_stmt(&mut self) -> Option<Stmt> {
        let open = self.eat(TokenKind::LBrace)?;
        let mut statements = Vec::new();

        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if let Some(stmt) = self.parse_stmt(StmtContext::Nested) {
                statements.push(stmt);
            } else {
                let range = self.current().range;
                self.error("unexpected token in block", range);
                self.bump();
            }
        }

        let end = if let Some(close) = self.eat(TokenKind::RBrace) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected '}' to close block", range);
            range_end(range)
        };

        Some(Stmt::Block {
            statements,
            range: text_range(range_start(open.range), end),
        })
    }

    fn parse_if_stmt(&mut self) -> Option<Stmt> {
        let if_token = self.eat_keyword("if")?;
        self.expect(TokenKind::LParen, "expected '(' after if")?;
        let condition = self.parse_expr()?;

        if self.eat(TokenKind::RParen).is_none() {
            let range = self.current().range;
            self.error("expected ')' after if condition", range);
        }

        let then_branch = self.parse_stmt(StmtContext::Nested)?;
        let else_branch = if self.eat_keyword("else").is_some() {
            self.parse_stmt(StmtContext::Nested).map(Box::new)
        } else {
            None
        };

        let end = else_branch
            .as_deref()
            .map(stmt_range)
            .unwrap_or_else(|| stmt_range(&then_branch));
        let end = range_end(end);

        Some(Stmt::If {
            condition,
            then_branch: Box::new(then_branch),
            else_branch,
            range: text_range(range_start(if_token.range), end),
        })
    }

    fn parse_while_stmt(&mut self) -> Option<Stmt> {
        let while_token = self.eat_keyword("while")?;
        self.expect(TokenKind::LParen, "expected '(' after while")?;

        let condition = if self.at(TokenKind::RParen) {
            let range = self.current().range;
            self.error("expected while condition", range);
            Expr::Ident {
                name: String::new(),
                range,
            }
        } else if let Some(expr) = self.parse_expr() {
            expr
        } else {
            let range = self.current().range;
            self.error("expected while condition", range);
            Expr::Ident {
                name: String::new(),
                range,
            }
        };

        if self.eat(TokenKind::RParen).is_none() {
            let range = self.current().range;
            self.error("expected ')' after while condition", range);
        }

        let body = if let Some(stmt) = self.parse_stmt(StmtContext::Nested) {
            stmt
        } else {
            let range = self.current().range;
            self.error("expected while body", range);
            Stmt::Empty { range }
        };

        Some(Stmt::While {
            condition,
            range: text_range(range_start(while_token.range), range_end(stmt_range(&body))),
            body: Box::new(body),
        })
    }

    fn parse_do_while_stmt(&mut self) -> Option<Stmt> {
        let do_token = self.eat_keyword("do")?;
        let body = if let Some(stmt) = self.parse_stmt(StmtContext::Nested) {
            stmt
        } else {
            let range = self.current().range;
            self.error("expected do-while body", range);
            Stmt::Empty { range }
        };
        let body_end = range_end(stmt_range(&body));

        let while_token = if let Some(token) = self.eat_keyword("while") {
            token
        } else {
            let range = self.current().range;
            self.error("expected 'while' after do body", range);
            return Some(Stmt::DoWhile {
                body: Box::new(body),
                condition: Expr::Ident {
                    name: String::new(),
                    range,
                },
                range: text_range(range_start(do_token.range), body_end.max(range_end(range))),
            });
        };

        self.expect(TokenKind::LParen, "expected '(' after while")?;

        let condition = if self.at(TokenKind::RParen) {
            let range = self.current().range;
            self.error("expected do-while condition", range);
            Expr::Ident {
                name: String::new(),
                range,
            }
        } else if let Some(expr) = self.parse_expr() {
            expr
        } else {
            let range = self.current().range;
            self.error("expected do-while condition", range);
            Expr::Ident {
                name: String::new(),
                range,
            }
        };

        let mut end = range_end(condition.range()).max(range_end(while_token.range));
        if let Some(close) = self.eat(TokenKind::RParen) {
            end = range_end(close.range);
        } else {
            let range = self.current().range;
            self.error("expected ')' after do-while condition", range);
            end = end.max(range_end(range));
        }

        if let Some(semi) = self.eat(TokenKind::Semi) {
            end = range_end(semi.range);
        } else {
            let range = self.current().range;
            self.error("expected ';' after do-while statement", range);
            end = end.max(range_end(range));
        }

        Some(Stmt::DoWhile {
            body: Box::new(body),
            condition,
            range: text_range(range_start(do_token.range), end),
        })
    }

    fn parse_switch_stmt(&mut self) -> Option<Stmt> {
        let switch_token = self.eat_keyword("switch")?;
        self.expect(TokenKind::LParen, "expected '(' after switch")?;

        let control = if self.at(TokenKind::RParen) {
            let range = self.current().range;
            self.error("expected switch control expression", range);
            Expr::Ident {
                name: String::new(),
                range,
            }
        } else if let Some(expr) = self.parse_expr() {
            expr
        } else {
            let range = self.current().range;
            self.error("expected switch control expression", range);
            Expr::Ident {
                name: String::new(),
                range,
            }
        };

        if self.eat(TokenKind::RParen).is_none() {
            let range = self.current().range;
            self.error("expected ')' after switch control expression", range);
        }

        let block_start = if let Some(open) = self.eat(TokenKind::LBrace) {
            range_start(open.range)
        } else {
            let range = self.current().range;
            self.error("expected '{' after switch clause", range);
            range_start(range)
        };

        let mut clauses = Vec::new();
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if self.at_keyword("case") || self.at_keyword("default") {
                if let Some(clause) = self.parse_switch_clause() {
                    clauses.push(clause);
                } else {
                    self.recover_to_switch_clause_boundary();
                }
                continue;
            }

            let range = self.current().range;
            self.error("expected 'case' or 'default' in switch body", range);
            self.recover_to_switch_clause_boundary();
            if self.current().range == range {
                self.bump();
            }
        }

        let end = if let Some(close) = self.eat(TokenKind::RBrace) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected '}' to close switch statement", range);
            range_end(range).max(block_start)
        };

        Some(Stmt::Switch {
            control,
            clauses,
            range: text_range(range_start(switch_token.range), end),
        })
    }

    fn parse_switch_clause(&mut self) -> Option<SwitchClause> {
        let start = range_start(self.current().range);
        let (label, label_end) = if self.at_keyword("case") {
            self.bump();
            let value = if self.at(TokenKind::Colon) {
                let range = self.current().range;
                self.error("expected case value", range);
                Expr::Ident {
                    name: String::new(),
                    range,
                }
            } else if let Some(expr) = self.parse_expr() {
                expr
            } else {
                let range = self.current().range;
                self.error("expected case value", range);
                Expr::Ident {
                    name: String::new(),
                    range,
                }
            };

            let end = if let Some(colon) = self.eat(TokenKind::Colon) {
                range_end(colon.range)
            } else {
                let range = self.current().range;
                self.error("expected ':' after switch label", range);
                range_end(value.range()).max(range_end(range))
            };

            (SwitchLabel::Case(value), end)
        } else if self.at_keyword("default") {
            let default_token = self.bump();
            let end = if let Some(colon) = self.eat(TokenKind::Colon) {
                range_end(colon.range)
            } else {
                let range = self.current().range;
                self.error("expected ':' after switch label", range);
                range_end(default_token.range).max(range_end(range))
            };

            (
                SwitchLabel::Default {
                    range: text_range(range_start(default_token.range), end),
                },
                end,
            )
        } else {
            return None;
        };

        let mut statements = Vec::new();
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::RBrace)
            && !self.at_keyword("case")
            && !self.at_keyword("default")
        {
            if let Some(stmt) = self.parse_stmt(StmtContext::Nested) {
                statements.push(stmt);
            } else {
                let range = self.current().range;
                self.error("unexpected token in switch clause", range);
                self.bump();
            }
        }

        let end = statements
            .last()
            .map(stmt_range)
            .map(range_end)
            .unwrap_or(label_end);

        Some(SwitchClause {
            label,
            statements,
            range: text_range(start, end),
        })
    }

    fn parse_for_stmt(&mut self) -> Option<Stmt> {
        let for_token = self.eat_keyword("for")?;
        self.expect(TokenKind::LParen, "expected '(' after for")?;

        let checkpoint = self.pos;
        if let Some(binding) = self.parse_expr()
            && self.eat_keyword("in").is_some()
        {
            let iterable = if let Some(expr) = self.parse_expr() {
                expr
            } else {
                let range = self.current().range;
                self.error("expected iterable expression after 'in'", range);
                return None;
            };

            let close = self.expect(TokenKind::RParen, "expected ')' after for-in clause")?;
            let body = self.parse_stmt(StmtContext::Nested)?;
            let body_end = range_end(stmt_range(&body)).max(range_end(close.range));
            return Some(Stmt::ForIn {
                binding,
                iterable,
                body: Box::new(body),
                range: text_range(range_start(for_token.range), body_end),
            });
        }

        self.pos = checkpoint;

        let init = if self.at(TokenKind::Semi) {
            None
        } else {
            self.parse_for_clause_exprs()
        };
        self.expect(TokenKind::Semi, "expected ';' after for init")?;

        let condition = if self.at(TokenKind::Semi) {
            None
        } else {
            self.parse_expr()
        };
        self.expect(TokenKind::Semi, "expected ';' after for condition")?;

        let update = if self.at(TokenKind::RParen) {
            None
        } else {
            self.parse_for_clause_exprs()
        };
        let close = self.expect(TokenKind::RParen, "expected ')' after for clause")?;
        let body = self.parse_stmt(StmtContext::Nested)?;

        let body_end = range_end(stmt_range(&body)).max(range_end(close.range));
        Some(Stmt::For {
            init,
            condition,
            update,
            body: Box::new(body),
            range: text_range(range_start(for_token.range), body_end),
        })
    }

    fn parse_for_clause_exprs(&mut self) -> Option<Vec<Expr>> {
        let mut exprs = Vec::new();

        loop {
            let expr = self.parse_expr()?;
            exprs.push(expr);

            if self.eat(TokenKind::Comma).is_none() {
                break;
            }

            if self.at(TokenKind::Semi) || self.at(TokenKind::RParen) {
                self.error(
                    "expected expression after ',' in for clause",
                    self.current().range,
                );
                break;
            }
        }

        Some(exprs)
    }

    fn parse_return_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let token = self.eat_keyword("return")?;
        let expr = if self.at(TokenKind::Semi) {
            None
        } else {
            self.parse_expr()
        };
        let end = self.expect_stmt_terminator("expected ';' after return statement", context)?;
        Some(Stmt::Return {
            expr,
            range: text_range(range_start(token.range), end),
        })
    }

    fn parse_break_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let token = self.eat_keyword("break")?;
        let end = self.expect_stmt_terminator("expected ';' after break statement", context)?;
        Some(Stmt::Break {
            range: text_range(range_start(token.range), end),
        })
    }

    fn parse_continue_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let token = self.eat_keyword("continue")?;
        let end = self.expect_stmt_terminator("expected ';' after continue statement", context)?;
        Some(Stmt::Continue {
            range: text_range(range_start(token.range), end),
        })
    }

    fn parse_command_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let start = range_start(self.current().range);
        let invoke = self.parse_shell_like_invoke(false)?;
        let end = self.expect_stmt_terminator("expected ';' after statement", context)?;
        Some(Stmt::Expr {
            expr: Expr::Invoke(invoke),
            range: text_range(start, end),
        })
    }

    fn parse_expr_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let start = range_start(self.current().range);
        let expr = self.parse_expr()?;
        let end = self.expect_stmt_terminator("expected ';' after statement", context)?;
        Some(Stmt::Expr {
            expr,
            range: text_range(start, end),
        })
    }

    fn starts_var_decl(&self) -> bool {
        if self.at_keyword("global") {
            if self.peek_keyword() == Some("proc") {
                return false;
            }

            return self.peek_keyword().is_some_and(is_type_keyword);
        }

        self.at(TokenKind::Ident) && is_type_keyword(self.token_text(self.current()))
    }

    fn starts_command_stmt(&self) -> bool {
        self.at(TokenKind::Ident) && !self.starts_function_stmt()
    }

    fn starts_function_stmt(&self) -> bool {
        if !self.at(TokenKind::Ident) || self.peek_kind() != Some(TokenKind::LParen) {
            return false;
        }

        let head_index = self.current_index();
        let open_index = self.next_significant_index(head_index + 1);
        if self.has_line_break_between(head_index, open_index) {
            return false;
        }

        let Some(close_index) = self.matching_rparen_index(open_index) else {
            return true;
        };

        let next_index = self.next_significant_index(close_index + 1);
        let next_kind = self.token_at(next_index).kind;
        next_kind == TokenKind::Semi
            || matches!(next_kind, TokenKind::Eof | TokenKind::RBrace)
            || (self.has_line_break_between(close_index, next_index)
                && self.starts_stmt_after_function_args(next_index))
    }

    fn parse_var_decl_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let start = range_start(self.current().range);
        let is_global = self.eat_keyword("global").is_some();
        let type_token = self.eat(TokenKind::Ident)?;
        let ty = parse_type_name(self.token_text(type_token))?;
        let mut declarators = Vec::new();

        loop {
            match self.parse_declarator() {
                Some(declarator) => declarators.push(declarator),
                None => {
                    self.error("expected variable declarator", self.current().range);
                    self.recover_to_decl_boundary();
                }
            }

            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            break;
        }

        let end =
            self.expect_stmt_terminator("expected ';' after variable declaration", context)?;
        let range = text_range(start, end);
        Some(Stmt::VarDecl {
            decl: VarDecl {
                is_global,
                ty,
                declarators,
                range,
            },
            range,
        })
    }

    fn parse_declarator(&mut self) -> Option<Declarator> {
        let dollar = self.eat(TokenKind::Dollar)?;
        let ident = if let Some(token) = self.eat(TokenKind::Ident) {
            token
        } else {
            self.error("expected identifier after '$'", self.current().range);
            return Some(Declarator {
                name: self.token_text(dollar).to_owned(),
                array_size: None,
                initializer: None,
                range: dollar.range,
            });
        };

        let mut end = range_end(ident.range);
        let mut array_size = None;
        if self.at(TokenKind::LBracket) {
            let (size, suffix_end) = self.parse_decl_array_suffix();
            array_size = Some(size);
            end = suffix_end;
        }

        let initializer = if self.eat(TokenKind::Assign).is_some() {
            match self.parse_expr() {
                Some(expr) => {
                    end = range_end(expr.range());
                    Some(expr)
                }
                None => {
                    self.error("expected expression after '='", self.current().range);
                    None
                }
            }
        } else {
            None
        };

        Some(Declarator {
            name: format!("{}{}", self.token_text(dollar), self.token_text(ident)),
            array_size,
            initializer,
            range: text_range(range_start(dollar.range), end),
        })
    }

    fn parse_decl_array_suffix(&mut self) -> (Option<Expr>, u32) {
        let open = self.bump();
        let size = if self.at(TokenKind::RBracket) {
            None
        } else {
            self.parse_expr()
        };
        let end = if let Some(close) = self.eat(TokenKind::RBracket) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected ']' after declaration array suffix", range);
            range_end(range).max(range_end(open.range))
        };

        (size, end)
    }

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_expr_bp(0)
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.parse_prefix_expr()?;

        loop {
            if self.at(TokenKind::Dot) {
                let l_bp = 90;
                if l_bp < min_bp {
                    break;
                }

                self.bump();
                if self.current().kind != TokenKind::Ident {
                    self.error("expected member name after '.'", self.current().range);
                    return Some(lhs);
                }

                let member_token = self.bump();
                let member_name = self.token_text(member_token).to_owned();
                let range = text_range(range_start(lhs.range()), range_end(member_token.range));

                lhs = if let Some(component) = parse_vector_component_name(&member_name) {
                    Expr::ComponentAccess {
                        range,
                        target: Box::new(lhs),
                        component,
                    }
                } else {
                    Expr::MemberAccess {
                        range,
                        target: Box::new(lhs),
                        member: member_name,
                    }
                };
                continue;
            }

            if self.at(TokenKind::LBracket) {
                let l_bp = 90;
                if l_bp < min_bp {
                    break;
                }

                let open = self.bump();
                let index = if let Some(expr) = self.parse_expr() {
                    expr
                } else {
                    let range = self.current().range;
                    self.error("expected expression inside index", range);
                    return Some(lhs);
                };

                let end = if let Some(close) = self.eat(TokenKind::RBracket) {
                    range_end(close.range)
                } else {
                    let range = self.current().range;
                    self.error("expected ']' after index expression", range);
                    range_end(range)
                };

                let lhs_start = range_start(lhs.range());
                lhs = Expr::Index {
                    target: Box::new(lhs),
                    index: Box::new(index),
                    range: text_range(range_start(open.range).min(lhs_start), end),
                };
                continue;
            }

            if let Some(op) = self.parse_postfix_update_op() {
                let l_bp = 90;
                if l_bp < min_bp {
                    break;
                }
                let end = range_end(self.previous_range());
                lhs = Expr::PostfixUpdate {
                    op,
                    range: text_range(range_start(lhs.range()), end),
                    expr: Box::new(lhs),
                };
                continue;
            }

            if self.at(TokenKind::Question) {
                let l_bp = 15;
                if l_bp < min_bp {
                    break;
                }

                let question = self.bump();
                let then_expr = if let Some(expr) = self.parse_expr_bp(0) {
                    expr
                } else {
                    self.error("expected expression after '?'", self.current().range);
                    return Some(lhs);
                };

                if self.eat(TokenKind::Colon).is_none() {
                    self.error("expected ':' in ternary expression", self.current().range);
                    return Some(lhs);
                }

                let else_expr = if let Some(expr) = self.parse_expr_bp(l_bp) {
                    expr
                } else {
                    self.error("expected expression after ':'", self.current().range);
                    return Some(lhs);
                };

                lhs = Expr::Ternary {
                    range: text_range(
                        range_start(lhs.range()),
                        range_end(else_expr.range()).max(range_end(question.range)),
                    ),
                    condition: Box::new(lhs),
                    then_expr: Box::new(then_expr),
                    else_expr: Box::new(else_expr),
                };
                continue;
            }

            let Some((l_bp, r_bp, kind)) = infix_binding_power(self.current().kind) else {
                break;
            };

            if l_bp < min_bp {
                break;
            }

            let op_token = self.bump();
            let rhs = if let Some(expr) = self.parse_expr_bp(r_bp) {
                expr
            } else {
                self.error("expected expression after operator", self.current().range);
                return Some(lhs);
            };

            let range = text_range(
                range_start(lhs.range()),
                range_end(rhs.range()).max(range_end(op_token.range)),
            );
            lhs = match kind {
                InfixKind::Binary(op) => Expr::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    range,
                },
                InfixKind::Assign(op) => Expr::Assign {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    range,
                },
            };
        }

        Some(lhs)
    }

    fn parse_prefix_expr(&mut self) -> Option<Expr> {
        if self.at_cast_expr() {
            return self.parse_cast_expr();
        }

        if let Some(token) = self.eat(TokenKind::PlusPlus) {
            return self.parse_prefix_update_expr(token, UpdateOp::Increment);
        }

        if let Some(token) = self.eat(TokenKind::MinusMinus) {
            return self.parse_prefix_update_expr(token, UpdateOp::Decrement);
        }

        if let Some(token) = self.eat(TokenKind::Bang) {
            return self.parse_unary_expr(token, UnaryOp::Not);
        }

        if let Some(token) = self.eat(TokenKind::Minus) {
            return self.parse_unary_expr(token, UnaryOp::Negate);
        }

        self.parse_atom()
    }

    fn parse_prefix_update_expr(&mut self, token: Token, op: UpdateOp) -> Option<Expr> {
        let expr = if let Some(expr) = self.parse_expr_bp(80) {
            expr
        } else {
            self.error(
                "expected expression after prefix update",
                self.current().range,
            );
            return None;
        };

        Some(Expr::PrefixUpdate {
            op,
            range: text_range(range_start(token.range), range_end(expr.range())),
            expr: Box::new(expr),
        })
    }

    fn parse_unary_expr(&mut self, token: Token, op: UnaryOp) -> Option<Expr> {
        let expr = if let Some(expr) = self.parse_expr_bp(80) {
            expr
        } else {
            self.error(
                "expected expression after unary operator",
                self.current().range,
            );
            return None;
        };

        Some(Expr::Unary {
            op,
            range: text_range(range_start(token.range), range_end(expr.range())),
            expr: Box::new(expr),
        })
    }

    fn parse_atom(&mut self) -> Option<Expr> {
        match self.current().kind {
            TokenKind::Dollar => self.parse_variable_expr(),
            TokenKind::Ident if self.peek_kind() == Some(TokenKind::LParen) => {
                self.parse_function_invoke().map(Expr::Invoke)
            }
            TokenKind::Ident if self.at_path_like_bareword_expr() => {
                self.parse_path_like_bareword_expr()
            }
            TokenKind::Pipe | TokenKind::Star | TokenKind::Colon => {
                self.parse_path_like_bareword_expr()
            }
            TokenKind::Ident => {
                let token = self.bump();
                Some(Expr::Ident {
                    name: self.token_text(token).to_owned(),
                    range: token.range,
                })
            }
            TokenKind::IntLiteral => {
                let token = self.bump();
                let value = match parse_int_literal_text(self.token_text(token)) {
                    Ok(value) => value,
                    Err(IntLiteralError::OutOfRange) => {
                        self.error("integer literal out of range", token.range);
                        0
                    }
                };
                Some(Expr::Int {
                    value,
                    range: token.range,
                })
            }
            TokenKind::FloatLiteral => {
                let token = self.bump();
                Some(Expr::Float {
                    text: self.token_text(token).to_owned(),
                    range: token.range,
                })
            }
            TokenKind::StringLiteral => {
                let token = self.bump();
                Some(Expr::String {
                    text: self.token_text(token).to_owned(),
                    range: token.range,
                })
            }
            TokenKind::LtLt => self.parse_vector_literal_expr(),
            TokenKind::LBrace => self.parse_brace_list_expr(),
            TokenKind::Backquote => self.parse_backquoted_invoke().map(Expr::Invoke),
            TokenKind::LParen => self.parse_grouped_expr(),
            _ => None,
        }
    }

    fn parse_path_like_bareword_expr(&mut self) -> Option<Expr> {
        let start_index = self.current_index();
        let end_index = self.scan_path_like_bareword_end(start_index)?;
        if start_index == end_index && self.token_at(start_index).kind == TokenKind::Ident {
            let token = self.bump();
            return Some(Expr::Ident {
                name: self.token_text(token).to_owned(),
                range: token.range,
            });
        }

        let start = range_start(self.token_at(start_index).range);
        let end = range_end(self.token_at(end_index).range);
        let range = text_range(start, end);
        self.pos = end_index + 1;

        Some(Expr::BareWord {
            text: self.input[start as usize..end as usize].to_owned(),
            range,
        })
    }

    fn at_path_like_bareword_expr(&self) -> bool {
        let start_index = self.current_index();
        let start_kind = self.token_at(start_index).kind;
        self.scan_path_like_bareword_end(start_index)
            .is_some_and(|end_index| {
                start_kind != TokenKind::Ident
                    || (start_index..=end_index).any(|index| {
                        matches!(
                            self.tokens.get(index).map(|token| token.kind),
                            Some(TokenKind::Pipe | TokenKind::Colon)
                        )
                    })
            })
    }

    fn parse_variable_expr(&mut self) -> Option<Expr> {
        let dollar = self.eat(TokenKind::Dollar)?;
        let ident = if let Some(token) = self.eat(TokenKind::Ident) {
            token
        } else {
            self.error("expected identifier after '$'", self.current().range);
            return Some(Expr::Ident {
                name: self.token_text(dollar).to_owned(),
                range: dollar.range,
            });
        };

        Some(Expr::Ident {
            name: format!("{}{}", self.token_text(dollar), self.token_text(ident)),
            range: text_range(range_start(dollar.range), range_end(ident.range)),
        })
    }

    fn at_cast_expr(&self) -> bool {
        if !self.at(TokenKind::LParen) {
            return false;
        }

        let open_index = self.current_index();
        let type_index = self.next_significant_index(open_index + 1);
        let close_index = self.next_significant_index(type_index + 1);

        let type_token = self.token_at(type_index);
        let close_token = self.token_at(close_index);
        type_token.kind == TokenKind::Ident
            && is_type_keyword(self.token_text(type_token))
            && close_token.kind == TokenKind::RParen
    }

    fn parse_cast_expr(&mut self) -> Option<Expr> {
        let open = self.eat(TokenKind::LParen)?;
        let type_token = self.eat(TokenKind::Ident)?;
        let ty = parse_type_name(self.token_text(type_token))?;
        let close = self.expect(TokenKind::RParen, "expected ')' after cast type")?;

        let expr = if let Some(expr) = self.parse_expr_bp(80) {
            expr
        } else {
            let range = self.current().range;
            self.error("expected expression after cast", range);
            Expr::Ident {
                name: String::new(),
                range,
            }
        };

        Some(Expr::Cast {
            ty,
            range: text_range(
                range_start(open.range),
                range_end(expr.range()).max(range_end(close.range)),
            ),
            expr: Box::new(expr),
        })
    }

    fn parse_grouped_expr(&mut self) -> Option<Expr> {
        self.eat(TokenKind::LParen)?;
        let expr = self.parse_expr()?;
        if self.eat(TokenKind::RParen).is_none() {
            self.error(
                "expected ')' to close grouped expression",
                self.current().range,
            );
            self.recover_to_rparen_or_stmt_boundary();
            let _ = self.eat(TokenKind::RParen);
        }
        Some(expr)
    }

    fn parse_brace_list_expr(&mut self) -> Option<Expr> {
        let open = self.eat(TokenKind::LBrace)?;
        let mut elements = Vec::new();

        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            if let Some(expr) = self.parse_expr() {
                elements.push(expr);
            } else {
                self.error(
                    "expected expression inside brace list",
                    self.current().range,
                );
                self.recover_to_brace_list_boundary();
            }

            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            break;
        }

        let end = if let Some(close) = self.eat(TokenKind::RBrace) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected '}' to close brace list", range);
            range_end(range).max(range_end(open.range))
        };

        Some(Expr::ArrayLiteral {
            elements,
            range: text_range(range_start(open.range), end),
        })
    }

    fn parse_vector_literal_expr(&mut self) -> Option<Expr> {
        let open = self.eat(TokenKind::LtLt)?;
        let mut elements = Vec::new();

        while !self.at(TokenKind::GtGt) && !self.at(TokenKind::Eof) {
            if let Some(expr) = self.parse_expr() {
                elements.push(expr);
            } else {
                self.error(
                    "expected expression inside vector literal",
                    self.current().range,
                );
                self.recover_to_vector_literal_boundary();
            }

            if self.eat(TokenKind::Comma).is_some() {
                continue;
            }
            break;
        }

        let end = if let Some(close) = self.eat(TokenKind::GtGt) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected '>>' to close vector literal", range);
            range_end(range).max(range_end(open.range))
        };

        Some(Expr::VectorLiteral {
            elements,
            range: text_range(range_start(open.range), end),
        })
    }

    fn parse_function_invoke(&mut self) -> Option<InvokeExpr> {
        let name_token = self.eat(TokenKind::Ident)?;
        let name = self.token_text(name_token).to_owned();
        let _open = self.eat(TokenKind::LParen)?;

        let mut args = Vec::new();
        if !self.at(TokenKind::RParen) {
            loop {
                if let Some(expr) = self.parse_expr() {
                    args.push(expr);
                } else {
                    self.error(
                        "expected expression as function argument",
                        self.current().range,
                    );
                    break;
                }

                if self.eat(TokenKind::Comma).is_some() {
                    continue;
                }
                break;
            }
        }

        let end = if let Some(close) = self.eat(TokenKind::RParen) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected ')' to close function invocation", range);
            self.recover_to_rparen_or_stmt_boundary();
            if let Some(close) = self.eat(TokenKind::RParen) {
                range_end(close.range)
            } else {
                range_end(self.previous_range()).max(range_end(range))
            }
        };

        Some(InvokeExpr {
            surface: InvokeSurface::Function { name, args },
            resolution: CalleeResolution::Unresolved,
            range: text_range(range_start(name_token.range), end),
        })
    }

    fn parse_shell_like_invoke(&mut self, captured: bool) -> Option<InvokeExpr> {
        let head_token = self.eat(TokenKind::Ident)?;
        let head = self.token_text(head_token).to_owned();
        let mut words = Vec::new();

        while !self.at(TokenKind::Eof) && !self.at_shell_terminator(captured) {
            if captured && self.at_captured_shell_recovery_boundary() {
                break;
            }

            if self.at(TokenKind::Flag) {
                let flag = self.bump();
                words.push(ShellWord::Flag {
                    text: self.token_text(flag).to_owned(),
                    range: flag.range,
                });
                continue;
            }

            let Some(word) = self.parse_shell_word(captured) else {
                let error_index = self.current_index();
                self.error(
                    "unexpected token in command invocation",
                    self.current().range,
                );
                self.recover_to_shell_word_boundary(captured);
                if self.current_index() == error_index && !self.at_shell_terminator(captured) {
                    self.bump();
                }
                continue;
            };
            words.push(word);
        }

        let end = range_end(self.previous_range()).max(range_end(head_token.range));
        Some(InvokeExpr {
            surface: InvokeSurface::ShellLike {
                head,
                words,
                captured,
            },
            resolution: CalleeResolution::Unresolved,
            range: text_range(range_start(head_token.range), end),
        })
    }

    fn parse_backquoted_invoke(&mut self) -> Option<InvokeExpr> {
        let open = self.eat(TokenKind::Backquote)?;

        let invoke = if self.current().kind == TokenKind::Ident {
            self.parse_shell_like_invoke(true).unwrap_or(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: String::new(),
                    words: Vec::new(),
                    captured: true,
                },
                resolution: CalleeResolution::Unresolved,
                range: open.range,
            })
        } else {
            self.error(
                "expected command name after backquote",
                self.current().range,
            );
            InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: String::new(),
                    words: Vec::new(),
                    captured: true,
                },
                resolution: CalleeResolution::Unresolved,
                range: open.range,
            }
        };

        let end = if let Some(close) = self.eat(TokenKind::Backquote) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected closing backquote", range);
            range_end(range)
        };

        Some(InvokeExpr {
            range: text_range(range_start(open.range), end),
            ..invoke
        })
    }

    fn at_shell_terminator(&self, captured: bool) -> bool {
        if captured {
            self.at(TokenKind::Backquote)
        } else {
            self.at(TokenKind::Semi) || self.at(TokenKind::RBrace)
        }
    }

    fn at_captured_shell_recovery_boundary(&self) -> bool {
        self.at(TokenKind::RParen) || self.at(TokenKind::Semi) || self.at(TokenKind::RBrace)
    }

    fn parse_shell_word(&mut self, captured: bool) -> Option<ShellWord> {
        match self.current().kind {
            TokenKind::Dot if self.peek_kind() == Some(TokenKind::Dot) => {
                self.parse_punct_bareword_shell_word()
            }
            TokenKind::StringLiteral => {
                let token = self.bump();
                Some(ShellWord::QuotedString {
                    text: self.token_text(token).to_owned(),
                    range: token.range,
                })
            }
            TokenKind::IntLiteral | TokenKind::FloatLiteral => self.parse_numeric_shell_word(),
            TokenKind::Dot
                if self.peek_kind().is_some_and(|kind| {
                    matches!(kind, TokenKind::IntLiteral | TokenKind::FloatLiteral)
                }) =>
            {
                self.parse_numeric_shell_word()
            }
            TokenKind::Minus
                if matches!(
                    self.peek_kind(),
                    Some(TokenKind::IntLiteral | TokenKind::FloatLiteral)
                ) =>
            {
                self.parse_numeric_shell_word()
            }
            TokenKind::Minus if self.peek_kind() == Some(TokenKind::Ident) => {
                self.parse_spaced_flag_shell_word()
            }
            TokenKind::LBrace => self.parse_brace_list_shell_word(),
            TokenKind::LtLt => self.parse_vector_literal_shell_word(),
            TokenKind::Dollar => self.parse_variable_shell_word(),
            TokenKind::Backquote if !captured => self.parse_capture_shell_word(),
            TokenKind::LParen => self.parse_grouped_shell_word(),
            TokenKind::Ident | TokenKind::Pipe | TokenKind::Star | TokenKind::Colon => {
                self.parse_path_like_bareword_shell_word()
            }
            _ => None,
        }
    }

    fn parse_path_like_bareword_shell_word(&mut self) -> Option<ShellWord> {
        let start_index = self.current_index();
        let end_index = self.scan_shell_path_like_bareword_end(start_index)?;

        let start = range_start(self.token_at(start_index).range);
        let end = range_end(self.token_at(end_index).range);
        let range = text_range(start, end);
        self.pos = end_index + 1;

        Some(ShellWord::BareWord {
            text: self.input[start as usize..end as usize].to_owned(),
            range,
        })
    }

    fn scan_shell_path_like_bareword_end(&self, start_index: usize) -> Option<usize> {
        if !matches!(
            self.token_at(start_index).kind,
            TokenKind::Ident | TokenKind::Pipe | TokenKind::Star | TokenKind::Colon
        ) {
            return None;
        }

        let mut end_index = start_index;
        let mut index = start_index + 1;
        let mut expecting_segment_start = false;

        match self.token_at(start_index).kind {
            TokenKind::Pipe | TokenKind::Colon => {
                end_index = start_index;
                expecting_segment_start = true;
            }
            TokenKind::Ident | TokenKind::Star => {
                while matches!(
                    self.tokens.get(index).map(|token| token.kind),
                    Some(TokenKind::Ident | TokenKind::Star)
                ) {
                    end_index = index;
                    index += 1;
                }

                while matches!(
                    (
                        self.tokens.get(index).map(|token| token.kind),
                        self.tokens.get(index + 1).map(|token| token.kind)
                    ),
                    (
                        Some(TokenKind::Colon),
                        Some(TokenKind::Ident | TokenKind::Star)
                    )
                ) {
                    end_index = index + 1;
                    index += 2;
                    while matches!(
                        self.tokens.get(index).map(|token| token.kind),
                        Some(TokenKind::Ident | TokenKind::Star)
                    ) {
                        end_index = index;
                        index += 1;
                    }
                }
            }
            _ => return None,
        }

        loop {
            if expecting_segment_start {
                let mut consumed_atom = false;
                while matches!(
                    self.tokens.get(index).map(|token| token.kind),
                    Some(TokenKind::Ident | TokenKind::Star)
                ) {
                    end_index = index;
                    index += 1;
                    consumed_atom = true;
                }

                if !consumed_atom {
                    return None;
                }

                while matches!(
                    (
                        self.tokens.get(index).map(|token| token.kind),
                        self.tokens.get(index + 1).map(|token| token.kind)
                    ),
                    (
                        Some(TokenKind::Colon),
                        Some(TokenKind::Ident | TokenKind::Star)
                    )
                ) {
                    end_index = index + 1;
                    index += 2;
                    while matches!(
                        self.tokens.get(index).map(|token| token.kind),
                        Some(TokenKind::Ident | TokenKind::Star)
                    ) {
                        end_index = index;
                        index += 1;
                    }
                }

                expecting_segment_start = false;
                continue;
            }

            if matches!(
                self.tokens.get(index).map(|token| token.kind),
                Some(TokenKind::Pipe)
            ) {
                end_index = index;
                index += 1;
                expecting_segment_start = true;
                continue;
            }

            if matches!(
                (
                    self.tokens.get(index).map(|token| token.kind),
                    self.tokens.get(index + 1).map(|token| token.kind)
                ),
                (Some(TokenKind::Dot), Some(TokenKind::Ident))
            ) {
                end_index = index + 1;
                index += 2;
                continue;
            }

            if let Some(suffix_end) = self.bareword_bracket_suffix_end(index) {
                end_index = suffix_end;
                index = suffix_end + 1;
                continue;
            }

            while matches!(
                self.tokens.get(index).map(|token| token.kind),
                Some(TokenKind::Ident | TokenKind::Star)
            ) {
                end_index = index;
                index += 1;
            }

            break;
        }

        if expecting_segment_start {
            return None;
        }

        Some(end_index)
    }

    fn scan_path_like_bareword_end(&self, start_index: usize) -> Option<usize> {
        if !matches!(
            self.token_at(start_index).kind,
            TokenKind::Ident | TokenKind::Pipe | TokenKind::Star | TokenKind::Colon
        ) {
            return None;
        }

        let mut end_index = start_index;
        let mut index = start_index + 1;
        let mut expecting_segment_start = false;

        match self.token_at(start_index).kind {
            TokenKind::Pipe | TokenKind::Colon => {
                end_index = start_index;
                expecting_segment_start = true;
            }
            TokenKind::Ident | TokenKind::Star => {
                while matches!(
                    self.tokens.get(index).map(|token| token.kind),
                    Some(TokenKind::Ident | TokenKind::Star)
                ) {
                    end_index = index;
                    index += 1;
                }

                while matches!(
                    (
                        self.tokens.get(index).map(|token| token.kind),
                        self.tokens.get(index + 1).map(|token| token.kind)
                    ),
                    (
                        Some(TokenKind::Colon),
                        Some(TokenKind::Ident | TokenKind::Star)
                    )
                ) {
                    end_index = index + 1;
                    index += 2;
                    while matches!(
                        self.tokens.get(index).map(|token| token.kind),
                        Some(TokenKind::Ident | TokenKind::Star)
                    ) {
                        end_index = index;
                        index += 1;
                    }
                }
            }
            _ => return None,
        }

        loop {
            if expecting_segment_start {
                let mut consumed_atom = false;
                while matches!(
                    self.tokens.get(index).map(|token| token.kind),
                    Some(TokenKind::Ident | TokenKind::Star)
                ) {
                    end_index = index;
                    index += 1;
                    consumed_atom = true;
                }

                if !consumed_atom {
                    return (self.token_at(end_index).kind == TokenKind::Pipe).then_some(end_index);
                }

                while matches!(
                    (
                        self.tokens.get(index).map(|token| token.kind),
                        self.tokens.get(index + 1).map(|token| token.kind)
                    ),
                    (
                        Some(TokenKind::Colon),
                        Some(TokenKind::Ident | TokenKind::Star)
                    )
                ) {
                    end_index = index + 1;
                    index += 2;
                    while matches!(
                        self.tokens.get(index).map(|token| token.kind),
                        Some(TokenKind::Ident | TokenKind::Star)
                    ) {
                        end_index = index;
                        index += 1;
                    }
                }

                expecting_segment_start = false;
                continue;
            }

            if matches!(
                self.tokens.get(index).map(|token| token.kind),
                Some(TokenKind::Pipe)
            ) {
                end_index = index;
                index += 1;
                expecting_segment_start = true;
                continue;
            }

            if matches!(
                (
                    self.tokens.get(index).map(|token| token.kind),
                    self.tokens.get(index + 1).map(|token| token.kind)
                ),
                (Some(TokenKind::Dot), Some(TokenKind::Ident))
            ) {
                end_index = index + 1;
                index += 2;
                continue;
            }

            if let Some(suffix_end) = self.bareword_bracket_suffix_end(index) {
                end_index = suffix_end;
                index = suffix_end + 1;
                continue;
            }

            while matches!(
                self.tokens.get(index).map(|token| token.kind),
                Some(TokenKind::Ident | TokenKind::Star)
            ) {
                end_index = index;
                index += 1;
            }

            break;
        }

        if expecting_segment_start {
            return None;
        }

        Some(end_index)
    }

    fn bareword_bracket_suffix_end(&self, start_index: usize) -> Option<usize> {
        if self.token_at(start_index).kind != TokenKind::LBracket {
            return None;
        }

        match (
            self.tokens.get(start_index + 1).map(|token| token.kind),
            self.tokens.get(start_index + 2).map(|token| token.kind),
            self.tokens.get(start_index + 3).map(|token| token.kind),
        ) {
            (Some(TokenKind::IntLiteral), Some(TokenKind::RBracket), _) => Some(start_index + 2),
            (Some(TokenKind::Dollar), Some(TokenKind::Ident), Some(TokenKind::RBracket)) => {
                Some(start_index + 3)
            }
            _ => None,
        }
    }

    fn parse_punct_bareword_shell_word(&mut self) -> Option<ShellWord> {
        if self.at(TokenKind::Dot) && self.peek_kind() == Some(TokenKind::Dot) {
            let first = self.bump();
            let second = self.bump();
            let range = text_range(range_start(first.range), range_end(second.range));
            return Some(ShellWord::BareWord {
                text: self.input[range_start(range) as usize..range_end(range) as usize].to_owned(),
                range,
            });
        }

        None
    }

    fn parse_numeric_shell_word(&mut self) -> Option<ShellWord> {
        match self.current().kind {
            TokenKind::IntLiteral | TokenKind::FloatLiteral => {
                let token = self.bump();
                Some(ShellWord::NumericLiteral {
                    text: self.token_text(token).to_owned(),
                    range: token.range,
                })
            }
            TokenKind::Minus
                if matches!(
                    self.peek_kind(),
                    Some(TokenKind::IntLiteral | TokenKind::FloatLiteral)
                ) =>
            {
                let minus = self.bump();
                let literal = self.bump();
                let range = text_range(range_start(minus.range), range_end(literal.range));
                Some(ShellWord::NumericLiteral {
                    text: self.input[range_start(range) as usize..range_end(range) as usize]
                        .to_owned(),
                    range,
                })
            }
            TokenKind::Dot
                if self.peek_kind().is_some_and(|kind| {
                    matches!(kind, TokenKind::IntLiteral | TokenKind::FloatLiteral)
                }) =>
            {
                let dot = self.bump();
                let literal = self.bump();
                let range = text_range(range_start(dot.range), range_end(literal.range));
                Some(ShellWord::NumericLiteral {
                    text: self.input[range_start(range) as usize..range_end(range) as usize]
                        .to_owned(),
                    range,
                })
            }
            _ => None,
        }
    }

    fn parse_spaced_flag_shell_word(&mut self) -> Option<ShellWord> {
        if !self.at(TokenKind::Minus) || self.peek_kind() != Some(TokenKind::Ident) {
            return None;
        }

        let minus_index = self.current_index();
        let ident_index = self.next_significant_index(minus_index + 1);
        if self.token_at(ident_index).kind != TokenKind::Ident
            || self.has_line_break_between(minus_index, ident_index)
        {
            return None;
        }

        let minus = self.bump();
        let ident = self.bump();
        let range = text_range(range_start(minus.range), range_end(ident.range));
        Some(ShellWord::Flag {
            text: self.input[range_start(range) as usize..range_end(range) as usize].to_owned(),
            range,
        })
    }

    fn parse_brace_list_shell_word(&mut self) -> Option<ShellWord> {
        let expr = self.parse_brace_list_expr()?;
        let range = expr.range();
        Some(ShellWord::BraceList { expr, range })
    }

    fn parse_vector_literal_shell_word(&mut self) -> Option<ShellWord> {
        let expr = self.parse_vector_literal_expr()?;
        let range = expr.range();
        Some(ShellWord::VectorLiteral { expr, range })
    }

    fn parse_variable_shell_word(&mut self) -> Option<ShellWord> {
        let mut expr = self.parse_variable_expr()?;

        loop {
            if self.at(TokenKind::Dot) {
                self.bump();
                if self.current().kind != TokenKind::Ident {
                    self.error("expected member name after '.'", self.current().range);
                    break;
                }

                let member_token = self.bump();
                let member_name = self.token_text(member_token).to_owned();
                let range = text_range(range_start(expr.range()), range_end(member_token.range));
                expr = if let Some(component) = parse_vector_component_name(&member_name) {
                    Expr::ComponentAccess {
                        range,
                        target: Box::new(expr),
                        component,
                    }
                } else {
                    Expr::MemberAccess {
                        range,
                        target: Box::new(expr),
                        member: member_name,
                    }
                };
                continue;
            }

            if self.at(TokenKind::LBracket) {
                let open = self.bump();
                let index = if let Some(index) = self.parse_expr() {
                    index
                } else {
                    self.error("expected expression inside index", self.current().range);
                    break;
                };

                let end = if let Some(close) = self.eat(TokenKind::RBracket) {
                    range_end(close.range)
                } else {
                    let range = self.current().range;
                    self.error("expected ']' after index expression", range);
                    range_end(range).max(range_end(open.range))
                };

                let expr_start = range_start(expr.range());
                expr = Expr::Index {
                    target: Box::new(expr),
                    index: Box::new(index),
                    range: text_range(range_start(open.range).min(expr_start), end),
                };
                continue;
            }

            break;
        }

        let range = expr.range();
        Some(ShellWord::Variable { expr, range })
    }

    fn parse_grouped_shell_word(&mut self) -> Option<ShellWord> {
        let open = self.eat(TokenKind::LParen)?;
        let expr = self.parse_expr()?;
        let end = if let Some(close) = self.eat(TokenKind::RParen) {
            range_end(close.range)
        } else {
            self.error(
                "expected ')' to close grouped expression",
                self.current().range,
            );
            self.recover_to_rparen_or_stmt_boundary();
            self.eat(TokenKind::RParen)
                .map(|token| range_end(token.range))
                .unwrap_or_else(|| range_end(self.previous_range()).max(range_end(open.range)))
        };

        Some(ShellWord::GroupedExpr {
            expr,
            range: text_range(range_start(open.range), end),
        })
    }

    fn parse_capture_shell_word(&mut self) -> Option<ShellWord> {
        let invoke = self.parse_backquoted_invoke()?;
        Some(ShellWord::Capture {
            range: invoke.range,
            invoke,
        })
    }

    fn recover_to_shell_word_boundary(&mut self, captured: bool) {
        while !self.at(TokenKind::Eof) && !self.at_shell_terminator(captured) {
            if self.at(TokenKind::Flag) || self.can_start_shell_word(captured) {
                break;
            }
            self.bump();
        }
    }

    fn parse_postfix_update_op(&mut self) -> Option<UpdateOp> {
        if self.eat(TokenKind::PlusPlus).is_some() {
            return Some(UpdateOp::Increment);
        }

        if self.eat(TokenKind::MinusMinus).is_some() {
            return Some(UpdateOp::Decrement);
        }

        None
    }

    fn expect_stmt_terminator(&mut self, message: &str, context: StmtContext) -> Option<u32> {
        if let Some(token) = self.eat(TokenKind::Semi) {
            Some(range_end(token.range))
        } else if self.can_omit_stmt_semi(context) {
            Some(range_end(self.previous_significant_range()))
        } else {
            let range = self.current().range;
            self.error(message, range);
            self.recover_to_stmt_boundary();
            if let Some(token) = self.eat(TokenKind::Semi) {
                Some(range_end(token.range))
            } else {
                Some(range_end(self.previous_range()).max(range_end(range)))
            }
        }
    }

    fn can_omit_stmt_semi(&self, context: StmtContext) -> bool {
        matches!(self.options.mode, ParseMode::AllowTrailingStmtWithoutSemi)
            && matches!(context, StmtContext::TopLevel)
            && self.at(TokenKind::Eof)
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn can_start_shell_word(&self, captured: bool) -> bool {
        matches!(
            self.current().kind,
            TokenKind::StringLiteral
                | TokenKind::Dollar
                | TokenKind::LParen
                | TokenKind::LBrace
                | TokenKind::Ident
                | TokenKind::Pipe
                | TokenKind::Star
                | TokenKind::Colon
                | TokenKind::LtLt
                | TokenKind::IntLiteral
                | TokenKind::FloatLiteral
                | TokenKind::Dot
        ) || (!captured && self.at(TokenKind::Backquote))
            || (self.at(TokenKind::Minus)
                && matches!(
                    self.peek_kind(),
                    Some(TokenKind::IntLiteral | TokenKind::FloatLiteral)
                ))
            || (self.at(TokenKind::Dot) && self.peek_kind() == Some(TokenKind::Dot))
    }

    fn at_keyword(&self, keyword: &str) -> bool {
        self.current().kind == TokenKind::Ident && self.token_text(self.current()) == keyword
    }

    fn eat(&mut self, kind: TokenKind) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn eat_keyword(&mut self, keyword: &str) -> Option<Token> {
        if self.at_keyword(keyword) {
            Some(self.bump())
        } else {
            None
        }
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Option<Token> {
        if let Some(token) = self.eat(kind) {
            Some(token)
        } else {
            self.error(message, self.current().range);
            None
        }
    }

    fn bump(&mut self) -> Token {
        let index = self.current_index();
        let token = self.token_at(index);
        if token.kind != TokenKind::Eof {
            self.pos = index + 1;
        }
        token
    }

    fn current(&self) -> Token {
        self.token_at(self.current_index())
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.nth_kind_after_current(1)
    }

    fn peek_keyword(&self) -> Option<&'a str> {
        let next = self.next_significant_index(self.current_index() + 1);
        let token = self.tokens.get(next).copied()?;
        (token.kind == TokenKind::Ident).then(|| self.token_text(token))
    }

    fn nth_kind_after_current(&self, n: usize) -> Option<TokenKind> {
        let mut index = self.current_index();
        for _ in 0..n {
            index = self.next_significant_index(index + 1);
        }
        self.tokens.get(index).map(|token| token.kind)
    }

    fn token_text(&self, token: Token) -> &'a str {
        let start = range_start(token.range) as usize;
        let end = range_end(token.range) as usize;
        &self.input[start..end]
    }

    fn previous_range(&self) -> TextRange {
        self.current_index()
            .checked_sub(1)
            .map(|index| self.token_at(index).range)
            .unwrap_or(text_range(0, 0))
    }

    fn previous_significant_range(&self) -> TextRange {
        let mut index = self.current_index();
        while index > 0 {
            index -= 1;
            let token = self.token_at(index);
            if !token.kind.is_trivia() {
                return token.range;
            }
        }

        text_range(0, 0)
    }

    fn error(&mut self, message: &str, range: TextRange) {
        self.errors.push(ParseError {
            message: message.to_owned(),
            range,
        });
    }

    fn recover_to_decl_boundary(&mut self) {
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Comma) && !self.at(TokenKind::Semi) {
            self.bump();
        }
    }

    fn recover_to_proc_param_boundary(&mut self) {
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::Comma)
            && !self.at(TokenKind::RParen)
            && !self.at(TokenKind::LBrace)
        {
            self.bump();
        }
    }

    fn recover_to_brace_list_boundary(&mut self) {
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Comma) && !self.at(TokenKind::RBrace)
        {
            self.bump();
        }
    }

    fn recover_to_vector_literal_boundary(&mut self) {
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Comma) && !self.at(TokenKind::GtGt) {
            self.bump();
        }
    }

    fn recover_to_switch_clause_boundary(&mut self) {
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::RBrace)
            && !self.at_keyword("case")
            && !self.at_keyword("default")
        {
            self.bump();
        }
    }

    fn recover_to_rparen_or_stmt_boundary(&mut self) {
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::RParen)
            && !self.at(TokenKind::Comma)
            && !self.at(TokenKind::Semi)
            && !self.at(TokenKind::RBrace)
        {
            self.bump();
        }
    }

    fn recover_to_stmt_boundary(&mut self) {
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::Semi)
            && !self.at(TokenKind::RBrace)
            && !self.at_stmt_recovery_boundary()
        {
            self.bump();
        }
    }

    fn at_stmt_recovery_boundary(&self) -> bool {
        self.at(TokenKind::Dollar)
            || self.at(TokenKind::LBrace)
            || self.at(TokenKind::Backquote)
            || self.at(TokenKind::LParen)
            || self.at(TokenKind::IntLiteral)
            || self.at(TokenKind::FloatLiteral)
            || self.at(TokenKind::StringLiteral)
            || self.at(TokenKind::LtLt)
            || self.at(TokenKind::PlusPlus)
            || self.at(TokenKind::MinusMinus)
            || self.at(TokenKind::Bang)
            || self.at(TokenKind::Minus)
            || self.at(TokenKind::Ident)
            || self.at_keyword("if")
            || self.at_keyword("while")
            || self.at_keyword("do")
            || self.at_keyword("switch")
            || self.at_keyword("for")
            || self.at_keyword("break")
            || self.at_keyword("continue")
            || self.at_keyword("return")
            || self.at_keyword("proc")
            || (self.at_keyword("global") && self.peek_keyword() == Some("proc"))
    }

    fn current_index(&self) -> usize {
        self.next_significant_index(self.pos)
    }

    fn matching_rparen_index(&self, open_index: usize) -> Option<usize> {
        if self.token_at(open_index).kind != TokenKind::LParen {
            return None;
        }

        let mut depth = 0usize;
        let mut index = open_index;
        loop {
            let token = self.token_at(index);
            match token.kind {
                TokenKind::LParen => depth += 1,
                TokenKind::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(index);
                    }
                }
                TokenKind::Semi | TokenKind::RBrace | TokenKind::Eof => return None,
                _ => {}
            }
            index += 1;
        }
    }

    fn has_line_break_between(&self, left_index: usize, right_index: usize) -> bool {
        if right_index <= left_index {
            return false;
        }

        let left = self.token_at(left_index);
        let right = self.token_at(right_index);
        let start = range_end(left.range) as usize;
        let end = range_start(right.range) as usize;
        self.input[start.min(end)..end.max(start)].contains('\n')
    }

    fn starts_stmt_after_function_args(&self, index: usize) -> bool {
        let token = self.token_at(index);
        match token.kind {
            TokenKind::Dollar => {
                let next_index = self.next_significant_index(index + 1);
                let assign_index = if self.token_at(next_index).kind == TokenKind::Ident {
                    self.next_significant_index(next_index + 1)
                } else {
                    next_index
                };
                matches!(
                    self.token_at(assign_index).kind,
                    TokenKind::Assign
                        | TokenKind::PlusEq
                        | TokenKind::MinusEq
                        | TokenKind::StarEq
                        | TokenKind::SlashEq
                        | TokenKind::PlusPlus
                        | TokenKind::MinusMinus
                )
            }
            TokenKind::LBrace => true,
            TokenKind::Ident => {
                let text = self.token_text(token);
                matches!(
                    text,
                    "if" | "while"
                        | "do"
                        | "switch"
                        | "for"
                        | "return"
                        | "break"
                        | "continue"
                        | "global"
                ) || is_type_keyword(text)
            }
            _ => false,
        }
    }

    fn next_significant_index(&self, start: usize) -> usize {
        let mut index = start;
        while let Some(token) = self.tokens.get(index) {
            if !token.kind.is_trivia() {
                return index;
            }
            index += 1;
        }
        self.tokens.len().saturating_sub(1)
    }

    fn token_at(&self, index: usize) -> Token {
        self.tokens
            .get(index)
            .copied()
            .or_else(|| self.tokens.last().copied())
            .unwrap_or(Token::new(TokenKind::Eof, text_range(0, 0)))
    }
}

#[derive(Clone)]
enum InfixKind {
    Binary(BinaryOp),
    Assign(AssignOp),
}

fn infix_binding_power(kind: TokenKind) -> Option<(u8, u8, InfixKind)> {
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

fn is_type_keyword(keyword: &str) -> bool {
    matches!(keyword, "int" | "float" | "string" | "vector" | "matrix")
}

fn parse_type_name(keyword: &str) -> Option<TypeName> {
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
enum IntLiteralError {
    OutOfRange,
}

fn parse_int_literal_text(text: &str) -> Result<i64, IntLiteralError> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).map_err(|_| IntLiteralError::OutOfRange)
    } else {
        text.parse::<i64>().map_err(|_| IntLiteralError::OutOfRange)
    }
}

fn parse_vector_component_name(name: &str) -> Option<VectorComponent> {
    match name {
        "x" => Some(VectorComponent::X),
        "y" => Some(VectorComponent::Y),
        "z" => Some(VectorComponent::Z),
        _ => None,
    }
}

fn remap_parse_ranges(parse: &mut Parse, map: &OffsetMap) {
    remap_source_file_ranges(&mut parse.syntax, map);

    for diagnostic in &mut parse.lex_errors {
        diagnostic.range = map.map_range(diagnostic.range);
    }

    for error in &mut parse.errors {
        error.range = map.map_range(error.range);
    }
}

fn remap_source_file_ranges(source: &mut SourceFile, map: &OffsetMap) {
    for item in &mut source.items {
        match item {
            Item::Proc(proc_def) => remap_proc_def_ranges(proc_def, map),
            Item::Stmt(stmt) => remap_stmt_ranges(stmt, map),
        }
    }
}

fn remap_proc_def_ranges(proc_def: &mut mel_ast::ProcDef, map: &OffsetMap) {
    if let Some(return_type) = &mut proc_def.return_type {
        return_type.range = map.map_range(return_type.range);
    }

    for param in &mut proc_def.params {
        param.range = map.map_range(param.range);
    }

    remap_stmt_ranges(&mut proc_def.body, map);
    proc_def.range = map.map_range(proc_def.range);
}

fn remap_stmt_ranges(stmt: &mut Stmt, map: &OffsetMap) {
    match stmt {
        Stmt::Empty { range } | Stmt::Break { range } | Stmt::Continue { range } => {
            *range = map.map_range(*range);
        }
        Stmt::Proc { proc_def, range } => {
            remap_proc_def_ranges(proc_def, map);
            *range = map.map_range(*range);
        }
        Stmt::Block { statements, range } => {
            for stmt in statements {
                remap_stmt_ranges(stmt, map);
            }
            *range = map.map_range(*range);
        }
        Stmt::Expr { expr, range } => {
            remap_expr_ranges(expr, map);
            *range = map.map_range(*range);
        }
        Stmt::VarDecl { decl, range } => {
            remap_var_decl_ranges(decl, map);
            *range = map.map_range(*range);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            range,
        } => {
            remap_expr_ranges(condition, map);
            remap_stmt_ranges(then_branch, map);
            if let Some(else_branch) = else_branch {
                remap_stmt_ranges(else_branch, map);
            }
            *range = map.map_range(*range);
        }
        Stmt::While {
            condition,
            body,
            range,
        } => {
            remap_expr_ranges(condition, map);
            remap_stmt_ranges(body, map);
            *range = map.map_range(*range);
        }
        Stmt::DoWhile {
            body,
            condition,
            range,
        } => {
            remap_stmt_ranges(body, map);
            remap_expr_ranges(condition, map);
            *range = map.map_range(*range);
        }
        Stmt::Switch {
            control,
            clauses,
            range,
        } => {
            remap_expr_ranges(control, map);
            for clause in clauses {
                match &mut clause.label {
                    SwitchLabel::Case(expr) => remap_expr_ranges(expr, map),
                    SwitchLabel::Default { range } => *range = map.map_range(*range),
                }
                for stmt in &mut clause.statements {
                    remap_stmt_ranges(stmt, map);
                }
                clause.range = map.map_range(clause.range);
            }
            *range = map.map_range(*range);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
            range,
        } => {
            if let Some(init) = init {
                for expr in init {
                    remap_expr_ranges(expr, map);
                }
            }
            if let Some(condition) = condition {
                remap_expr_ranges(condition, map);
            }
            if let Some(update) = update {
                for expr in update {
                    remap_expr_ranges(expr, map);
                }
            }
            remap_stmt_ranges(body, map);
            *range = map.map_range(*range);
        }
        Stmt::ForIn {
            binding,
            iterable,
            body,
            range,
        } => {
            remap_expr_ranges(binding, map);
            remap_expr_ranges(iterable, map);
            remap_stmt_ranges(body, map);
            *range = map.map_range(*range);
        }
        Stmt::Return { expr, range } => {
            if let Some(expr) = expr {
                remap_expr_ranges(expr, map);
            }
            *range = map.map_range(*range);
        }
    }
}

fn remap_var_decl_ranges(decl: &mut VarDecl, map: &OffsetMap) {
    for declarator in &mut decl.declarators {
        if let Some(Some(size)) = &mut declarator.array_size {
            remap_expr_ranges(size, map);
        }
        if let Some(initializer) = &mut declarator.initializer {
            remap_expr_ranges(initializer, map);
        }
        declarator.range = map.map_range(declarator.range);
    }
    decl.range = map.map_range(decl.range);
}

fn remap_expr_ranges(expr: &mut Expr, map: &OffsetMap) {
    match expr {
        Expr::Ident { range, .. }
        | Expr::BareWord { range, .. }
        | Expr::Int { range, .. }
        | Expr::Float { range, .. }
        | Expr::String { range, .. } => *range = map.map_range(*range),
        Expr::Cast { expr, range, .. } => {
            remap_expr_ranges(expr, map);
            *range = map.map_range(*range);
        }
        Expr::VectorLiteral { elements, range } | Expr::ArrayLiteral { elements, range } => {
            for element in elements {
                remap_expr_ranges(element, map);
            }
            *range = map.map_range(*range);
        }
        Expr::Unary { expr, range, .. }
        | Expr::PrefixUpdate { expr, range, .. }
        | Expr::PostfixUpdate { expr, range, .. } => {
            remap_expr_ranges(expr, map);
            *range = map.map_range(*range);
        }
        Expr::Binary {
            lhs, rhs, range, ..
        }
        | Expr::Assign {
            lhs, rhs, range, ..
        } => {
            remap_expr_ranges(lhs, map);
            remap_expr_ranges(rhs, map);
            *range = map.map_range(*range);
        }
        Expr::Ternary {
            condition,
            then_expr,
            else_expr,
            range,
        } => {
            remap_expr_ranges(condition, map);
            remap_expr_ranges(then_expr, map);
            remap_expr_ranges(else_expr, map);
            *range = map.map_range(*range);
        }
        Expr::Index {
            target,
            index,
            range,
        } => {
            remap_expr_ranges(target, map);
            remap_expr_ranges(index, map);
            *range = map.map_range(*range);
        }
        Expr::MemberAccess { target, range, .. } | Expr::ComponentAccess { target, range, .. } => {
            remap_expr_ranges(target, map);
            *range = map.map_range(*range);
        }
        Expr::Invoke(invoke) => remap_invoke_ranges(invoke, map),
    }
}

fn remap_invoke_ranges(invoke: &mut InvokeExpr, map: &OffsetMap) {
    match &mut invoke.surface {
        InvokeSurface::Function { args, .. } => {
            for arg in args {
                remap_expr_ranges(arg, map);
            }
        }
        InvokeSurface::ShellLike { words, .. } => {
            for word in words {
                remap_shell_word_ranges(word, map);
            }
        }
    }
    invoke.range = map.map_range(invoke.range);
}

fn remap_shell_word_ranges(word: &mut ShellWord, map: &OffsetMap) {
    match word {
        ShellWord::Flag { range, .. }
        | ShellWord::NumericLiteral { range, .. }
        | ShellWord::BareWord { range, .. }
        | ShellWord::QuotedString { range, .. } => {
            *range = map.map_range(*range);
        }
        ShellWord::Variable { expr, range }
        | ShellWord::GroupedExpr { expr, range }
        | ShellWord::BraceList { expr, range }
        | ShellWord::VectorLiteral { expr, range } => {
            remap_expr_ranges(expr, map);
            *range = map.map_range(*range);
        }
        ShellWord::Capture { invoke, range } => {
            remap_invoke_ranges(invoke, map);
            *range = map.map_range(*range);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ParseMode, ParseOptions, SourceEncoding, parse_bytes, parse_bytes_with_encoding,
        parse_source, parse_source_with_options,
    };
    use encoding_rs::{GBK, SHIFT_JIS};
    use mel_ast::{
        AssignOp, BinaryOp, Expr, InvokeSurface, Item, ShellWord, Stmt, SwitchLabel, TypeName,
        UnaryOp, UpdateOp, VectorComponent,
    };
    use mel_syntax::text_range;

    #[test]
    fn parses_proc_fixtures() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/proc/basic-global-proc.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Proc(proc_def) => {
                assert!(proc_def.is_global);
                assert_eq!(proc_def.name, "greetUser");
                assert!(matches!(
                    proc_def.return_type,
                    Some(mel_ast::ProcReturnType {
                        ty: TypeName::String,
                        is_array: false,
                        ..
                    })
                ));
                assert_eq!(proc_def.params.len(), 1);
                assert!(matches!(proc_def.params[0].ty, TypeName::String));
                assert_eq!(proc_def.params[0].name, "$name");
                assert!(!proc_def.params[0].is_array);
                assert!(matches!(proc_def.body, Stmt::Block { .. }));
            }
            _ => panic!("expected proc item"),
        }

        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/proc/local-array-param-proc.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Proc(proc_def) => {
                assert!(!proc_def.is_global);
                assert!(proc_def.return_type.is_none());
                assert_eq!(proc_def.params.len(), 1);
                assert!(matches!(proc_def.params[0].ty, TypeName::Vector));
                assert!(proc_def.params[0].is_array);
                assert!(matches!(proc_def.body, Stmt::Block { .. }));
            }
            _ => panic!("expected proc item"),
        }

        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/proc/array-return-proc.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Proc(proc_def) => {
                assert!(matches!(
                    proc_def.return_type,
                    Some(mel_ast::ProcReturnType {
                        ty: TypeName::String,
                        is_array: true,
                        ..
                    })
                ));
                assert_eq!(proc_def.params.len(), 1);
                assert!(matches!(proc_def.params[0].ty, TypeName::String));
                assert!(!proc_def.params[0].is_array);
            }
            _ => panic!("expected proc item"),
        }
    }

    #[test]
    fn parses_nested_proc_definition_statement_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/nested-proc-definition.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Proc(proc_def) => match &proc_def.body {
                Stmt::Block { statements, .. } => {
                    assert!(matches!(statements[0], Stmt::Proc { .. }));
                    assert!(matches!(statements[1], Stmt::Proc { .. }));
                }
                _ => panic!("expected proc body block"),
            },
            _ => panic!("expected outer proc item"),
        }
    }

    #[test]
    fn reports_missing_nested_proc_body() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-nested-proc-missing-body.mel"
        ));
        assert!(
            parse
                .errors
                .iter()
                .any(|error| error.message == "expected proc body block")
        );
    }

    #[test]
    fn parses_command_statement_with_flags() {
        let parse = parse_source("frameLayout -edit -label $title $fl;");
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { words, .. } => {
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(words[1], ShellWord::Flag { .. }));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected command statement"),
        }
    }

    #[test]
    fn parses_command_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-bareword.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "print");
                        assert!(matches!(words[0], ShellWord::BareWord { .. }));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected command statement"),
        }
    }

    #[test]
    fn parses_command_dotdot_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-dotdot-bareword.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "setParent");
                        assert!(matches!(
                            words[0],
                            ShellWord::BareWord { ref text, .. } if text == ".."
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_dotdot_after_flag_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-dotdot-flag-arg.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { words, .. } => {
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. } if text == ".."
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_dotdot_without_whitespace() {
        let parse = parse_source("setParent..;");
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "setParent");
                        assert!(matches!(
                            words[0],
                            ShellWord::BareWord { ref text, .. } if text == ".."
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn keeps_quoted_dotdot_as_quoted_string() {
        let parse = parse_source(r#"setParent "..";"#);
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { words, .. } => {
                        assert!(matches!(
                            words[0],
                            ShellWord::QuotedString { ref text, .. } if text == "\"..\""
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn keeps_no_whitespace_ident_lparen_as_function_stmt() {
        let parse = parse_source("doItDRA();");
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::Function { name, args } => {
                        assert_eq!(name, "doItDRA");
                        assert!(args.is_empty());
                    }
                    _ => panic!("expected function invoke"),
                },
                _ => panic!("expected expression statement"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_function_stmt_spaced_lparen_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/function-stmt-spaced-lparen.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::Function { name, args } => {
                        assert_eq!(name, "tmBuildSet");
                        assert_eq!(args.len(), 2);
                    }
                    _ => panic!("expected function invoke"),
                },
                _ => panic!("expected expression statement"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_leading_grouped_arg_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-leading-grouped-arg.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "renameAttr");
                        assert!(matches!(
                            words[0],
                            ShellWord::GroupedExpr {
                                expr: Expr::Binary { .. },
                                ..
                            }
                        ));
                        assert!(matches!(words[1], ShellWord::Variable { .. }));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_numeric_arg_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-numeric-arg.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { words, .. } => {
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(words[1], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[2],
                            ShellWord::NumericLiteral { ref text, .. } if text == "0"
                        ));
                        assert!(matches!(words[3], ShellWord::Variable { .. }));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_signed_numeric_arg_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-signed-numeric-arg.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { words, .. } => {
                        assert!(matches!(
                            words[3],
                            ShellWord::NumericLiteral { ref text, .. } if text == "-10"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_leading_dot_float_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-leading-dot-float.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { words, .. } => {
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::NumericLiteral { ref text, .. } if text == ".7"
                        ));
                        assert!(matches!(words[2], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[3],
                            ShellWord::NumericLiteral { ref text, .. } if text == ".001"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_trailing_dot_float_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-trailing-dot-float.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { words, .. } => {
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(words[1], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[2],
                            ShellWord::NumericLiteral { ref text, .. } if text == "-1000."
                        ));
                        assert!(matches!(words[3], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[4],
                            ShellWord::NumericLiteral { ref text, .. } if text == "1000."
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_grouped_subtraction_call_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/grouped-subtraction-call.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                    Some(Expr::Invoke(invoke)) => match &invoke.surface {
                        InvokeSurface::Function { args, .. } => {
                            assert!(matches!(
                                args[1],
                                Expr::Binary {
                                    op: BinaryOp::Sub,
                                    ..
                                }
                            ));
                        }
                        _ => panic!("expected function invoke"),
                    },
                    _ => panic!("expected invoke initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_spaced_flag_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-spaced-flag.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                    Some(Expr::Invoke(invoke)) => match &invoke.surface {
                        InvokeSurface::ShellLike { head, words, .. } => {
                            assert_eq!(head, "optionVar");
                            assert!(matches!(
                                words[0],
                                ShellWord::Flag { ref text, .. } if text == "- q"
                            ));
                            assert!(matches!(
                                words[1],
                                ShellWord::BareWord { ref text, .. }
                                if text == "LayoutPreviewResolution"
                            ));
                        }
                        _ => panic!("expected shell-like invoke"),
                    },
                    _ => panic!("expected invoke initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_multiline_grouped_args_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-multiline-grouped-args.mel"
        ));
        assert!(parse.errors.is_empty());
        assert_eq!(parse.syntax.items.len(), 2);

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "connectAttr");
                        assert_eq!(words.len(), 2);
                        assert!(matches!(words[0], ShellWord::GroupedExpr { .. }));
                        assert!(matches!(words[1], ShellWord::GroupedExpr { .. }));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "setAttr");
                        assert!(matches!(words[0], ShellWord::GroupedExpr { .. }));
                        assert!(
                            matches!(words[1], ShellWord::Flag { ref text, .. } if text == "-type")
                        );
                        assert!(matches!(
                            words[2],
                            ShellWord::BareWord { ref text, .. } if text == "double3"
                        ));
                        assert!(matches!(words[3], ShellWord::GroupedExpr { .. }));
                        assert!(matches!(words[4], ShellWord::GroupedExpr { .. }));
                        assert!(matches!(words[5], ShellWord::GroupedExpr { .. }));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_point_constraint_brace_list_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-point-constraint-brace-list.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "applyPointConstraintArgs");
                        assert!(matches!(
                            words[0],
                            ShellWord::NumericLiteral { ref text, .. } if text == "2"
                        ));
                        assert!(matches!(
                            words[1],
                            ShellWord::BraceList {
                                expr: Expr::ArrayLiteral { ref elements, .. },
                                ..
                            } if elements.len() == 10
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_orient_constraint_brace_list_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-orient-constraint-brace-list.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "applyOrientConstraintArgs");
                        match &words[1] {
                            ShellWord::BraceList {
                                expr: Expr::ArrayLiteral { elements, .. },
                                ..
                            } => {
                                assert!(matches!(
                                    elements[0],
                                    Expr::String { ref text, .. } if text == "\"1\""
                                ));
                                assert!(matches!(
                                    elements[7],
                                    Expr::String { ref text, .. } if text == "\"8\""
                                ));
                                assert!(matches!(
                                    elements[8],
                                    Expr::String { ref text, .. } if text == "\"\""
                                ));
                            }
                            _ => panic!("expected brace-list shell word"),
                        }
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_capture_vector_literal_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-capture-vector-literal.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                    Some(Expr::Invoke(invoke)) => match &invoke.surface {
                        InvokeSurface::ShellLike {
                            head,
                            words,
                            captured,
                        } => {
                            assert_eq!(head, "hsv_to_rgb");
                            assert!(*captured);
                            assert!(matches!(
                                words[0],
                                ShellWord::VectorLiteral {
                                    expr: Expr::VectorLiteral { ref elements, .. },
                                    ..
                                } if elements.len() == 3
                            ));
                        }
                        _ => panic!("expected shell-like invoke"),
                    },
                    _ => panic!("expected invoke initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "text");
                        assert!(matches!(
                            words[2],
                            ShellWord::GroupedExpr {
                                expr: Expr::ComponentAccess { .. },
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_dotted_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-dotted-bareword.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "setDrivenKeyframe");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                                if text == "N_arm_01.rotateX"
                        ));
                        assert!(matches!(
                            words[2],
                            ShellWord::BareWord { ref text, .. }
                                if text == "N_arm_01_H.rotateX"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_dotted_indexed_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-dotted-indexed-bareword.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "connectAttr");
                        assert!(matches!(
                            words[0],
                            ShellWord::BareWord { ref text, .. }
                                if text == "foo.worldMatrix[0]"
                        ));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                                if text == "bar.inputWorldMatrix"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_dotted_variable_indexed_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-dotted-variable-indexed-bareword.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "connectAttr");
                        assert!(matches!(
                            words[0],
                            ShellWord::GroupedExpr {
                                expr: Expr::Binary { .. },
                                ..
                            }
                        ));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                                if text == "LayerRegistry.layerSlot[$index]"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_dotted_global_attr_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-dotted-global-attr.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "getAttr");
                        assert!(matches!(
                            words[0],
                            ShellWord::BareWord { ref text, .. }
                                if text == "defaultRenderGlobals.hyperShadeBinList"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_pipe_dag_path_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-pipe-dag-path.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "select");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                                if text == "Null|Spine_00|Tail_00"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_pipe_wildcard_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-pipe-wildcard-bareword.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "select");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. } if text == "*|_x005"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_absolute_plug_path_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-absolute-plug-path.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "defaultNavigation");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(
                            matches!(words[1], ShellWord::BareWord { ref text, .. } if text == "shaderNodePreview1")
                        );
                        assert!(matches!(words[2], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[3],
                            ShellWord::BareWord { ref text, .. }
                                if text == "|geoPreview1|geoPreviewShape1.instObjGroups[0]"
                        ));
                        assert!(matches!(words[4], ShellWord::Flag { .. }));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_namespace_pipe_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-namespace-pipe-bareword.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "select");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                                if text == "ns:root|ns:spine|ns:ctrl"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_leading_colon_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-leading-colon-bareword.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                    Some(Expr::Invoke(invoke)) => match &invoke.surface {
                        InvokeSurface::ShellLike { head, words, .. } => {
                            assert_eq!(head, "camera");
                            assert!(matches!(words[0], ShellWord::Flag { .. }));
                            assert!(matches!(
                                words[1],
                                ShellWord::BareWord { ref text, .. }
                                    if text == ":previewViewportCamera"
                            ));
                        }
                        _ => panic!("expected shell-like invoke"),
                    },
                    _ => panic!("expected invoke initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_command_grouped_args_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-grouped-args.mel"
        ));
        assert!(parse.errors.is_empty());
        assert_eq!(parse.syntax.items.len(), 2);

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "iconTextButton");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(words[1], ShellWord::QuotedString { .. }));
                        assert!(matches!(
                            words[5],
                            ShellWord::GroupedExpr {
                                expr: Expr::Binary { .. },
                                ..
                            }
                        ));
                        assert!(matches!(
                            words[7],
                            ShellWord::GroupedExpr {
                                expr: Expr::Binary { .. },
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected command statement"),
        }

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "menuItem");
                        assert!(matches!(
                            words[1],
                            ShellWord::GroupedExpr {
                                expr: Expr::Binary { .. },
                                ..
                            }
                        ));
                        assert!(matches!(
                            words[3],
                            ShellWord::Variable {
                                expr: Expr::MemberAccess { ref member, .. },
                                ..
                            } if member == "name"
                        ));
                        assert!(matches!(words[5], ShellWord::Capture { .. }));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected command statement"),
        }
    }

    #[test]
    fn parses_command_capture_grouped_function_call_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/command-capture-grouped-function-call.mel"
        ));
        assert!(parse.errors.is_empty());
        assert_eq!(parse.syntax.items.len(), 2);

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                    Some(Expr::Unary { expr, .. }) => match &**expr {
                        Expr::Invoke(invoke) => match &invoke.surface {
                            InvokeSurface::ShellLike {
                                head,
                                words,
                                captured,
                            } => {
                                assert_eq!(head, "optionVar");
                                assert!(*captured);
                                assert!(matches!(words[0], ShellWord::Flag { .. }));
                                assert!(matches!(
                                    words[1],
                                    ShellWord::GroupedExpr {
                                        expr: Expr::Invoke(_),
                                        ..
                                    }
                                ));
                            }
                            _ => panic!("expected shell-like capture"),
                        },
                        _ => panic!("expected invoke under unary expression"),
                    },
                    _ => panic!("expected unary initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "optionVar");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::GroupedExpr {
                                expr: Expr::Invoke(_),
                                ..
                            }
                        ));
                        assert!(matches!(
                            words[2],
                            ShellWord::GroupedExpr {
                                expr: Expr::Invoke(_),
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_index_and_add_assign() {
        let parse = parse_source("$items[$i] += 1;");
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Assign { op, lhs, .. },
                    ..
                } => {
                    assert!(matches!(op, AssignOp::AddAssign));
                    assert!(matches!(**lhs, Expr::Index { .. }));
                }
                _ => panic!("expected add-assign statement"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_operator_precedence() {
        let parse = parse_source("$value = 1 + 2 * 3;");
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => match &**rhs {
                    Expr::Binary {
                        op: BinaryOp::Add,
                        rhs,
                        ..
                    } => {
                        assert!(matches!(
                            **rhs,
                            Expr::Binary {
                                op: BinaryOp::Mul,
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected additive expression"),
                },
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_ternary_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/ternary-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        assert_eq!(parse.syntax.items.len(), 3);

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => assert!(matches!(**rhs, Expr::Ternary { .. })),
                _ => panic!("expected ternary assignment"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_exponent_float_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/exponent-float-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        assert_eq!(parse.syntax.items.len(), 3);

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => assert!(matches!(
                    **rhs,
                    Expr::Float { ref text, .. } if text == "1.0e-3"
                )),
                _ => panic!("expected exponent assignment"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => match &**rhs {
                    Expr::Binary { lhs, rhs, .. } => {
                        assert!(matches!(
                            **lhs,
                            Expr::Float { ref text, .. } if text == "1e+3"
                        ));
                        assert!(matches!(
                            **rhs,
                            Expr::Float { ref text, .. } if text == "0.0e0"
                        ));
                    }
                    _ => panic!("expected exponent binary expression"),
                },
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[2] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => {
                    assert!(matches!(
                        decl.declarators[0].initializer,
                        Some(Expr::ArrayLiteral { .. })
                    ));
                }
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_trailing_dot_float_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/trailing-dot-float-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        assert_eq!(parse.syntax.items.len(), 3);

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => assert!(matches!(
                    **rhs,
                    Expr::Float { ref text, .. } if text == "1000."
                )),
                _ => panic!("expected trailing-dot float assignment"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => match &**rhs {
                    Expr::Binary { lhs, rhs, .. } => {
                        assert!(matches!(
                            **lhs,
                            Expr::Float { ref text, .. } if text == "0."
                        ));
                        assert!(matches!(
                            **rhs,
                            Expr::Float { ref text, .. } if text == "1."
                        ));
                    }
                    _ => panic!("expected binary expression"),
                },
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[2] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => match &**rhs {
                    Expr::ArrayLiteral { elements, .. } => {
                        assert_eq!(elements.len(), 3);
                        assert!(matches!(
                            elements[0],
                            Expr::Float { ref text, .. } if text == "0."
                        ));
                        assert!(matches!(
                            elements[1],
                            Expr::Float { ref text, .. } if text == "1."
                        ));
                        assert!(matches!(
                            elements[2],
                            Expr::Float { ref text, .. } if text == "2."
                        ));
                    }
                    _ => panic!("expected brace-list assignment"),
                },
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_for_in_and_for_loop_fixtures() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/while-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        assert!(matches!(
            parse.syntax.items[0],
            Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::While { .. })
        ));

        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/for-loop-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        assert!(matches!(
            parse.syntax.items[0],
            Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::For { .. })
        ));

        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/for-loop-multi-init-update.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::For {
                    init: Some(init),
                    condition: Some(_),
                    update: Some(update),
                    ..
                } => {
                    assert_eq!(init.len(), 2);
                    assert_eq!(update.len(), 2);
                }
                _ => panic!("expected classic for statement with multi-clause init/update"),
            },
            _ => panic!("expected statement"),
        }

        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/for-in-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        assert!(matches!(
            parse.syntax.items[0],
            Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::ForIn { .. })
        ));
    }

    #[test]
    fn parses_if_else_and_break_continue_fixtures() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/if-else-command.mel"
        ));
        assert!(parse.errors.is_empty());
        assert!(matches!(
            parse.syntax.items[0],
            Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::If { .. })
        ));

        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/break-continue.mel"
        ));
        assert!(parse.errors.is_empty());
    }

    #[test]
    fn parses_switch_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/switch-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Switch { clauses, .. } => {
                    assert_eq!(clauses.len(), 3);
                    assert!(matches!(clauses[0].label, SwitchLabel::Default { .. }));
                    assert!(clauses[0].statements.len() == 2);
                    assert!(matches!(clauses[1].label, SwitchLabel::Case(_)));
                    assert!(clauses[1].statements.is_empty());
                    assert!(matches!(clauses[2].label, SwitchLabel::Case(_)));
                    assert!(clauses[2].statements.len() == 2);
                }
                _ => panic!("expected switch statement"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_postfix_update_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/postfix-update.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::PostfixUpdate { op, .. },
                    ..
                } => assert!(matches!(op, UpdateOp::Increment)),
                _ => panic!("expected postfix update"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_compound_assign_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/compound-assign-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        assert_eq!(parse.syntax.items.len(), 3);

        let expected = [
            AssignOp::SubAssign,
            AssignOp::MulAssign,
            AssignOp::DivAssign,
        ];

        for (item, expected_op) in parse.syntax.items.iter().zip(expected) {
            match item {
                Item::Stmt(stmt) => match &**stmt {
                    Stmt::Expr {
                        expr: Expr::Assign { op, .. },
                        ..
                    } => assert_eq!(*op, expected_op),
                    _ => panic!("expected compound assignment"),
                },
                _ => panic!("expected statement"),
            }
        }
    }

    #[test]
    fn parses_prefix_update_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/prefix-update-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        assert_eq!(parse.syntax.items.len(), 2);

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::PrefixUpdate { op, .. },
                    ..
                } => assert!(matches!(op, UpdateOp::Increment)),
                _ => panic!("expected prefix increment"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::PrefixUpdate { op, .. },
                    ..
                } => assert!(matches!(op, UpdateOp::Decrement)),
                _ => panic!("expected prefix decrement"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_do_while_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/do-while-basic.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::DoWhile { body, .. } => {
                    assert!(matches!(&**body, Stmt::Block { .. }));
                }
                _ => panic!("expected do-while statement"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_variable_declaration_fixtures() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/var-decl-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => {
                    assert!(matches!(decl.ty, TypeName::Int));
                    assert_eq!(decl.declarators.len(), 1);
                    assert_eq!(decl.declarators[0].name, "$count");
                }
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }

        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/global-var-decl.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => {
                    assert!(decl.is_global);
                    assert!(matches!(decl.ty, TypeName::String));
                    assert!(decl.declarators[0].array_size.is_some());
                    assert!(matches!(
                        decl.declarators[0].initializer,
                        Some(Expr::ArrayLiteral { .. })
                    ));
                }
                _ => panic!("expected global variable declaration"),
            },
            _ => panic!("expected statement"),
        }

        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/var-decl-multi-array.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => {
                    assert_eq!(decl.declarators.len(), 2);
                    assert!(matches!(
                        decl.declarators[1].initializer,
                        Some(Expr::ArrayLiteral { .. })
                    ));
                }
                _ => panic!("expected multi declarator"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_brace_list_assignment_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/brace-list-assign.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => {
                    assert!(matches!(**rhs, Expr::ArrayLiteral { .. }));
                }
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_cast_expression_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/cast-basic.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                    Some(Expr::Cast { ty, expr, .. }) => {
                        assert!(matches!(ty, TypeName::String));
                        assert!(matches!(**expr, Expr::Ident { .. }));
                    }
                    _ => panic!("expected string cast initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                    Some(Expr::Cast { ty, expr, .. }) => {
                        assert!(matches!(ty, TypeName::Int));
                        assert!(matches!(**expr, Expr::Binary { .. }));
                    }
                    _ => panic!("expected int cast initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[2] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => match &**rhs {
                    Expr::Cast { ty, expr, .. } => {
                        assert!(matches!(ty, TypeName::String));
                        assert!(matches!(**expr, Expr::Invoke(_)));
                    }
                    _ => panic!("expected nested string cast"),
                },
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_path_like_bareword_expression_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/path-like-bareword-basic.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                    Some(Expr::BareWord { text, .. }) => {
                        assert_eq!(text, "AA_Bar*|mdl|_XXa0|");
                    }
                    _ => panic!("expected path-like bareword initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_hex_integer_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/hex-int-basic.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => match &**rhs {
                    Expr::Binary { lhs, rhs, .. } => {
                        assert!(matches!(**lhs, Expr::Int { value, .. } if value == 0x8000));
                        assert!(matches!(**rhs, Expr::Int { value, .. } if value == 0x0001));
                    }
                    _ => panic!("expected hex integer binary expression"),
                },
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_caret_operator_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/caret-operator-basic.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                    Some(Expr::Binary { op, .. }) => assert_eq!(op, &BinaryOp::Caret),
                    _ => panic!("expected caret binary expression"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_two_element_vector_literal_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/vector-literal-two-elements.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                    Some(Expr::VectorLiteral { elements, .. }) => assert_eq!(elements.len(), 2),
                    _ => panic!("expected vector literal initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_unary_negate_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/unary-negate-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => match &**rhs {
                    Expr::Binary { lhs, rhs, .. } => {
                        assert!(matches!(
                            **lhs,
                            Expr::Unary {
                                op: UnaryOp::Negate,
                                ..
                            }
                        ));
                        assert!(matches!(
                            **rhs,
                            Expr::Unary {
                                op: UnaryOp::Negate,
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected binary negate expression"),
                },
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_vector_literal_and_component_fixtures() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/vector-literal-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => {
                    assert!(matches!(**rhs, Expr::VectorLiteral { .. }));
                }
                _ => panic!("expected vector assignment"),
            },
            _ => panic!("expected statement"),
        }

        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/vector-component-basic.mel"
        ));
        assert!(parse.errors.is_empty());
        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => match &**rhs {
                    Expr::Binary { lhs, rhs, .. } => {
                        assert!(matches!(
                            **lhs,
                            Expr::ComponentAccess {
                                component: VectorComponent::X,
                                ..
                            }
                        ));
                        assert!(matches!(
                            **rhs,
                            Expr::ComponentAccess {
                                component: VectorComponent::Y,
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected binary component access"),
                },
                _ => panic!("expected vector component assignment"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn parses_member_access_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/member-access-basic.mel"
        ));
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => match &**rhs {
                    Expr::Binary { lhs, rhs, .. } => {
                        assert!(matches!(
                            **lhs,
                            Expr::MemberAccess { ref member, .. } if member == "foo"
                        ));
                        assert!(matches!(
                            **rhs,
                            Expr::MemberAccess { ref member, .. } if member == "bar"
                        ));
                    }
                    _ => panic!("expected binary member access"),
                },
                _ => panic!("expected member access assignment"),
            },
            _ => panic!("expected statement"),
        }

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr:
                        Expr::Assign {
                            rhs,
                            op: AssignOp::Assign,
                            ..
                        },
                    ..
                } => assert!(matches!(
                    **rhs,
                    Expr::MemberAccess { ref member, .. } if member == "name"
                )),
                _ => panic!("expected indexed member access"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn reports_missing_index_bracket_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-index-bracket.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected ']' after index expression"
        );
    }

    #[test]
    fn reports_missing_proc_body_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/proc/missing-proc-body.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected proc body block");
    }

    #[test]
    fn reports_missing_proc_param_name_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/proc/missing-proc-param-name.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected '$' before proc parameter name"
        );
    }

    #[test]
    fn reports_missing_compound_assign_rhs_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-compound-assign-rhs.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected expression after operator"
        );
    }

    #[test]
    fn reports_missing_ternary_colon_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-ternary-colon.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected ':' in ternary expression"
        );
    }

    #[test]
    fn reports_missing_prefix_update_operand_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-prefix-update-operand.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected expression after prefix update"
        );
    }

    #[test]
    fn reports_missing_do_while_semi_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/missing-do-while-semi.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected ';' after do-while statement"
        );
    }

    #[test]
    fn reports_missing_for_clause_expr_after_comma_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/missing-for-clause-expr-after-comma.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected expression after ',' in for clause"
        );
    }

    #[test]
    fn reports_missing_var_declarator_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/missing-var-declarator.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected variable declarator");
    }

    #[test]
    fn reports_missing_while_condition_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/missing-while-condition.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected while condition");
    }

    #[test]
    fn reports_missing_while_body_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/missing-while-body.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected while body");
    }

    #[test]
    fn reports_missing_switch_case_value_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/missing-switch-case-value.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected case value");
    }

    #[test]
    fn reports_missing_switch_colon_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/missing-switch-colon.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected ':' after switch label");
    }

    #[test]
    fn reports_missing_unary_negate_operand_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-unary-negate-operand.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected expression after unary operator"
        );
    }

    #[test]
    fn reports_missing_caret_rhs_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-caret-rhs.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected expression after operator"
        );
    }

    #[test]
    fn reports_missing_brace_list_close_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-brace-list-close.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected '}' to close brace list");
    }

    #[test]
    fn reports_missing_cast_operand_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-cast-operand.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected expression after cast");
    }

    #[test]
    fn reports_malformed_path_like_bareword_missing_segment_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/malformed-path-like-bareword-missing-segment.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected expression inside index");
    }

    #[test]
    fn reports_missing_vector_close_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-vector-close.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected '>>' to close vector literal"
        );
    }

    #[test]
    fn reports_missing_member_name_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/missing-member-name.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected member name after '.'");
    }

    #[test]
    fn reports_trailing_dot_float_double_dot_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/expressions/trailing-dot-float-double-dot.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected member name after '.'");
    }

    #[test]
    fn reports_malformed_command_word_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-word.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_missing_closing_backquote_without_command_cascade_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-missing-closing-backquote.mel"
        ));
        assert_eq!(parse.errors.len(), 1);
        assert_eq!(parse.errors[0].message, "expected closing backquote");
        assert_eq!(parse.syntax.items.len(), 2);

        match &parse.syntax.items[1] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "optionVar");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::GroupedExpr {
                                expr: Expr::Invoke(_),
                                ..
                            }
                        ));
                        assert!(matches!(
                            words[2],
                            ShellWord::GroupedExpr {
                                expr: Expr::Invoke(_),
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn reports_malformed_command_signed_number_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-signed-number.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_spaced_flag_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-spaced-flag.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_single_dot_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-single-dot.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_leading_dot_no_digit_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-leading-dot-no-digit.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_spaced_dotted_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-spaced-dotted-bareword.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_spaced_pipe_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-spaced-pipe-bareword.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_empty_pipe_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-empty-pipe-bareword.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_pipe_followed_by_whitespace() {
        let parse = parse_source("select -r | spine_00;");
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_trailing_pipe_before_semicolon() {
        let parse = parse_source("select -r y_ang| ;");
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_spaced_namespace_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-spaced-namespace-bareword.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_leading_colon_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-leading-colon-bareword.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_dotted_variable_indexed_bareword_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-dotted-variable-indexed-bareword.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "unexpected token in command invocation"
        );
    }

    #[test]
    fn reports_malformed_command_brace_list_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-brace-list.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected '}' to close brace list");
    }

    #[test]
    fn reports_malformed_command_vector_literal_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/malformed-command-vector-literal.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(
            parse.errors[0].message,
            "expected '>>' to close vector literal"
        );
    }

    #[test]
    fn recovers_missing_statement_parens_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/missing-statement-parens.mel"
        ));
        assert_eq!(parse.errors.len(), 2);
        assert_eq!(
            parse.errors[0].message,
            "expected ')' to close grouped expression"
        );
        assert_eq!(
            parse.errors[1].message,
            "expected ')' to close function invocation"
        );
        assert_eq!(parse.syntax.items.len(), 3);
        assert!(matches!(
            parse.syntax.items[2],
            Item::Stmt(ref stmt) if matches!(
                &**stmt,
                Stmt::Expr {
                    expr: Expr::Assign { .. },
                    ..
                }
            )
        ));
    }

    #[test]
    fn recovers_missing_statement_semi_fixture() {
        let parse = parse_source(include_str!(
            "../../../tests/corpus/parser/statements/missing-statement-semi-recovery.mel"
        ));
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected ';' after statement");
        assert_eq!(parse.syntax.items.len(), 2);
        assert!(matches!(
            parse.syntax.items[1],
            Item::Stmt(ref stmt) if matches!(
                &**stmt,
                Stmt::Expr {
                    expr: Expr::Assign { .. },
                    ..
                }
            )
        ));
    }

    #[test]
    fn allows_trailing_top_level_statement_without_semicolon_in_lenient_mode() {
        let parse = parse_source_with_options(
            include_str!("../../../tests/corpus/parser/statements/trailing-statement-no-semi.mel"),
            ParseOptions {
                mode: ParseMode::AllowTrailingStmtWithoutSemi,
            },
        );
        assert!(parse.errors.is_empty());
        assert_eq!(parse.syntax.items.len(), 1);
        assert!(matches!(
            parse.syntax.items[0],
            Item::Stmt(ref stmt) if matches!(
                &**stmt,
                Stmt::Expr {
                    expr: Expr::Invoke(_),
                    ..
                }
            )
        ));
    }

    #[test]
    fn still_requires_semicolon_between_top_level_statements_in_lenient_mode() {
        let parse = parse_source_with_options(
            "$x = 1\n$y = 2;",
            ParseOptions {
                mode: ParseMode::AllowTrailingStmtWithoutSemi,
            },
        );
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected ';' after statement");
    }

    #[test]
    fn still_requires_semicolon_for_nested_statement_in_lenient_mode() {
        let parse = parse_source_with_options(
            "if ($ready) print(\"hello\")",
            ParseOptions {
                mode: ParseMode::AllowTrailingStmtWithoutSemi,
            },
        );
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "expected ';' after statement");
    }

    #[test]
    fn auto_detects_cp932_and_maps_ranges_to_original_bytes() {
        let source = r#"print "設定";"#;
        let (bytes, _, had_errors) = SHIFT_JIS.encode(source);
        assert!(!had_errors);

        let parse = parse_bytes(bytes.as_ref());
        assert_eq!(parse.source_encoding, SourceEncoding::Cp932);
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { words, .. } => match &words[0] {
                        ShellWord::QuotedString { range, .. } => {
                            assert_eq!(*range, text_range(6, bytes.len() as u32 - 1));
                        }
                        _ => panic!("expected quoted string"),
                    },
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn explicit_gbk_decode_preserves_command_surface() {
        let source = r#"print "按钮";"#;
        let (bytes, _, had_errors) = GBK.encode(source);
        assert!(!had_errors);

        let parse = parse_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Gbk);
        assert_eq!(parse.source_encoding, SourceEncoding::Gbk);
        assert!(parse.errors.is_empty());

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => match &invoke.surface {
                    InvokeSurface::ShellLike { words, .. } => match &words[0] {
                        ShellWord::QuotedString { range, .. } => {
                            assert_eq!(*range, text_range(6, bytes.len() as u32 - 1));
                        }
                        _ => panic!("expected quoted string"),
                    },
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected command expression"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn lossy_utf8_single_invalid_byte_keeps_display_ranges_aligned() {
        let parse = parse_bytes(b"print \"A\xffB\";\nprint(;\n");
        assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
        assert_eq!(parse.decode_errors.len(), 1);
        assert_eq!(
            parse.decode_errors[0].message,
            "source is not valid UTF-8; decoded lossily"
        );
        assert_eq!(parse.decode_errors[0].range, text_range(8, 9));

        let decode_span = parse.source_map.display_range(parse.decode_errors[0].range);
        assert_eq!(&parse.source_text[decode_span], "\u{FFFD}");

        assert!(!parse.errors.is_empty());
        let parse_error_span = parse.source_map.display_range(parse.errors[0].range);
        assert_eq!(&parse.source_text[parse_error_span], ";");
    }

    #[test]
    fn lossy_utf8_truncated_sequence_maps_full_invalid_span_to_replacement() {
        let parse =
            parse_bytes_with_encoding(b"print \"A\xe3\x81\";\nprint(;\n", SourceEncoding::Utf8);
        assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
        assert_eq!(parse.decode_errors.len(), 1);
        assert_eq!(parse.decode_errors[0].range, text_range(8, 10));

        let decode_span = parse.source_map.display_range(parse.decode_errors[0].range);
        assert_eq!(&parse.source_text[decode_span], "\u{FFFD}");

        assert!(!parse.errors.is_empty());
        let parse_error_span = parse.source_map.display_range(parse.errors[0].range);
        assert_eq!(&parse.source_text[parse_error_span], ";");
    }

    #[test]
    fn reports_decimal_integer_literal_overflow() {
        let parse = parse_source("int $value = 9223372036854775808;");
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "integer literal out of range");
        assert_eq!(parse.errors[0].range, text_range(13, 32));

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                    Some(Expr::Int { value, .. }) => assert_eq!(*value, 0),
                    _ => panic!("expected integer initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }
    }

    #[test]
    fn reports_hex_integer_literal_overflow() {
        let parse = parse_source("int $value = 0x8000000000000000;");
        assert!(!parse.errors.is_empty());
        assert_eq!(parse.errors[0].message, "integer literal out of range");
        assert_eq!(parse.errors[0].range, text_range(13, 31));

        match &parse.syntax.items[0] {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                    Some(Expr::Int { value, .. }) => assert_eq!(*value, 0),
                    _ => panic!("expected integer initializer"),
                },
                _ => panic!("expected variable declaration"),
            },
            _ => panic!("expected statement"),
        }
    }
}
