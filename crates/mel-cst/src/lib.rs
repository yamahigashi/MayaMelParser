#![forbid(unsafe_code)]
//! Minimal CST scaffold.
//!
//! The long-term goal is a lossless syntax layer. For now this crate only defines
//! a tiny node structure so the workspace and documentation have a stable anchor.

use mel_syntax::{TextRange, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CstKind {
    SourceFile,
    Item,
    Statement,
    Expression,
    Token(TokenKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CstNode {
    pub kind: CstKind,
    pub range: TextRange,
    pub children: Vec<CstNode>,
}

impl CstNode {
    #[must_use]
    pub fn new(kind: CstKind, range: TextRange) -> Self {
        Self {
            kind,
            range,
            children: Vec::new(),
        }
    }

    pub fn push_child(&mut self, child: CstNode) {
        self.children.push(child);
    }
}

#[cfg(test)]
mod tests {
    use super::{CstKind, CstNode};
    use mel_syntax::text_range;

    #[test]
    fn node_can_store_children() {
        let mut root = CstNode::new(CstKind::SourceFile, text_range(0, 1));
        root.push_child(CstNode::new(CstKind::Item, text_range(0, 1)));
        assert_eq!(root.children.len(), 1);
    }
}
