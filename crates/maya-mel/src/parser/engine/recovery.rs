use super::*;

impl<'a> Parser<'a> {
    pub(super) fn expect_stmt_terminator(
        &mut self,
        message: &'static str,
        context: StmtContext,
    ) -> Option<u32> {
        if self.is_halted() {
            return Some(range_end(self.previous_significant_range()));
        }
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

    pub(super) fn can_omit_stmt_semi(&mut self, context: StmtContext) -> bool {
        matches!(self.options.mode, ParseMode::AllowTrailingStmtWithoutSemi)
            && matches!(context, StmtContext::TopLevel)
            && self.at(TokenKind::Eof)
    }

    pub(super) fn error(&mut self, message: &'static str, range: TextRange) {
        if self.is_halted() {
            return;
        }
        if self.tokens.has_budget_error() && self.current().kind == TokenKind::Eof {
            return;
        }
        self.errors.push(ParseError { message, range });
    }

    pub(super) fn recover_to_decl_boundary(&mut self) {
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Comma) && !self.at(TokenKind::Semi) {
            self.bump();
        }
    }

    pub(super) fn recover_to_proc_param_boundary(&mut self) {
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::Comma)
            && !self.at(TokenKind::RParen)
            && !self.at(TokenKind::LBrace)
        {
            self.bump();
        }
    }

    pub(super) fn recover_to_brace_list_boundary(&mut self) {
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Comma) && !self.at(TokenKind::RBrace)
        {
            self.bump();
        }
    }

    pub(super) fn recover_to_vector_literal_boundary(&mut self) {
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Comma) && !self.at(TokenKind::GtGt) {
            self.bump();
        }
    }

    pub(super) fn recover_to_switch_clause_boundary(&mut self) {
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::RBrace)
            && !self.at_keyword("case")
            && !self.at_keyword("default")
        {
            self.bump();
        }
    }

    pub(super) fn recover_to_rparen_or_stmt_boundary(&mut self) {
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::RParen)
            && !self.at(TokenKind::Comma)
            && !self.at(TokenKind::Semi)
            && !self.at(TokenKind::RBrace)
        {
            self.bump();
        }
    }

    pub(super) fn recover_to_stmt_boundary(&mut self) {
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::Semi)
            && !self.at(TokenKind::RBrace)
            && !self.at_stmt_recovery_boundary()
        {
            self.bump();
        }
    }

    pub(super) fn at_stmt_recovery_boundary(&mut self) -> bool {
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
}
