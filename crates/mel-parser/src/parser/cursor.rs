use super::*;

impl<'a> Parser<'a> {
    pub(super) fn at(&mut self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    pub(super) fn at_keyword(&mut self, keyword: &str) -> bool {
        let current = self.current();
        current.kind == TokenKind::Ident && self.token_text(current) == keyword
    }

    pub(super) fn eat(&mut self, kind: TokenKind) -> Option<Token> {
        if self.at(kind) {
            Some(self.bump())
        } else {
            None
        }
    }

    pub(super) fn eat_keyword(&mut self, keyword: &str) -> Option<Token> {
        if self.at_keyword(keyword) {
            Some(self.bump())
        } else {
            None
        }
    }

    pub(super) fn expect(&mut self, kind: TokenKind, message: &'static str) -> Option<Token> {
        if let Some(token) = self.eat(kind) {
            Some(token)
        } else {
            let range = self.current().range;
            self.error(message, range);
            None
        }
    }

    pub(super) fn bump(&mut self) -> Token {
        let index = self.current_index();
        let token = self.current();
        if token.kind != TokenKind::Eof {
            self.set_pos(index + 1);
        }
        token
    }

    pub(super) fn current(&mut self) -> Token {
        if self.token_cache_base != self.pos {
            self.refresh_token_cache();
        }
        self.token_cache[0]
    }

    pub(super) fn peek_kind(&mut self) -> Option<TokenKind> {
        self.nth_kind_after_current(1)
    }

    pub(super) fn peek_keyword(&mut self) -> Option<&'a str> {
        let next = self.next_significant_index(self.current_index() + 1);
        let token = self.token_at(next);
        (token.kind == TokenKind::Ident).then(|| self.token_text(token))
    }

    pub(super) fn nth_kind_after_current(&mut self, n: usize) -> Option<TokenKind> {
        if n < TOKEN_LOOKAHEAD {
            return Some(self.token_at(self.current_index().saturating_add(n)).kind);
        }

        let mut index = self.current_index();
        for _ in 0..n {
            index = self.next_significant_index(index + 1);
        }
        self.kind_at(index)
    }

    pub(super) fn token_text(&self, token: Token) -> &'a str {
        let start = range_start(token.range) as usize;
        let end = range_end(token.range) as usize;
        debug_assert!(self.input.is_char_boundary(start));
        debug_assert!(self.input.is_char_boundary(end));
        &self.input[start..end]
    }

    pub(super) fn previous_range(&mut self) -> TextRange {
        self.current_index()
            .checked_sub(1)
            .map(|index| self.token_at(index).range)
            .unwrap_or(text_range(0, 0))
    }

    pub(super) fn previous_significant_range(&mut self) -> TextRange {
        self.previous_range()
    }

    pub(super) fn current_index(&self) -> usize {
        self.pos
    }

    pub(super) fn matching_rparen_index(&mut self, open_index: usize) -> Option<usize> {
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

    pub(super) fn has_line_break_between(&mut self, left_index: usize, right_index: usize) -> bool {
        if right_index <= left_index {
            return false;
        }

        let left = self.token_at(left_index);
        let right = self.token_at(right_index);
        let start = range_end(left.range) as usize;
        let end = range_start(right.range) as usize;
        self.input[start.min(end)..end.max(start)].contains('\n')
    }

    pub(super) fn tokens_are_adjacent(&mut self, left_index: usize, right_index: usize) -> bool {
        if right_index <= left_index {
            return false;
        }

        let left = self.token_at(left_index);
        let right = self.token_at(right_index);
        range_end(left.range) as usize == range_start(right.range) as usize
    }

    pub(super) fn starts_stmt_after_function_args(&mut self, index: usize) -> bool {
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

    pub(super) fn next_significant_index(&mut self, start: usize) -> usize {
        self.tokens.clamp_index(start)
    }

    pub(super) fn token_at(&mut self, index: usize) -> Token {
        if index >= self.token_cache_base && index - self.token_cache_base < TOKEN_LOOKAHEAD {
            return self.token_cache[index - self.token_cache_base];
        }
        self.tokens.token_at(index)
    }

    pub(super) fn refresh_token_cache(&mut self) {
        self.token_cache_base = self.pos;
        for i in 0..TOKEN_LOOKAHEAD {
            self.token_cache[i] = self.tokens.token_at(self.pos + i);
        }
    }

    pub(super) fn kind_at(&mut self, index: usize) -> Option<TokenKind> {
        Some(self.token_at(index).kind)
    }
}
