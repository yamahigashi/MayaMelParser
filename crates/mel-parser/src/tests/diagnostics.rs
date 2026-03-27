use super::*;

#[test]
fn reports_missing_ternary_colon_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/missing-ternary-colon.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected ':' in ternary expression"
    );
}

#[test]
fn reports_missing_prefix_update_operand_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/missing-prefix-update-operand.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after prefix update"
    );
}

#[test]
fn reports_missing_do_while_semi_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/missing-do-while-semi.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected ';' after do-while statement"
    );
}

#[test]
fn reports_missing_for_clause_expr_after_comma_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/missing-for-clause-expr-after-comma.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after ',' in for clause"
    );
}

#[test]
fn reports_missing_var_declarator_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/missing-var-declarator.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected variable declarator");
}

#[test]
fn reports_missing_while_condition_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/missing-while-condition.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected while condition");
}

#[test]
fn reports_missing_while_body_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/missing-while-body.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected while body");
}

#[test]
fn reports_missing_switch_case_value_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/missing-switch-case-value.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected case value");
}

#[test]
fn reports_missing_switch_colon_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/missing-switch-colon.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected ':' after switch label");
}

#[test]
fn reports_missing_unary_negate_operand_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/missing-unary-negate-operand.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after unary operator"
    );
}

#[test]
fn reports_missing_caret_rhs_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/missing-caret-rhs.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after operator"
    );
}

#[test]
fn reports_missing_brace_list_close_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/missing-brace-list-close.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected '}' to close brace list");
}

#[test]
fn reports_missing_cast_operand_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/missing-cast-operand.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected expression after cast");
}

#[test]
fn reports_malformed_path_like_bareword_missing_segment_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/malformed-path-like-bareword-missing-segment.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected expression inside index");
}

#[test]
fn reports_missing_vector_close_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/missing-vector-close.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected '>>' to close vector literal"
    );
}

#[test]
fn reports_missing_member_name_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/missing-member-name.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected member name after '.'");
}

#[test]
fn reports_trailing_dot_float_double_dot_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/trailing-dot-float-double-dot.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected member name after '.'");
}

#[test]
fn reports_malformed_command_word_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-word.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_missing_closing_backquote_without_command_cascade_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-missing-closing-backquote.mel"
    ));
    assert_eq!(parse.errors.len(), 1);
    assert_eq!(parse.errors[0].message, "expected closing backquote");
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "optionVar");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Invoke(_))
                    ));
                    assert!(matches!(
                        words[2],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Invoke(_))
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn reports_malformed_command_signed_number_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-signed-number.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_spaced_flag_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-spaced-flag.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_single_dot_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-single-dot.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_leading_dot_no_digit_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-leading-dot-no-digit.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_spaced_dotted_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-spaced-dotted-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_spaced_pipe_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-spaced-pipe-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_empty_pipe_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-empty-pipe-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_pipe_followed_by_whitespace() {
    let parse = parse_source("select -r | spine_00;");
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_trailing_pipe_before_semicolon() {
    let parse = parse_source("select -r y_ang| ;");
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_spaced_namespace_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-spaced-namespace-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_leading_colon_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-leading-colon-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_dotted_variable_indexed_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-dotted-variable-indexed-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_brace_list_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-brace-list.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected '}' to close brace list");
}

#[test]
fn reports_malformed_command_vector_literal_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/malformed-command-vector-literal.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected '>>' to close vector literal"
    );
}

#[test]
fn recovers_missing_statement_parens_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/missing-statement-parens.mel"
    ));
    assert_eq!(parse.errors.len(), 2);
    assert_eq!(
        parse.errors[0].message,
        "expected ')' to close grouped expression"
    );
    assert_eq!(
        parse.errors[1].message,
        "expected ')' to close function invocation"
    );
    assert_eq!(parse.syntax.items.len(), 3);
    assert!(matches!(
        parse.syntax.items[2],
        Item::Stmt(ref stmt) if matches!(
            &**stmt,
            Stmt::Expr {
                expr: Expr::Assign { .. },
                ..
            }
        )
    ));
}

#[test]
fn recovers_missing_statement_semi_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/missing-statement-semi-recovery.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected ';' after statement");
    assert_eq!(parse.syntax.items.len(), 2);
    assert!(matches!(
        parse.syntax.items[1],
        Item::Stmt(ref stmt) if matches!(
            &**stmt,
            Stmt::Expr {
                expr: Expr::Assign { .. },
                ..
            }
        )
    ));
}

#[test]
fn allows_trailing_top_level_statement_without_semicolon_in_lenient_mode() {
    let parse = parse_source_with_options(
        include_str!("../../../../tests/corpus/parser/statements/trailing-statement-no-semi.mel"),
        ParseOptions {
            mode: ParseMode::AllowTrailingStmtWithoutSemi,
        },
    );
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 1);
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(
            &**stmt,
            Stmt::Expr {
                expr: Expr::Invoke(_),
                ..
            }
        )
    ));
}

#[test]
fn still_requires_semicolon_between_top_level_statements_in_lenient_mode() {
    let parse = parse_source_with_options(
        "$x = 1\n$y = 2;",
        ParseOptions {
            mode: ParseMode::AllowTrailingStmtWithoutSemi,
        },
    );
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected ';' after statement");
}

#[test]
fn still_requires_semicolon_for_nested_statement_in_lenient_mode() {
    let parse = parse_source_with_options(
        "if ($ready) print(\"hello\")",
        ParseOptions {
            mode: ParseMode::AllowTrailingStmtWithoutSemi,
        },
    );
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected ';' after statement");
}

#[test]
fn auto_detects_cp932_and_maps_ranges_to_original_bytes() {
    let source = r#"print "設定";"#;
    let (bytes, _, had_errors) = SHIFT_JIS.encode(source);
    assert!(!had_errors);

    let parse = parse_bytes(bytes.as_ref());
    assert_eq!(parse.source_encoding, SourceEncoding::Cp932);
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => match &words[0] {
                    ShellWord::QuotedString { range, .. } => {
                        assert_eq!(*range, text_range(6, bytes.len() as u32 - 1));
                    }
                    _ => panic!("expected quoted string"),
                },
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn auto_detects_sparse_cp932_bytes_near_end_of_large_file() {
    let prefix = "setAttr \".tx\" 1;\n".repeat(8192);
    let source = format!("{prefix}print \"設定\";");
    let (bytes, _, had_errors) = SHIFT_JIS.encode(&source);
    assert!(!had_errors);

    let parse = parse_bytes(bytes.as_ref());
    assert_eq!(parse.source_encoding, SourceEncoding::Cp932);
    assert!(parse.errors.is_empty());

    match parse.syntax.items.last() {
        Some(Item::Stmt(stmt)) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => match words.last() {
                    Some(ShellWord::QuotedString { range, .. }) => {
                        assert_eq!(parse.string_literal_contents(*range), Some("設定"));
                    }
                    _ => panic!("expected quoted string"),
                },
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected trailing statement"),
    }
}

#[test]
fn explicit_gbk_decode_preserves_command_surface() {
    let source = r#"print "按钮";"#;
    let (bytes, _, had_errors) = GBK.encode(source);
    assert!(!had_errors);

    let parse = parse_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Gbk);
    assert_eq!(parse.source_encoding, SourceEncoding::Gbk);
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => match &words[0] {
                    ShellWord::QuotedString { range, .. } => {
                        assert_eq!(*range, text_range(6, bytes.len() as u32 - 1));
                        assert_eq!(parse.source_slice(*head_range), "print");
                        assert_eq!(parse.source_slice(*range), "\"按钮\"");
                        assert_eq!(parse.string_literal_contents(*range), Some("按钮"));
                    }
                    _ => panic!("expected quoted string"),
                },
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn auto_detect_prefers_less_suspicious_gbk_candidate() {
    let parse = parse_bytes(b"print \"\xA0\xA1\";");
    assert_eq!(parse.source_encoding, SourceEncoding::Gbk);
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => match &words[0] {
                    ShellWord::QuotedString { range, .. } => {
                        assert_eq!(parse.string_literal_contents(*range), Some("牎"));
                    }
                    _ => panic!("expected quoted string"),
                },
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn lossy_utf8_single_invalid_byte_keeps_display_ranges_aligned() {
    let parse = parse_bytes(b"print \"A\xffB\";\nprint(;\n");
    assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
    assert_eq!(parse.decode_errors.len(), 1);
    assert_eq!(
        parse.decode_errors[0].message,
        "source is not valid UTF-8; decoded lossily"
    );
    assert_eq!(parse.decode_errors[0].range, text_range(8, 9));

    let decode_span = parse.source_map.display_range(parse.decode_errors[0].range);
    assert_eq!(&parse.source_text[decode_span], "\u{FFFD}");

    assert!(!parse.errors.is_empty());
    let parse_error_span = parse.source_map.display_range(parse.errors[0].range);
    assert_eq!(&parse.source_text[parse_error_span], ";");
}

#[test]
fn utf8_unknown_codepoint_produces_single_lex_error_and_safe_display_slice() {
    let parse = parse_source("😀;\n");
    assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
    assert_eq!(parse.decode_errors.len(), 0);
    assert_eq!(parse.lex_errors.len(), 1);
    assert_eq!(parse.lex_errors[0].message, "unknown character");
    assert_eq!(parse.lex_errors[0].range, text_range(0, 4));
    assert_eq!(parse.display_slice(parse.lex_errors[0].range), "😀");
}

#[test]
fn cp932_unknown_codepoint_remaps_to_original_source_bytes() {
    let (bytes, _, had_errors) = SHIFT_JIS.encode("設;\n");
    assert!(!had_errors);

    let parse = parse_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Cp932);
    assert_eq!(parse.source_encoding, SourceEncoding::Cp932);
    assert!(parse.decode_errors.is_empty());
    assert_eq!(parse.lex_errors.len(), 1);
    assert_eq!(parse.lex_errors[0].message, "unknown character");
    assert_eq!(parse.lex_errors[0].range, text_range(0, 2));
    assert_eq!(parse.display_slice(parse.lex_errors[0].range), "設");
}

#[test]
fn lossy_utf8_unknown_codepoint_maps_full_replacement_character() {
    let parse = parse_bytes_with_encoding(b"\xff;\n", SourceEncoding::Utf8);
    assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
    assert_eq!(parse.decode_errors.len(), 1);
    assert_eq!(parse.lex_errors.len(), 1);
    assert_eq!(parse.lex_errors[0].message, "unknown character");
    assert_eq!(parse.lex_errors[0].range, text_range(0, 1));
    assert_eq!(parse.display_slice(parse.lex_errors[0].range), "\u{FFFD}");
}

#[test]
fn lossy_utf8_truncated_sequence_maps_full_invalid_span_to_replacement() {
    let parse = parse_bytes_with_encoding(b"print \"A\xe3\x81\";\nprint(;\n", SourceEncoding::Utf8);
    assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
    assert_eq!(parse.decode_errors.len(), 1);
    assert_eq!(parse.decode_errors[0].range, text_range(8, 10));

    let decode_span = parse.source_map.display_range(parse.decode_errors[0].range);
    assert_eq!(&parse.source_text[decode_span], "\u{FFFD}");

    assert!(!parse.errors.is_empty());
    let parse_error_span = parse.source_map.display_range(parse.errors[0].range);
    assert_eq!(&parse.source_text[parse_error_span], ";");
}

#[test]
fn explicit_utf8_override_does_not_fall_back_to_cp932_auto_detection() {
    let source = r#"print "設定";"#;
    let (bytes, _, had_errors) = SHIFT_JIS.encode(source);
    assert!(!had_errors);

    let parse = parse_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Utf8);
    assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
    assert_eq!(parse.decode_errors.len(), 1);
    assert_eq!(
        parse.decode_errors[0].message,
        "source is not valid UTF-8; decoded lossily"
    );
}

#[test]
fn reports_decimal_integer_literal_overflow() {
    let parse = parse_source("int $value = 9223372036854775808;");
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "integer literal out of range");
    assert_eq!(parse.errors[0].range, text_range(13, 32));

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Int { value, .. }) => assert_eq!(*value, 0),
                _ => panic!("expected integer initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn reports_hex_integer_literal_overflow() {
    let parse = parse_source("int $value = 0x8000000000000000;");
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "integer literal out of range");
    assert_eq!(parse.errors[0].range, text_range(13, 31));

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Int { value, .. }) => assert_eq!(*value, 0),
                _ => panic!("expected integer initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parse_helpers_slice_and_unquote_string_literals() {
    let parse = parse_source("print \"hello\";");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => match &words[0] {
                    ShellWord::QuotedString { range, .. } => {
                        assert_eq!(parse.source_slice(*range), "\"hello\"");
                        assert_eq!(parse.display_slice(*range), "\"hello\"");
                        assert_eq!(parse.string_literal_contents(*range), Some("hello"));
                    }
                    _ => panic!("expected quoted string shell word"),
                },
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}
