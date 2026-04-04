use mel_ast::{
    AssignOp, BinaryOp, Declarator, Expr, InvokeExpr, InvokeSurface, Item, ProcDef, ProcParam,
    ProcReturnType, ShellWord, SourceFile, Stmt, SwitchClause, SwitchLabel, TypeName, UnaryOp,
    UpdateOp, VarDecl, VectorComponent,
};
use mel_lexer::{Lexer, significant_lexer};
use mel_syntax::{SourceMap, TextRange, Token, TokenKind, range_end, range_start, text_range};

use crate::{Parse, ParseError, ParseMode, ParseOptions, SourceEncoding};

mod bareword;
mod cursor;
mod expr;
mod proc;
mod recovery;
mod shell;
mod stmt;
mod support;

use self::support::*;

const TOKEN_LOOKAHEAD: usize = 4;

pub(crate) struct Parser<'a> {
    input: &'a str,
    options: ParseOptions,
    tokens: TokenWindow<'a>,
    pos: usize,
    rewind_floor: Option<usize>,
    token_cache: [Token; TOKEN_LOOKAHEAD],
    token_cache_base: usize,
    errors: Vec<ParseError>,
}

struct TokenWindow<'a> {
    lexer: Lexer<'a>,
    tokens: Vec<Token>,
    base_index: usize,
    eof_seen: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StmtContext {
    TopLevel,
    Nested,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(input: &'a str, options: ParseOptions) -> Self {
        let mut parser = Self {
            input,
            options,
            tokens: TokenWindow::new(input),
            pos: 0,
            rewind_floor: None,
            token_cache: [Token::new(TokenKind::Eof, text_range(0, 0)); TOKEN_LOOKAHEAD],
            token_cache_base: 0,
            errors: Vec::new(),
        };
        parser.refresh_token_cache();
        parser
    }

    pub(crate) fn parse(mut self) -> Parse {
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
            lex_errors: self.tokens.finish_diagnostics(),
            errors: self.errors,
        }
    }

    fn set_pos(&mut self, pos: usize) {
        if pos == self.pos && self.token_cache_base == self.pos {
            return;
        }

        let position_delta = pos.checked_sub(self.pos);
        let can_advance_cache = position_delta
            .is_some_and(|delta| self.token_cache_base == self.pos && delta < TOKEN_LOOKAHEAD);
        self.pos = pos;
        self.prune_consumed_tokens();

        if let Some(delta) = position_delta.filter(|&delta| delta > 0) {
            if can_advance_cache {
                self.advance_token_cache_by(delta);
                return;
            }
        } else {
            self.refresh_token_cache();
            return;
        }

        self.refresh_token_cache();
    }

    fn advance_token_cache_by(&mut self, delta: usize) {
        debug_assert!((1..TOKEN_LOOKAHEAD).contains(&delta));
        self.token_cache.copy_within(delta.., 0);
        for i in TOKEN_LOOKAHEAD - delta..TOKEN_LOOKAHEAD {
            self.token_cache[i] = self.tokens.token_at(self.pos + i);
        }
        self.token_cache_base = self.pos;
    }

    fn prune_consumed_tokens(&mut self) {
        let keep_from = self
            .rewind_floor
            .unwrap_or_else(|| self.pos.saturating_sub(1));
        self.tokens.discard_before(keep_from);
    }
}

impl<'a> TokenWindow<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            lexer: significant_lexer(input),
            tokens: Vec::new(),
            base_index: 0,
            eof_seen: false,
        }
    }

    fn token_at(&mut self, index: usize) -> Token {
        self.ensure_loaded(index);
        let relative = index.saturating_sub(self.base_index);
        self.tokens
            .get(relative)
            .copied()
            .or_else(|| self.tokens.last().copied())
            .unwrap_or(Token::new(TokenKind::Eof, text_range(0, 0)))
    }

    fn clamp_index(&mut self, index: usize) -> usize {
        self.ensure_loaded(index);
        if index < self.base_index {
            self.base_index
        } else {
            let last_index = self.base_index + self.tokens.len().saturating_sub(1);
            index.min(last_index)
        }
    }

    fn discard_before(&mut self, keep_from: usize) {
        const PRUNE_GRANULARITY: usize = 16384;
        const HISTORY_TAIL: usize = 128;

        if keep_from <= self.base_index + PRUNE_GRANULARITY {
            return;
        }

        let target = keep_from.saturating_sub(HISTORY_TAIL);
        let max_drop = self.tokens.len().saturating_sub(2);
        let drop = target.saturating_sub(self.base_index).min(max_drop);
        if drop == 0 {
            return;
        }

        self.tokens.drain(..drop);
        self.base_index += drop;
    }

    fn finish_diagnostics(mut self) -> Vec<mel_syntax::LexDiagnostic> {
        while !self.eof_seen {
            self.ensure_loaded(self.base_index + self.tokens.len());
        }
        self.lexer.finish()
    }

    fn ensure_loaded(&mut self, index: usize) {
        const PREFETCH_CHUNK_TOKENS: usize = 32_768;
        let target = index.saturating_add(PREFETCH_CHUNK_TOKENS);
        let desired = target.saturating_add(1).saturating_sub(self.base_index);
        if desired > self.tokens.len() {
            self.tokens
                .reserve(desired.saturating_sub(self.tokens.len()));
        }
        while self.base_index + self.tokens.len() <= target && !self.eof_seen {
            let Some(token) = self.lexer.next() else {
                break;
            };
            self.eof_seen = token.kind == TokenKind::Eof;
            self.tokens.push(token);
        }

        if self.tokens.is_empty() {
            self.eof_seen = true;
            self.tokens
                .push(Token::new(TokenKind::Eof, text_range(0, 0)));
        }
    }
}
