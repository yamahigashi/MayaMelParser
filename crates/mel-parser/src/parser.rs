use mel_ast::{
    AssignOp, BinaryOp, Declarator, Expr, InvokeExpr, InvokeSurface, Item, ProcDef, ProcParam,
    ProcReturnType, ShellWord, SourceFile, Stmt, SwitchClause, SwitchLabel, TypeName, UnaryOp,
    UpdateOp, VarDecl, VectorComponent,
};
use mel_lexer::lex_significant;
use mel_syntax::{SourceMap, TextRange, Token, TokenKind, range_end, range_start, text_range};

use crate::{Parse, ParseError, ParseMode, ParseOptions, SourceEncoding};

pub(crate) struct Parser<'a> {
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
    pub(crate) fn new(input: &'a str, options: ParseOptions) -> Self {
        Self {
            input,
            options,
            tokens: Vec::new(),
            pos: 0,
            errors: Vec::new(),
        }
    }

    pub(crate) fn parse(mut self) -> Parse {
        let lexed = lex_significant(self.input);
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
            name_range: name_token.range,
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
                name_range: range,
                range,
            }
        } else if let Some(expr) = self.parse_expr() {
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
                    name_range: range,
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
                name_range: range,
                range,
            }
        } else if let Some(expr) = self.parse_expr() {
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
                name_range: range,
                range,
            }
        } else if let Some(expr) = self.parse_expr() {
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
            expr: Expr::Invoke(Box::new(invoke)),
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
                    self.error("expected expression after '='", self.current().range);
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

    fn parse_path_like_bareword_expr(&mut self) -> Option<Expr> {
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
        self.pos = end_index + 1;

        Some(Expr::BareWord { text: range, range })
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
                name_range: dollar.range,
                range: dollar.range,
            });
        };

        Some(Expr::Ident {
            name_range: text_range(range_start(dollar.range), range_end(ident.range)),
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
            surface: InvokeSurface::Function {
                head_range: name_token.range,
                args,
            },
            range: text_range(range_start(name_token.range), end),
        })
    }

    fn parse_shell_like_invoke(&mut self, captured: bool) -> Option<InvokeExpr> {
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
                head_range: head_token.range,
                words,
                captured,
            },
            range: text_range(range_start(head_token.range), end),
        })
    }

    fn parse_backquoted_invoke(&mut self) -> Option<InvokeExpr> {
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
            self.error(
                "expected command name after backquote",
                self.current().range,
            );
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

    fn parse_path_like_bareword_shell_word(&mut self) -> Option<ShellWord> {
        let start_index = self.current_index();
        let end_index = self.scan_shell_path_like_bareword_end(start_index)?;

        let start = range_start(self.token_at(start_index).range);
        let end = range_end(self.token_at(end_index).range);
        let range = text_range(start, end);
        self.pos = end_index + 1;

        Some(ShellWord::BareWord { text: range, range })
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
                ) && self.tokens_are_adjacent(end_index, index)
                {
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
                ) && self.tokens_are_adjacent(end_index, index)
                    && self.tokens_are_adjacent(index, index + 1)
                {
                    end_index = index + 1;
                    index += 2;
                    while matches!(
                        self.tokens.get(index).map(|token| token.kind),
                        Some(TokenKind::Ident | TokenKind::Star)
                    ) && self.tokens_are_adjacent(end_index, index)
                    {
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
                ) && self.tokens_are_adjacent(end_index, index)
                {
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
                ) && self.tokens_are_adjacent(end_index, index)
                    && self.tokens_are_adjacent(index, index + 1)
                {
                    end_index = index + 1;
                    index += 2;
                    while matches!(
                        self.tokens.get(index).map(|token| token.kind),
                        Some(TokenKind::Ident | TokenKind::Star)
                    ) && self.tokens_are_adjacent(end_index, index)
                    {
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
            ) && self.tokens_are_adjacent(end_index, index)
            {
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
            ) && self.tokens_are_adjacent(end_index, index)
                && self.tokens_are_adjacent(index, index + 1)
            {
                end_index = index + 1;
                index += 2;
                continue;
            }

            if self.tokens_are_adjacent(end_index, index)
                && let Some(suffix_end) = self.bareword_bracket_suffix_end(index)
            {
                end_index = suffix_end;
                index = suffix_end + 1;
                continue;
            }

            while matches!(
                self.tokens.get(index).map(|token| token.kind),
                Some(TokenKind::Ident | TokenKind::Star)
            ) && self.tokens_are_adjacent(end_index, index)
            {
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
                ) && self.tokens_are_adjacent(end_index, index)
                {
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
                ) && self.tokens_are_adjacent(end_index, index)
                    && self.tokens_are_adjacent(index, index + 1)
                {
                    end_index = index + 1;
                    index += 2;
                    while matches!(
                        self.tokens.get(index).map(|token| token.kind),
                        Some(TokenKind::Ident | TokenKind::Star)
                    ) && self.tokens_are_adjacent(end_index, index)
                    {
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
                ) && self.tokens_are_adjacent(end_index, index)
                {
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
                ) && self.tokens_are_adjacent(end_index, index)
                    && self.tokens_are_adjacent(index, index + 1)
                {
                    end_index = index + 1;
                    index += 2;
                    while matches!(
                        self.tokens.get(index).map(|token| token.kind),
                        Some(TokenKind::Ident | TokenKind::Star)
                    ) && self.tokens_are_adjacent(end_index, index)
                    {
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
            ) && self.tokens_are_adjacent(end_index, index)
            {
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
            ) && self.tokens_are_adjacent(end_index, index)
                && self.tokens_are_adjacent(index, index + 1)
            {
                end_index = index + 1;
                index += 2;
                continue;
            }

            if self.tokens_are_adjacent(end_index, index)
                && let Some(suffix_end) = self.bareword_bracket_suffix_end(index)
            {
                end_index = suffix_end;
                index = suffix_end + 1;
                continue;
            }

            while matches!(
                self.tokens.get(index).map(|token| token.kind),
                Some(TokenKind::Ident | TokenKind::Star)
            ) && self.tokens_are_adjacent(end_index, index)
            {
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
            return Some(ShellWord::BareWord { text: range, range });
        }

        None
    }

    fn parse_numeric_shell_word(&mut self) -> Option<ShellWord> {
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
        Some(ShellWord::Flag { text: range, range })
    }

    fn parse_brace_list_shell_word(&mut self) -> Option<ShellWord> {
        let expr = self.parse_brace_list_expr()?;
        let range = expr.range();
        Some(ShellWord::BraceList {
            expr: Box::new(expr),
            range,
        })
    }

    fn parse_vector_literal_shell_word(&mut self) -> Option<ShellWord> {
        let expr = self.parse_vector_literal_expr()?;
        let range = expr.range();
        Some(ShellWord::VectorLiteral {
            expr: Box::new(expr),
            range,
        })
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
        Some(ShellWord::Variable {
            expr: Box::new(expr),
            range,
        })
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
            expr: Box::new(expr),
            range: text_range(range_start(open.range), end),
        })
    }

    fn parse_capture_shell_word(&mut self) -> Option<ShellWord> {
        let invoke = self.parse_backquoted_invoke()?;
        Some(ShellWord::Capture {
            range: invoke.range,
            invoke: Box::new(invoke),
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

    fn expect_stmt_terminator(
        &mut self,
        message: &'static str,
        context: StmtContext,
    ) -> Option<u32> {
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

    fn expect(&mut self, kind: TokenKind, message: &'static str) -> Option<Token> {
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

    fn error(&mut self, message: &'static str, range: TextRange) {
        self.errors.push(ParseError { message, range });
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

    fn tokens_are_adjacent(&self, left_index: usize, right_index: usize) -> bool {
        if right_index <= left_index {
            return false;
        }

        let left = self.token_at(left_index);
        let right = self.token_at(right_index);
        range_end(left.range) as usize == range_start(right.range) as usize
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
