use mel_parser::parse_source;

#[test]
fn parser_handles_line_break_sensitive_command_surface_with_significant_tokens() {
    let parse = parse_source("print\n  $value;\nfoo (1);\nprint - q value;");

    assert!(parse.lex_errors.is_empty());
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 3);
}
