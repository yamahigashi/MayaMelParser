use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BarewordScanMode {
    Expr,
    Shell,
}

impl BarewordScanMode {
    pub(super) fn allows_trailing_pipe(self, end_kind: TokenKind) -> bool {
        matches!(self, Self::Expr) && end_kind == TokenKind::Pipe
    }
}

impl<'a> Parser<'a> {
    pub(super) fn scan_shell_path_like_bareword_end(
        &mut self,
        start_index: usize,
    ) -> Option<usize> {
        self.scan_path_like_bareword_end_with_mode(start_index, BarewordScanMode::Shell)
    }

    pub(super) fn scan_path_like_bareword_end(&mut self, start_index: usize) -> Option<usize> {
        self.scan_path_like_bareword_end_with_mode(start_index, BarewordScanMode::Expr)
    }

    fn scan_path_like_bareword_end_with_mode(
        &mut self,
        start_index: usize,
        mode: BarewordScanMode,
    ) -> Option<usize> {
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
                expecting_segment_start = true;
            }
            TokenKind::Ident | TokenKind::Star => {
                index = self.consume_bareword_atom_run(start_index, index, &mut end_index);
                index = self.consume_bareword_namespace_chain(index, &mut end_index);
            }
            _ => return None,
        }

        loop {
            if expecting_segment_start {
                let segment_start = index;
                index = self.consume_bareword_atom_run(end_index, index, &mut end_index);
                if segment_start == index {
                    return mode
                        .allows_trailing_pipe(self.token_at(end_index).kind)
                        .then_some(end_index);
                }

                index = self.consume_bareword_namespace_chain(index, &mut end_index);
                expecting_segment_start = false;
                continue;
            }

            if matches!(self.kind_at(index), Some(TokenKind::Pipe))
                && self.tokens_are_adjacent(end_index, index)
            {
                end_index = index;
                index += 1;
                expecting_segment_start = true;
                continue;
            }

            if matches!(
                (self.kind_at(index), self.kind_at(index + 1)),
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

            let _ = self.consume_bareword_atom_run(end_index, index, &mut end_index);
            break;
        }

        if expecting_segment_start {
            return None;
        }

        Some(end_index)
    }

    pub(super) fn consume_bareword_atom_run(
        &mut self,
        adjacent_to: usize,
        mut index: usize,
        end_index: &mut usize,
    ) -> usize {
        while matches!(
            self.kind_at(index),
            Some(TokenKind::Ident | TokenKind::Star)
        ) && self.tokens_are_adjacent(adjacent_to.max(*end_index), index)
        {
            *end_index = index;
            index += 1;
        }

        index
    }

    pub(super) fn consume_bareword_namespace_chain(
        &mut self,
        mut index: usize,
        end_index: &mut usize,
    ) -> usize {
        while matches!(
            (self.kind_at(index), self.kind_at(index + 1)),
            (
                Some(TokenKind::Colon),
                Some(TokenKind::Ident | TokenKind::Star)
            )
        ) && self.tokens_are_adjacent(*end_index, index)
            && self.tokens_are_adjacent(index, index + 1)
        {
            *end_index = index + 1;
            index += 2;
            while matches!(
                self.kind_at(index),
                Some(TokenKind::Ident | TokenKind::Star)
            ) && self.tokens_are_adjacent(*end_index, index)
            {
                *end_index = index;
                index += 1;
            }
        }

        index
    }

    pub(super) fn bareword_bracket_suffix_end(&mut self, start_index: usize) -> Option<usize> {
        if self.token_at(start_index).kind != TokenKind::LBracket {
            return None;
        }

        match (
            self.kind_at(start_index + 1),
            self.kind_at(start_index + 2),
            self.kind_at(start_index + 3),
        ) {
            (Some(TokenKind::IntLiteral), Some(TokenKind::RBracket), _) => Some(start_index + 2),
            (Some(TokenKind::Dollar), Some(TokenKind::Ident), Some(TokenKind::RBracket)) => {
                Some(start_index + 3)
            }
            _ => None,
        }
    }
}
