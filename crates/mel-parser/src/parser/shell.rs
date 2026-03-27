use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_shell_like_invoke(&mut self, captured: bool) -> Option<InvokeExpr> {
        let head_token = self.eat(TokenKind::Ident)?;
        let mut words = Vec::new();

        while !self.at(TokenKind::Eof) && !self.at_shell_terminator(captured) {
            if captured && self.at_captured_shell_recovery_boundary() {
                break;
            }

            if self.at(TokenKind::Flag) {
                let flag = self.bump();
                words.push(ShellWord::Flag {
                    text: flag.range,
                    range: flag.range,
                });
                continue;
            }

            let Some(word) = self.parse_shell_word(captured) else {
                let error_index = self.current_index();
                let range = self.current().range;
                self.error("unexpected token in command invocation", range);
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
                head_range: head_token.range,
                words,
                captured,
            },
            range: text_range(range_start(head_token.range), end),
        })
    }

    pub(super) fn parse_backquoted_invoke(&mut self) -> Option<InvokeExpr> {
        let open = self.eat(TokenKind::Backquote)?;

        let invoke = if self.current().kind == TokenKind::Ident {
            self.parse_shell_like_invoke(true).unwrap_or(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head_range: open.range,
                    words: Vec::new(),
                    captured: true,
                },
                range: open.range,
            })
        } else {
            let range = self.current().range;
            self.error("expected command name after backquote", range);
            InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head_range: open.range,
                    words: Vec::new(),
                    captured: true,
                },
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

    pub(super) fn at_shell_terminator(&mut self, captured: bool) -> bool {
        if captured {
            self.at(TokenKind::Backquote)
        } else {
            self.at(TokenKind::Semi) || self.at(TokenKind::RBrace)
        }
    }

    pub(super) fn at_captured_shell_recovery_boundary(&mut self) -> bool {
        self.at(TokenKind::RParen) || self.at(TokenKind::Semi) || self.at(TokenKind::RBrace)
    }

    pub(super) fn parse_shell_word(&mut self, captured: bool) -> Option<ShellWord> {
        match self.current().kind {
            TokenKind::Dot if self.peek_kind() == Some(TokenKind::Dot) => {
                self.parse_punct_bareword_shell_word()
            }
            TokenKind::StringLiteral => {
                let token = self.bump();
                Some(ShellWord::QuotedString {
                    text: token.range,
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

    pub(super) fn parse_path_like_bareword_shell_word(&mut self) -> Option<ShellWord> {
        let start_index = self.current_index();
        let end_index = self.scan_shell_path_like_bareword_end(start_index)?;

        let start = range_start(self.token_at(start_index).range);
        let end = range_end(self.token_at(end_index).range);
        let range = text_range(start, end);
        self.set_pos(end_index + 1);

        Some(ShellWord::BareWord { text: range, range })
    }

    pub(super) fn parse_punct_bareword_shell_word(&mut self) -> Option<ShellWord> {
        if self.at(TokenKind::Dot) && self.peek_kind() == Some(TokenKind::Dot) {
            let first = self.bump();
            let second = self.bump();
            let range = text_range(range_start(first.range), range_end(second.range));
            return Some(ShellWord::BareWord { text: range, range });
        }

        None
    }
    pub(super) fn parse_numeric_shell_word(&mut self) -> Option<ShellWord> {
        match self.current().kind {
            TokenKind::IntLiteral | TokenKind::FloatLiteral => {
                let token = self.bump();
                Some(ShellWord::NumericLiteral {
                    text: token.range,
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
                Some(ShellWord::NumericLiteral { text: range, range })
            }
            TokenKind::Dot
                if self.peek_kind().is_some_and(|kind| {
                    matches!(kind, TokenKind::IntLiteral | TokenKind::FloatLiteral)
                }) =>
            {
                let dot = self.bump();
                let literal = self.bump();
                let range = text_range(range_start(dot.range), range_end(literal.range));
                Some(ShellWord::NumericLiteral { text: range, range })
            }
            _ => None,
        }
    }

    pub(super) fn parse_spaced_flag_shell_word(&mut self) -> Option<ShellWord> {
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
        Some(ShellWord::Flag { text: range, range })
    }

    pub(super) fn parse_brace_list_shell_word(&mut self) -> Option<ShellWord> {
        let expr = self.parse_brace_list_expr()?;
        let range = expr.range();
        Some(ShellWord::BraceList {
            expr: Box::new(expr),
            range,
        })
    }

    pub(super) fn parse_vector_literal_shell_word(&mut self) -> Option<ShellWord> {
        let expr = self.parse_vector_literal_expr()?;
        let range = expr.range();
        Some(ShellWord::VectorLiteral {
            expr: Box::new(expr),
            range,
        })
    }

    pub(super) fn parse_variable_shell_word(&mut self) -> Option<ShellWord> {
        let mut expr = self.parse_variable_expr()?;

        loop {
            if self.at(TokenKind::Dot) {
                self.bump();
                if self.current().kind != TokenKind::Ident {
                    let range = self.current().range;
                    self.error("expected member name after '.'", range);
                    break;
                }

                let member_token = self.bump();
                let member_name = self.token_text(member_token);
                let range = text_range(range_start(expr.range()), range_end(member_token.range));

                expr = if let Some(component) = parse_vector_component_name(member_name) {
                    Expr::ComponentAccess {
                        range,
                        target: Box::new(expr),
                        component,
                    }
                } else {
                    Expr::MemberAccess {
                        range,
                        target: Box::new(expr),
                        member: member_token.range,
                    }
                };
                continue;
            }

            if self.at(TokenKind::LBracket) {
                let open = self.bump();
                let index = if let Some(index) = self.parse_expr() {
                    index
                } else {
                    let range = self.current().range;
                    self.error("expected expression inside index", range);
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
        Some(ShellWord::Variable {
            expr: Box::new(expr),
            range,
        })
    }

    pub(super) fn parse_grouped_shell_word(&mut self) -> Option<ShellWord> {
        let open = self.eat(TokenKind::LParen)?;
        let expr = self.parse_expr()?;
        let end = if let Some(close) = self.eat(TokenKind::RParen) {
            range_end(close.range)
        } else {
            let range = self.current().range;
            self.error("expected ')' to close grouped expression", range);
            self.recover_to_rparen_or_stmt_boundary();
            self.eat(TokenKind::RParen)
                .map(|token| range_end(token.range))
                .unwrap_or_else(|| range_end(self.previous_range()).max(range_end(open.range)))
        };

        Some(ShellWord::GroupedExpr {
            expr: Box::new(expr),
            range: text_range(range_start(open.range), end),
        })
    }

    pub(super) fn parse_capture_shell_word(&mut self) -> Option<ShellWord> {
        let invoke = self.parse_backquoted_invoke()?;
        Some(ShellWord::Capture {
            range: invoke.range,
            invoke: Box::new(invoke),
        })
    }

    pub(super) fn recover_to_shell_word_boundary(&mut self, captured: bool) {
        while !self.at(TokenKind::Eof) && !self.at_shell_terminator(captured) {
            if self.at(TokenKind::Flag) || self.can_start_shell_word(captured) {
                break;
            }
            self.bump();
        }
    }

    pub(super) fn can_start_shell_word(&mut self, captured: bool) -> bool {
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
}
