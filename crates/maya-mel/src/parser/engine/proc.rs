use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_item(&mut self) -> Option<Item> {
        if self.at_keyword("global") && self.peek_keyword() == Some("proc") {
            let proc_def = self.parse_proc_def()?;
            if !self.record_statement_budget(proc_def.range) {
                return None;
            }
            return Some(Item::Proc(Box::new(proc_def)));
        }

        if self.at_keyword("proc") {
            let proc_def = self.parse_proc_def()?;
            if !self.record_statement_budget(proc_def.range) {
                return None;
            }
            return Some(Item::Proc(Box::new(proc_def)));
        }

        self.parse_stmt(StmtContext::TopLevel)
            .map(Box::new)
            .map(Item::Stmt)
    }

    pub(super) fn parse_proc_def(&mut self) -> Option<ProcDef> {
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
                name_range: range,
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
                            let range = self.current().range;
                            self.error("expected proc parameter", range);
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

        let body = if self.at(TokenKind::LBrace) {
            let body_start = self.current().range;
            self.with_nesting(body_start, |parser| parser.parse_block_stmt())
        } else {
            self.parse_block_stmt()
        };
        let body = if let Some(stmt) = body {
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
            name_range: name_token.range,
            params,
            body,
            is_global,
            range: text_range(start, end),
        })
    }

    pub(super) fn parse_proc_return_type(&mut self) -> Option<ProcReturnType> {
        let current = self.current();
        if current.kind != TokenKind::Ident || !is_type_keyword(self.token_text(current)) {
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

    pub(super) fn parse_proc_param(&mut self) -> Option<ProcParam> {
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
                name_range: range,
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
                name_range: dollar.range,
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
            name_range: text_range(range_start(dollar.range), range_end(ident.range)),
            is_array,
            range: text_range(start, end),
        })
    }

    pub(super) fn parse_proc_param_array_suffix(&mut self) -> u32 {
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

    pub(super) fn parse_proc_return_array_suffix(&mut self) -> u32 {
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
}
