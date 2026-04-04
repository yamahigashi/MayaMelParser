use maya_mel as mel_lexer;
use maya_mel as mel_syntax;
use mel_lexer::{lex, lex_significant, lexer};
use mel_syntax::TokenKind;

#[test]
fn streaming_lexer_matches_eager_lex_output() {
    let input = "int $a = 1;\n// comment\nprint $a;";
    let eager = lex(input);

    let mut cursor = lexer(input);
    let streamed_tokens: Vec<_> = cursor.by_ref().collect();
    let streamed_diagnostics = cursor.finish();

    assert_eq!(streamed_tokens, eager.tokens);
    assert_eq!(streamed_diagnostics, eager.diagnostics);
}

#[test]
fn significant_lex_drops_trivia_and_keeps_eof() {
    let input = "print\n  $value; // trailing\n";
    let lexed = lex_significant(input);

    assert!(
        lexed
            .tokens
            .iter()
            .all(|token| { token.kind == TokenKind::Eof || !token.kind.is_trivia() })
    );
    assert_eq!(
        lexed.tokens.last().map(|token| token.kind),
        Some(TokenKind::Eof)
    );
    assert!(
        lexed
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Ident)
    );
    assert!(lexed.diagnostics.is_empty());
}
