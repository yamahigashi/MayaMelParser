use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let stmt = if let Some(token) = self.eat(TokenKind::Semi) {
            Some(Stmt::Empty { range: token.range })
        } else if self.at_keyword("proc")
            || (self.at_keyword("global") && self.peek_keyword() == Some("proc"))
        {
            let proc_def = self.parse_proc_def()?;
            let range = proc_def.range;
            Some(Stmt::Proc {
                proc_def: Box::new(proc_def),
                range,
            })
        } else if self.at(TokenKind::LBrace) {
            let block_start = self.current().range;
            self.with_nesting(block_start, |parser| parser.parse_block_stmt())
        } else if self.at_keyword("if") {
            self.parse_if_stmt()
        } else if self.at_keyword("while") {
            self.parse_while_stmt()
        } else if self.at_keyword("do") {
            self.parse_do_while_stmt()
        } else if self.at_keyword("switch") {
            self.parse_switch_stmt()
        } else if self.at_keyword("for") {
            self.parse_for_stmt()
        } else if self.starts_var_decl() {
            self.parse_var_decl_stmt(context)
        } else if self.at_keyword("return") {
            self.parse_return_stmt(context)
        } else if self.at_keyword("break") {
            self.parse_break_stmt(context)
        } else if self.at_keyword("continue") {
            self.parse_continue_stmt(context)
        } else {
            self.parse_invocation_or_expr_stmt(context)
        }?;

        if !self.record_statement_budget(stmt_range(&stmt)) {
            return None;
        }
        Some(stmt)
    }

    pub(super) fn parse_block_stmt(&mut self) -> Option<Stmt> {
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

    pub(super) fn parse_if_stmt(&mut self) -> Option<Stmt> {
        let if_token = self.eat_keyword("if")?;
        let paren_range = self.current().range;
        self.expect(TokenKind::LParen, "expected '(' after if")?;
        let condition = self.with_nesting(paren_range, |parser| parser.parse_expr())?;

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
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch,
            range: text_range(range_start(if_token.range), end),
        })
    }

    pub(super) fn parse_while_stmt(&mut self) -> Option<Stmt> {
        let while_token = self.eat_keyword("while")?;
        let paren_range = self.current().range;
        self.expect(TokenKind::LParen, "expected '(' after while")?;

        let condition = if self.at(TokenKind::RParen) {
            let range = self.current().range;
            self.error("expected while condition", range);
            Expr::Ident {
                name_range: range,
                range,
            }
        } else if let Some(expr) = self.with_nesting(paren_range, |parser| parser.parse_expr()) {
            expr
        } else {
            let range = self.current().range;
            self.error("expected while condition", range);
            Expr::Ident {
                name_range: range,
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
            condition: Box::new(condition),
            range: text_range(range_start(while_token.range), range_end(stmt_range(&body))),
            body: Box::new(body),
        })
    }

    pub(super) fn parse_do_while_stmt(&mut self) -> Option<Stmt> {
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
                condition: Box::new(Expr::Ident {
                    name_range: range,
                    range,
                }),
                range: text_range(range_start(do_token.range), body_end.max(range_end(range))),
            });
        };

        self.expect(TokenKind::LParen, "expected '(' after while")?;
        let paren_range = self.previous_range();

        let condition = if self.at(TokenKind::RParen) {
            let range = self.current().range;
            self.error("expected do-while condition", range);
            Expr::Ident {
                name_range: range,
                range,
            }
        } else if let Some(expr) = self.with_nesting(paren_range, |parser| parser.parse_expr()) {
            expr
        } else {
            let range = self.current().range;
            self.error("expected do-while condition", range);
            Expr::Ident {
                name_range: range,
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
            condition: Box::new(condition),
            range: text_range(range_start(do_token.range), end),
        })
    }

    pub(super) fn parse_switch_stmt(&mut self) -> Option<Stmt> {
        let switch_token = self.eat_keyword("switch")?;
        let paren_range = self.current().range;
        self.expect(TokenKind::LParen, "expected '(' after switch")?;

        let control = if self.at(TokenKind::RParen) {
            let range = self.current().range;
            self.error("expected switch control expression", range);
            Expr::Ident {
                name_range: range,
                range,
            }
        } else if let Some(expr) = self.with_nesting(paren_range, |parser| parser.parse_expr()) {
            expr
        } else {
            let range = self.current().range;
            self.error("expected switch control expression", range);
            Expr::Ident {
                name_range: range,
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
            control: Box::new(control),
            clauses,
            range: text_range(range_start(switch_token.range), end),
        })
    }

    pub(super) fn parse_switch_clause(&mut self) -> Option<SwitchClause> {
        let start = range_start(self.current().range);
        let (label, label_end) = if self.at_keyword("case") {
            self.bump();
            let value = if self.at(TokenKind::Colon) {
                let range = self.current().range;
                self.error("expected case value", range);
                Expr::Ident {
                    name_range: range,
                    range,
                }
            } else if let Some(expr) = self.parse_expr() {
                expr
            } else {
                let range = self.current().range;
                self.error("expected case value", range);
                Expr::Ident {
                    name_range: range,
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

    pub(super) fn parse_for_stmt(&mut self) -> Option<Stmt> {
        let for_token = self.eat_keyword("for")?;
        let paren_range = self.current().range;
        self.expect(TokenKind::LParen, "expected '(' after for")?;

        let checkpoint = self.pos;
        self.rewind_floor = Some(checkpoint);
        if let Some(binding) = self.with_nesting(paren_range, |parser| parser.parse_expr())
            && self.eat_keyword("in").is_some()
        {
            self.rewind_floor = None;
            self.prune_consumed_tokens();
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
                binding: Box::new(binding),
                iterable: Box::new(iterable),
                body: Box::new(body),
                range: text_range(range_start(for_token.range), body_end),
            });
        }

        self.rewind_floor = None;
        self.set_pos(checkpoint);

        let init = if self.at(TokenKind::Semi) {
            None
        } else {
            self.with_nesting(paren_range, |parser| parser.parse_for_clause_exprs())
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
            self.with_nesting(paren_range, |parser| parser.parse_for_clause_exprs())
        };
        let close = self.expect(TokenKind::RParen, "expected ')' after for clause")?;
        let body = self.parse_stmt(StmtContext::Nested)?;

        let body_end = range_end(stmt_range(&body)).max(range_end(close.range));
        Some(Stmt::For {
            init,
            condition: condition.map(Box::new),
            update,
            body: Box::new(body),
            range: text_range(range_start(for_token.range), body_end),
        })
    }

    pub(super) fn parse_for_clause_exprs(&mut self) -> Option<Vec<Expr>> {
        let mut exprs = Vec::new();

        loop {
            let expr = self.parse_expr()?;
            exprs.push(expr);

            if self.eat(TokenKind::Comma).is_none() {
                break;
            }

            if self.at(TokenKind::Semi) || self.at(TokenKind::RParen) {
                let range = self.current().range;
                self.error("expected expression after ',' in for clause", range);
                break;
            }
        }

        Some(exprs)
    }

    pub(super) fn parse_return_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
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

    pub(super) fn parse_break_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let token = self.eat_keyword("break")?;
        let end = self.expect_stmt_terminator("expected ';' after break statement", context)?;
        Some(Stmt::Break {
            range: text_range(range_start(token.range), end),
        })
    }

    pub(super) fn parse_continue_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let token = self.eat_keyword("continue")?;
        let end = self.expect_stmt_terminator("expected ';' after continue statement", context)?;
        Some(Stmt::Continue {
            range: text_range(range_start(token.range), end),
        })
    }

    pub(super) fn parse_command_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let start = range_start(self.current().range);
        let invoke = self.parse_shell_like_invoke(false)?;
        let end = self.expect_stmt_terminator("expected ';' after statement", context)?;
        Some(Stmt::Expr {
            expr: Expr::Invoke(Box::new(invoke)),
            range: text_range(start, end),
        })
    }

    pub(super) fn parse_expr_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let start = range_start(self.current().range);
        let expr = self.parse_expr()?;
        let end = self.expect_stmt_terminator("expected ';' after statement", context)?;
        Some(Stmt::Expr {
            expr,
            range: text_range(start, end),
        })
    }

    pub(super) fn parse_invocation_or_expr_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        if self.policy.expression_syntax && self.starts_expression_stmt_in_expression_mode() {
            return self.parse_expr_stmt(context);
        }

        if self.starts_command_stmt() {
            return self.parse_command_stmt(context);
        }

        self.parse_expr_stmt(context)
    }

    pub(super) fn starts_var_decl(&mut self) -> bool {
        if self.at_keyword("global") {
            if self.peek_keyword() == Some("proc") {
                return false;
            }

            return self.peek_keyword().is_some_and(is_type_keyword);
        }

        let current = self.current();
        current.kind == TokenKind::Ident && is_type_keyword(self.token_text(current))
    }

    pub(super) fn starts_command_stmt(&mut self) -> bool {
        self.at(TokenKind::Ident) && !self.starts_function_stmt()
    }

    pub(super) fn starts_expression_stmt_in_expression_mode(&mut self) -> bool {
        if !self.at(TokenKind::Ident) {
            return true;
        }

        let head_index = self.current_index();
        let next_index = self.next_significant_index(head_index + 1);
        matches!(
            self.token_at(next_index).kind,
            TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::Dot
                | TokenKind::Question
                | TokenKind::Semi
                | TokenKind::RBrace
                | TokenKind::Eof
                | TokenKind::PlusPlus
                | TokenKind::MinusMinus
                | TokenKind::Assign
                | TokenKind::PlusEq
                | TokenKind::MinusEq
                | TokenKind::StarEq
                | TokenKind::SlashEq
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::Caret
                | TokenKind::Lt
                | TokenKind::Le
                | TokenKind::Gt
                | TokenKind::Ge
                | TokenKind::EqEq
                | TokenKind::NotEq
                | TokenKind::AndAnd
                | TokenKind::OrOr
        )
    }

    pub(super) fn starts_function_stmt(&mut self) -> bool {
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

    pub(super) fn parse_var_decl_stmt(&mut self, context: StmtContext) -> Option<Stmt> {
        let start = range_start(self.current().range);
        let is_global = self.eat_keyword("global").is_some();
        let type_token = self.eat(TokenKind::Ident)?;
        let ty = parse_type_name(self.token_text(type_token))?;
        let mut declarators = Vec::new();

        loop {
            match self.parse_declarator() {
                Some(declarator) => declarators.push(declarator),
                None => {
                    let range = self.current().range;
                    self.error("expected variable declarator", range);
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

    pub(super) fn parse_declarator(&mut self) -> Option<Declarator> {
        let dollar = self.eat(TokenKind::Dollar)?;
        let ident = if let Some(token) = self.eat(TokenKind::Ident) {
            token
        } else {
            let range = self.current().range;
            self.error("expected identifier after '$'", range);
            return Some(Declarator {
                name_range: dollar.range,
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
                    let range = self.current().range;
                    self.error("expected expression after '='", range);
                    None
                }
            }
        } else {
            None
        };

        Some(Declarator {
            name_range: text_range(range_start(dollar.range), range_end(ident.range)),
            array_size,
            initializer,
            range: text_range(range_start(dollar.range), end),
        })
    }

    pub(super) fn parse_decl_array_suffix(&mut self) -> (Option<Expr>, u32) {
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
}
