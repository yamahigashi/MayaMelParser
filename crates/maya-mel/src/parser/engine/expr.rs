use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_expr_bp(0)
    }

    pub(super) fn parse_expr_bp(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.parse_prefix_expr()?;

        loop {
            if self.at(TokenKind::Dot) {
                let l_bp = 90;
                if l_bp < min_bp {
                    break;
                }

                self.bump();
                if self.current().kind != TokenKind::Ident {
                    let range = self.current().range;
                    self.error("expected member name after '.'", range);
                    return Some(lhs);
                }

                let member_token = self.bump();
                let member_name = self.token_text(member_token);
                let range = text_range(range_start(lhs.range()), range_end(member_token.range));

                lhs = if let Some(component) = parse_vector_component_name(member_name) {
                    Expr::ComponentAccess {
                        range,
                        target: Box::new(lhs),
                        component,
                    }
                } else {
                    Expr::MemberAccess {
                        range,
                        target: Box::new(lhs),
                        member: member_token.range,
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
                let index = if let Some(expr) =
                    self.with_nesting(open.range, |parser| parser.parse_expr())
                {
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
                    range_end(range).max(range_end(open.range))
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
                    let range = self.current().range;
                    self.error("expected expression after '?'", range);
                    return Some(lhs);
                };

                if self.eat(TokenKind::Colon).is_none() {
                    let range = self.current().range;
                    self.error("expected ':' in ternary expression", range);
                    return Some(lhs);
                }

                let else_expr = if let Some(expr) = self.parse_expr_bp(l_bp) {
                    expr
                } else {
                    let range = self.current().range;
                    self.error("expected expression after ':'", range);
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
                let range = self.current().range;
                self.error("expected expression after operator", range);
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

    pub(super) fn parse_prefix_expr(&mut self) -> Option<Expr> {
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

    pub(super) fn parse_prefix_update_expr(&mut self, token: Token, op: UpdateOp) -> Option<Expr> {
        let expr = if let Some(expr) = self.parse_expr_bp(80) {
            expr
        } else {
            let range = self.current().range;
            self.error("expected expression after prefix update", range);
            return None;
        };

        Some(Expr::PrefixUpdate {
            op,
            range: text_range(range_start(token.range), range_end(expr.range())),
            expr: Box::new(expr),
        })
    }

    pub(super) fn parse_unary_expr(&mut self, token: Token, op: UnaryOp) -> Option<Expr> {
        let expr = if let Some(expr) = self.parse_expr_bp(80) {
            expr
        } else {
            let range = self.current().range;
            self.error("expected expression after unary operator", range);
            return None;
        };

        Some(Expr::Unary {
            op,
            range: text_range(range_start(token.range), range_end(expr.range())),
            expr: Box::new(expr),
        })
    }

    pub(super) fn parse_atom(&mut self) -> Option<Expr> {
        match self.current().kind {
            TokenKind::Dollar => self.parse_variable_expr(),
            TokenKind::Ident if self.peek_kind() == Some(TokenKind::LParen) => self
                .parse_function_invoke()
                .map(|invoke| Expr::Invoke(Box::new(invoke))),
            TokenKind::Ident if self.at_path_like_bareword_expr() => {
                self.parse_path_like_bareword_expr()
            }
            TokenKind::Pipe | TokenKind::Star | TokenKind::Colon => {
                self.parse_path_like_bareword_expr()
            }
            TokenKind::Ident => {
                let token = self.bump();
                Some(Expr::Ident {
                    name_range: token.range,
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
                    text: token.range,
                    range: token.range,
                })
            }
            TokenKind::StringLiteral => {
                let token = self.bump();
                if !self.check_literal_budget(token.range) {
                    return None;
                }
                Some(Expr::String {
                    text: token.range,
                    range: token.range,
                })
            }
            TokenKind::LtLt => self.parse_vector_literal_expr(),
            TokenKind::LBrace => self.parse_brace_list_expr(),
            TokenKind::Backquote => self
                .parse_backquoted_invoke()
                .map(|invoke| Expr::Invoke(Box::new(invoke))),
            TokenKind::LParen => self.parse_grouped_expr(),
            _ => None,
        }
    }

    pub(super) fn parse_path_like_bareword_expr(&mut self) -> Option<Expr> {
        let start_index = self.current_index();
        let end_index = self.scan_path_like_bareword_end(start_index)?;
        if start_index == end_index && self.token_at(start_index).kind == TokenKind::Ident {
            let token = self.bump();
            return Some(Expr::Ident {
                name_range: token.range,
                range: token.range,
            });
        }

        let start = range_start(self.token_at(start_index).range);
        let end = range_end(self.token_at(end_index).range);
        let range = text_range(start, end);
        self.set_pos(end_index + 1);

        Some(Expr::BareWord { text: range, range })
    }

    pub(super) fn at_path_like_bareword_expr(&mut self) -> bool {
        let start_index = self.current_index();
        let start_kind = self.token_at(start_index).kind;
        self.scan_path_like_bareword_end(start_index)
            .is_some_and(|end_index| {
                start_kind != TokenKind::Ident
                    || (start_index..=end_index).any(|index| {
                        matches!(
                            self.kind_at(index),
                            Some(TokenKind::Pipe | TokenKind::Colon)
                        )
                    })
            })
    }

    pub(super) fn parse_variable_expr(&mut self) -> Option<Expr> {
        let dollar = self.eat(TokenKind::Dollar)?;
        let ident = if let Some(token) = self.eat(TokenKind::Ident) {
            token
        } else {
            let range = self.current().range;
            self.error("expected identifier after '$'", range);
            return Some(Expr::Ident {
                name_range: dollar.range,
                range: dollar.range,
            });
        };

        Some(Expr::Ident {
            name_range: text_range(range_start(dollar.range), range_end(ident.range)),
            range: text_range(range_start(dollar.range), range_end(ident.range)),
        })
    }

    pub(super) fn at_cast_expr(&mut self) -> bool {
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

    pub(super) fn parse_cast_expr(&mut self) -> Option<Expr> {
        let open = self.eat(TokenKind::LParen)?;
        let type_token = self.eat(TokenKind::Ident)?;
        let ty = parse_type_name(self.token_text(type_token))?;
        let close = self.expect(TokenKind::RParen, "expected ')' after cast type")?;

        let expr =
            if let Some(expr) = self.with_nesting(open.range, |parser| parser.parse_expr_bp(80)) {
                expr
            } else {
                let range = self.current().range;
                self.error("expected expression after cast", range);
                Expr::Ident {
                    name_range: range,
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

    pub(super) fn parse_grouped_expr(&mut self) -> Option<Expr> {
        let open = self.eat(TokenKind::LParen)?;
        self.with_nesting(open.range, |parser| {
            let expr = parser.parse_expr()?;
            let end = if let Some(close) = parser.eat(TokenKind::RParen) {
                range_end(close.range)
            } else {
                let range = parser.current().range;
                parser.error("expected ')' to close grouped expression", range);
                parser.recover_to_rparen_or_stmt_boundary();
                parser
                    .eat(TokenKind::RParen)
                    .map_or(range_end(range), |close| range_end(close.range))
            };
            let grouped_range = text_range(range_start(open.range), end);
            if !parser.check_literal_budget(grouped_range) {
                return None;
            }
            Some(expr)
        })
    }

    pub(super) fn parse_brace_list_expr(&mut self) -> Option<Expr> {
        let open = self.eat(TokenKind::LBrace)?;
        self.with_nesting(open.range, |parser| {
            let mut elements = Vec::new();

            while !parser.at(TokenKind::RBrace) && !parser.at(TokenKind::Eof) {
                if let Some(expr) = parser.parse_expr() {
                    elements.push(expr);
                } else {
                    let range = parser.current().range;
                    parser.error("expected expression inside brace list", range);
                    parser.recover_to_brace_list_boundary();
                }

                if parser.eat(TokenKind::Comma).is_some() {
                    continue;
                }
                break;
            }

            let end = if let Some(close) = parser.eat(TokenKind::RBrace) {
                range_end(close.range)
            } else {
                let range = parser.current().range;
                parser.error("expected '}' to close brace list", range);
                range_end(range).max(range_end(open.range))
            };
            let range = text_range(range_start(open.range), end);
            if !parser.check_literal_budget(range) {
                return None;
            }
            Some(Expr::ArrayLiteral { elements, range })
        })
    }

    pub(super) fn parse_vector_literal_expr(&mut self) -> Option<Expr> {
        let open = self.eat(TokenKind::LtLt)?;
        self.with_nesting(open.range, |parser| {
            let mut elements = Vec::new();

            while !parser.at(TokenKind::GtGt) && !parser.at(TokenKind::Eof) {
                if let Some(expr) = parser.parse_expr() {
                    elements.push(expr);
                } else {
                    let range = parser.current().range;
                    parser.error("expected expression inside vector literal", range);
                    parser.recover_to_vector_literal_boundary();
                }

                if parser.eat(TokenKind::Comma).is_some() {
                    continue;
                }
                break;
            }

            let end = if let Some(close) = parser.eat(TokenKind::GtGt) {
                range_end(close.range)
            } else {
                let range = parser.current().range;
                parser.error("expected '>>' to close vector literal", range);
                range_end(range).max(range_end(open.range))
            };
            let range = text_range(range_start(open.range), end);
            if !parser.check_literal_budget(range) {
                return None;
            }
            Some(Expr::VectorLiteral { elements, range })
        })
    }

    pub(super) fn parse_function_invoke(&mut self) -> Option<InvokeExpr> {
        let name_token = self.eat(TokenKind::Ident)?;
        let open = self.eat(TokenKind::LParen)?;

        self.with_nesting(open.range, |parser| {
            let mut args = Vec::new();
            if !parser.at(TokenKind::RParen) {
                loop {
                    if let Some(expr) = parser.parse_expr() {
                        args.push(expr);
                    } else {
                        let range = parser.current().range;
                        parser.error("expected expression as function argument", range);
                        break;
                    }

                    if parser.eat(TokenKind::Comma).is_some() {
                        continue;
                    }
                    break;
                }
            }

            let end = if let Some(close) = parser.eat(TokenKind::RParen) {
                range_end(close.range)
            } else {
                let range = parser.current().range;
                parser.error("expected ')' to close function invocation", range);
                parser.recover_to_rparen_or_stmt_boundary();
                if let Some(close) = parser.eat(TokenKind::RParen) {
                    range_end(close.range)
                } else {
                    range_end(parser.previous_range()).max(range_end(range))
                }
            };

            Some(InvokeExpr {
                surface: InvokeSurface::Function {
                    head_range: name_token.range,
                    args,
                },
                range: text_range(range_start(name_token.range), end),
            })
        })
    }

    pub(super) fn parse_postfix_update_op(&mut self) -> Option<UpdateOp> {
        if self.eat(TokenKind::PlusPlus).is_some() {
            return Some(UpdateOp::Increment);
        }

        if self.eat(TokenKind::MinusMinus).is_some() {
            return Some(UpdateOp::Decrement);
        }

        None
    }
}
