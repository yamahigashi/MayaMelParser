use super::{
    LightItem, LightParseOptions, LightWord, ParseMode, ParseOptions, SourceEncoding, parse_bytes,
    parse_bytes_with_encoding, parse_file, parse_file_with_encoding, parse_light_bytes,
    parse_light_source, parse_light_source_with_options, parse_source, parse_source_view_range,
    parse_source_with_options, scan_light_bytes_with_sink, scan_light_source_with_options_and_sink,
};
use encoding_rs::{GBK, SHIFT_JIS};
use mel_ast::{
    AssignOp, BinaryOp, Expr, InvokeSurface, Item, ShellWord, Stmt, SwitchLabel, TypeName, UnaryOp,
    UpdateOp, VectorComponent,
};
use mel_syntax::text_range;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn parses_proc_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/proc/basic-global-proc.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Proc(proc_def) => {
            assert!(proc_def.is_global);
            assert_eq!(parse.source_slice(proc_def.name_range), "greetUser");
            assert!(matches!(
                proc_def.return_type,
                Some(mel_ast::ProcReturnType {
                    ty: TypeName::String,
                    is_array: false,
                    ..
                })
            ));
            assert_eq!(proc_def.params.len(), 1);
            assert!(matches!(proc_def.params[0].ty, TypeName::String));
            assert_eq!(parse.source_slice(proc_def.params[0].name_range), "$name");
            assert!(!proc_def.params[0].is_array);
            assert!(matches!(proc_def.body, Stmt::Block { .. }));
        }
        _ => panic!("expected proc item"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/proc/local-array-param-proc.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Proc(proc_def) => {
            assert!(!proc_def.is_global);
            assert!(proc_def.return_type.is_none());
            assert_eq!(proc_def.params.len(), 1);
            assert!(matches!(proc_def.params[0].ty, TypeName::Vector));
            assert!(proc_def.params[0].is_array);
            assert!(matches!(proc_def.body, Stmt::Block { .. }));
        }
        _ => panic!("expected proc item"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/proc/array-return-proc.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Proc(proc_def) => {
            assert!(matches!(
                proc_def.return_type,
                Some(mel_ast::ProcReturnType {
                    ty: TypeName::String,
                    is_array: true,
                    ..
                })
            ));
            assert_eq!(proc_def.params.len(), 1);
            assert!(matches!(proc_def.params[0].ty, TypeName::String));
            assert!(!proc_def.params[0].is_array);
        }
        _ => panic!("expected proc item"),
    }
}

#[test]
fn parse_file_reuses_owned_utf8_bytes_without_decode_diagnostics() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-utf8-{unique}.mel"));
    fs::write(&path, "print \"hello\";\n").expect("temp fixture should be writable");

    let parse = parse_file(&path).expect("utf8 temp fixture should parse");
    let parse_utf8 =
        parse_file_with_encoding(&path, SourceEncoding::Utf8).expect("utf8 parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert!(parse.decode_errors.is_empty());
    assert!(parse_utf8.decode_errors.is_empty());
    assert!(parse.errors.is_empty());
    assert!(parse_utf8.errors.is_empty());

    let Item::Stmt(stmt) = &parse.syntax.items[0] else {
        panic!("expected command statement");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke expression");
    };
    let InvokeSurface::ShellLike { head_range, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    assert_eq!(parse.source_slice(*head_range), "print");
    assert_eq!(parse_utf8.source_slice(*head_range), "print");
}

#[test]
fn parse_file_with_explicit_cp932_ascii_keeps_identity_offsets() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-cp932-ascii-{unique}.mel"));
    fs::write(&path, b"setAttr \".tx\" 1;\n").expect("temp fixture should be writable");

    let parse =
        parse_file_with_encoding(&path, SourceEncoding::Cp932).expect("cp932 parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert!(parse.decode_errors.is_empty());
    assert!(parse.errors.is_empty());
    assert_eq!(parse.source_encoding, SourceEncoding::Cp932);
    assert_eq!(parse.source_map.display_offset(3), 3);
    assert_eq!(parse.source_map.source_offset_for_display(3), 3);

    let Item::Stmt(stmt) = &parse.syntax.items[0] else {
        panic!("expected command statement");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke expression");
    };
    let InvokeSurface::ShellLike { head_range, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    assert_eq!(parse.source_slice(*head_range), "setAttr");
}

#[test]
fn parse_file_auto_detects_cp932_and_preserves_source_slices() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-cp932-auto-{unique}.mel"));
    let (bytes, _, had_errors) = SHIFT_JIS.encode("print \"設定\";\n");
    assert!(!had_errors);
    fs::write(&path, bytes.as_ref()).expect("temp fixture should be writable");

    let parse = parse_file(&path).expect("auto cp932 parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert!(parse.decode_errors.is_empty());
    assert!(parse.errors.is_empty());
    assert_eq!(parse.source_encoding, SourceEncoding::Cp932);

    let Item::Stmt(stmt) = &parse.syntax.items[0] else {
        panic!("expected command statement");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke expression");
    };
    let InvokeSurface::ShellLike { words, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    let ShellWord::QuotedString { range, .. } = &words[0] else {
        panic!("expected quoted string");
    };
    assert_eq!(parse.string_literal_contents(*range), Some("設定"));
}

#[test]
fn light_parse_keeps_proc_body_as_single_item() {
    let parse =
        parse_light_source("global proc foo() {\nsetAttr \".tx\" 1;\n}\nsetAttr \".ty\" 2;\n");
    assert!(parse.errors.is_empty());
    assert_eq!(parse.source.items.len(), 2);
    let LightItem::Proc(proc_def) = &parse.source.items[0] else {
        panic!("expected proc item");
    };
    assert!(proc_def.is_global);
    assert_eq!(
        proc_def.name_range.map(|range| parse.source_slice(range)),
        Some("foo")
    );
    let LightItem::Command(command) = &parse.source.items[1] else {
        panic!("expected command item");
    };
    assert_eq!(parse.source_slice(command.head_range), "setAttr");
}

#[test]
fn streaming_light_scan_matches_materialized_items() {
    let source = "global proc foo() { }\nsetAttr \".tx\" 1;\n";
    let materialized = parse_light_source(source);
    let mut streamed = Vec::new();
    let report = scan_light_source_with_options_and_sink(
        source,
        LightParseOptions::default(),
        &mut |_: mel_syntax::SourceView<'_>, item: LightItem| streamed.push(item),
    );

    assert_eq!(streamed, materialized.source.items);
    assert_eq!(report.errors, materialized.errors);
}

#[test]
fn light_parse_tracks_multiline_command_tail_as_single_statement() {
    let source = "setAttr \".fc[0]\" -type \"polyFaces\"\n    f 4 0 1 2 3\n    mu 0 4 0 1 2 3;\n";
    let parse = parse_light_source_with_options(
        source,
        LightParseOptions {
            max_prefix_words: 4,
            max_prefix_bytes: 48,
        },
    );
    assert!(parse.errors.is_empty());
    let LightItem::Command(command) = &parse.source.items[0] else {
        panic!("expected command item");
    };
    assert_eq!(parse.source_slice(command.head_range), "setAttr");
    assert!(command.opaque_tail.is_some());
    assert_eq!(parse.source.items.len(), 1);
    let opaque_tail = parse.source_slice(command.opaque_tail.expect("opaque tail"));
    assert!(opaque_tail.starts_with("4 0 1 2 3"));
    assert!(opaque_tail.contains("mu 0 4 0 1 2 3"));
}

#[test]
fn light_parse_bounds_prefix_words_for_large_payloads() {
    let source = "setAttr \".pt\" 1 2 3 4 5 6 7 8 9 10;\n";
    let parse = parse_light_source_with_options(
        source,
        LightParseOptions {
            max_prefix_words: 3,
            max_prefix_bytes: 24,
        },
    );
    assert!(parse.errors.is_empty());
    let LightItem::Command(command) = &parse.source.items[0] else {
        panic!("expected command item");
    };
    assert_eq!(command.words.len(), 3);
    assert!(matches!(command.words[0], LightWord::QuotedString { .. }));
    assert!(matches!(command.words[1], LightWord::NumericLiteral { .. }));
    assert!(command.opaque_tail.is_some());
}

#[test]
fn light_parse_bytes_preserves_safe_source_slices_for_non_utf8() {
    let (bytes, _, _) = SHIFT_JIS.encode("setAttr \".名\" -type \"string\" \"値\";\n");
    let parse = parse_light_bytes(bytes.as_ref());
    assert!(parse.errors.is_empty());
    let LightItem::Command(command) = &parse.source.items[0] else {
        panic!("expected command item");
    };
    assert_eq!(parse.source_slice(command.head_range), "setAttr");
    assert_eq!(parse.source_slice(command.words[0].range()), "\".名\"");
}

#[test]
fn streaming_light_scan_bytes_preserves_safe_source_slices_for_non_utf8() {
    let (bytes, _, _) = SHIFT_JIS.encode("setAttr \".名\" -type \"string\" \"値\";\n");
    let mut streamed = Vec::new();
    let report = scan_light_bytes_with_sink(
        bytes.as_ref(),
        &mut |_: mel_syntax::SourceView<'_>, item: LightItem| streamed.push(item),
    );
    assert!(report.errors.is_empty());
    let LightItem::Command(command) = &streamed[0] else {
        panic!("expected command item");
    };
    assert_eq!(report.source_slice(command.head_range), "setAttr");
    assert_eq!(report.source_slice(command.words[0].range()), "\".名\"");
}

#[test]
fn parse_source_view_range_rebases_utf8_spans_to_global_source_offsets() {
    let parse = parse_light_source("print 1;\nsetAttr \".tx\" 1;\n");
    let LightItem::Command(command) = &parse.source.items[1] else {
        panic!("expected command");
    };
    let promoted = parse_source_view_range(parse.source_view(), command.span);
    assert!(promoted.errors.is_empty());
    let Item::Stmt(stmt) = &promoted.syntax.items[0] else {
        panic!("expected statement item");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke statement");
    };
    let InvokeSurface::ShellLike { head_range, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    assert_eq!(parse.source_slice(*head_range), "setAttr");
}

#[test]
fn parse_source_view_range_rebases_cp932_spans_to_original_bytes() {
    let (bytes, _, _) = SHIFT_JIS.encode("print 1;\nsetAttr \".名\" -type \"string\" \"値\";\n");
    let parse = parse_light_bytes(bytes.as_ref());
    let LightItem::Command(command) = &parse.source.items[1] else {
        panic!("expected command");
    };
    let promoted = parse_source_view_range(parse.source_view(), command.span);
    assert!(promoted.errors.is_empty());
    let Item::Stmt(stmt) = &promoted.syntax.items[0] else {
        panic!("expected statement item");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke statement");
    };
    let InvokeSurface::ShellLike { words, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    let ShellWord::QuotedString { text, .. } = &words[0] else {
        panic!("expected quoted attr path");
    };
    assert_eq!(parse.source_slice(*text), "\".名\"");
}

#[test]
fn parse_bytes_keeps_utf8_source_text_and_identity_source_map() {
    let input = b"setAttr \".tx\" 1;\n";
    let parse = parse_bytes(input);

    assert!(parse.decode_errors.is_empty());
    assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
    assert_eq!(parse.source_text, "setAttr \".tx\" 1;\n");

    let Item::Stmt(stmt) = &parse.syntax.items[0] else {
        panic!("expected statement item");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke statement");
    };
    let InvokeSurface::ShellLike {
        head_range, words, ..
    } = &invoke.surface
    else {
        panic!("expected shell-like invoke");
    };

    assert_eq!(parse.source_slice(*head_range), "setAttr");
    let ShellWord::QuotedString { text, .. } = &words[0] else {
        panic!("expected quoted string");
    };
    assert_eq!(parse.source_slice(*text), "\".tx\"");
}

#[test]
fn explicit_cp932_ascii_only_input_keeps_identity_offsets() {
    let parse = parse_bytes_with_encoding(b"setAttr \".tx\" 1;\n", SourceEncoding::Cp932);

    assert!(parse.decode_errors.is_empty());
    assert_eq!(parse.source_encoding, SourceEncoding::Cp932);
    assert_eq!(parse.source_map.display_offset(3), 3);
    assert_eq!(parse.source_map.source_offset_for_display(3), 3);

    let Item::Stmt(stmt) = &parse.syntax.items[0] else {
        panic!("expected statement item");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke statement");
    };
    let InvokeSurface::ShellLike { head_range, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    assert_eq!(parse.source_slice(*head_range), "setAttr");
}

#[test]
fn parse_source_view_range_rebases_gbk_spans_to_original_bytes() {
    let (bytes, _, _) = GBK.encode("print 1;\nsetAttr \".名\" -type \"string\" \"值\";\n");
    let parse = parse_light_bytes(bytes.as_ref());
    let LightItem::Command(command) = &parse.source.items[1] else {
        panic!("expected command");
    };
    let promoted = parse_source_view_range(parse.source_view(), command.span);
    assert!(promoted.errors.is_empty());
    let Item::Stmt(stmt) = &promoted.syntax.items[0] else {
        panic!("expected statement item");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke statement");
    };
    let InvokeSurface::ShellLike { words, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    let ShellWord::QuotedString { text, .. } = &words[0] else {
        panic!("expected quoted attr path");
    };
    assert_eq!(parse.source_slice(*text), "\".名\"");
}

#[test]
fn malformed_cp932_decode_path_keeps_current_replacement_behavior() {
    let parse = parse_bytes_with_encoding(b"print \"\x81\";\n", SourceEncoding::Cp932);
    assert_eq!(parse.source_encoding, SourceEncoding::Cp932);
    assert_eq!(parse.decode_errors.len(), 1);
    assert!(parse.decode_errors[0].message.contains("cp932"));

    let Item::Stmt(stmt) = &parse.syntax.items[0] else {
        panic!("expected statement item");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke statement");
    };
    let InvokeSurface::ShellLike { head_range, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    assert_eq!(parse.source_slice(*head_range), "print");
}

#[test]
fn parses_nested_proc_definition_statement_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/nested-proc-definition.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Proc(proc_def) => match &proc_def.body {
            Stmt::Block { statements, .. } => {
                assert!(matches!(statements[0], Stmt::Proc { .. }));
                assert!(matches!(statements[1], Stmt::Proc { .. }));
            }
            _ => panic!("expected proc body block"),
        },
        _ => panic!("expected outer proc item"),
    }
}

#[test]
fn reports_missing_nested_proc_body() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-nested-proc-missing-body.mel"
    ));
    assert!(
        parse
            .errors
            .iter()
            .any(|error| error.message == "expected proc body block")
    );
}

#[test]
fn parses_command_statement_with_flags() {
    let parse = parse_source("frameLayout -edit -label $title $fl;");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(words[1], ShellWord::Flag { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected command statement"),
    }
}

#[test]
fn parses_command_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "print");
                    assert!(matches!(words[0], ShellWord::BareWord { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected command statement"),
    }
}

#[test]
fn parses_command_dotdot_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotdot-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "setParent");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == ".."
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
fn parses_command_dotdot_after_flag_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotdot-flag-arg.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == ".."
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
fn parses_command_dotdot_without_whitespace() {
    let parse = parse_source("setParent..;");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "setParent");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == ".."
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
fn keeps_quoted_dotdot_as_quoted_string() {
    let parse = parse_source(r#"setParent "..";"#);
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(
                        words[0],
                        ShellWord::QuotedString { ref text, .. } if parse.source_slice(*text) == "\"..\""
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
fn keeps_no_whitespace_ident_lparen_as_function_stmt() {
    let parse = parse_source("doItDRA();");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::Function {
                    head_range, args, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "doItDRA");
                    assert!(args.is_empty());
                }
                _ => panic!("expected function invoke"),
            },
            _ => panic!("expected expression statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_function_stmt_across_line_break_and_following_keyword() {
    let parse = parse_source("foo(\n    \"arg\"\n);\nif ($ready) {\n    print $ready;\n}\n");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::Function {
                    head_range, args, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "foo");
                    assert_eq!(args.len(), 1);
                }
                _ => panic!("expected function invoke"),
            },
            _ => panic!("expected expression statement"),
        },
        _ => panic!("expected statement"),
    }

    assert!(matches!(
        parse.syntax.items[1],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::If { .. })
    ));
}

#[test]
fn parses_function_stmt_spaced_lparen_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/function-stmt-spaced-lparen.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::Function {
                    head_range, args, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "tmBuildSet");
                    assert_eq!(args.len(), 2);
                }
                _ => panic!("expected function invoke"),
            },
            _ => panic!("expected expression statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_leading_grouped_arg_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-leading-grouped-arg.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "renameAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
                    ));
                    assert!(matches!(words[1], ShellWord::Variable { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_numeric_arg_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-numeric-arg.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(words[1], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[2],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "0"
                    ));
                    assert!(matches!(words[3], ShellWord::Variable { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_signed_numeric_arg_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-signed-numeric-arg.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(
                        words[3],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "-10"
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
fn parses_command_leading_dot_float_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-leading-dot-float.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == ".7"
                    ));
                    assert!(matches!(words[2], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[3],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == ".001"
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
fn parses_command_trailing_dot_float_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-trailing-dot-float.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(words[1], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[2],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "-1000."
                    ));
                    assert!(matches!(words[3], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[4],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "1000."
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
fn parses_grouped_subtraction_call_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/grouped-subtraction-call.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::Function { args, .. } => {
                        assert!(matches!(
                            args[1],
                            Expr::Binary {
                                op: BinaryOp::Sub,
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected function invoke"),
                },
                _ => panic!("expected invoke initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_spaced_flag_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-spaced-flag.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::ShellLike {
                        head_range, words, ..
                    } => {
                        assert_eq!(parse.source_slice(*head_range), "optionVar");
                        assert!(matches!(
                            words[0],
                            ShellWord::Flag { ref text, .. } if parse.source_slice(*text) == "- q"
                        ));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "LayoutPreviewResolution"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected invoke initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_multiline_grouped_args_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-multiline-grouped-args.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "connectAttr");
                    assert_eq!(words.len(), 2);
                    assert!(matches!(words[0], ShellWord::GroupedExpr { .. }));
                    assert!(matches!(words[1], ShellWord::GroupedExpr { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "setAttr");
                    assert!(matches!(words[0], ShellWord::GroupedExpr { .. }));
                    assert!(
                        matches!(words[1], ShellWord::Flag { ref text, .. } if parse.source_slice(*text) == "-type")
                    );
                    assert!(matches!(
                        words[2],
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == "double3"
                    ));
                    assert!(matches!(words[3], ShellWord::GroupedExpr { .. }));
                    assert!(matches!(words[4], ShellWord::GroupedExpr { .. }));
                    assert!(matches!(words[5], ShellWord::GroupedExpr { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_point_constraint_brace_list_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-point-constraint-brace-list.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "applyPointConstraintArgs");
                    assert!(matches!(
                        words[0],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "2"
                    ));
                    assert!(matches!(
                        words[1],
                        ShellWord::BraceList {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::ArrayLiteral { elements, .. } if elements.len() == 10)
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
fn parses_command_orient_constraint_brace_list_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-orient-constraint-brace-list.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "applyOrientConstraintArgs");
                    match &words[1] {
                        ShellWord::BraceList { expr, .. } => match &**expr {
                            Expr::ArrayLiteral { elements, .. } => {
                                assert!(matches!(
                                    elements[0],
                                    Expr::String { ref text, .. } if parse.source_slice(*text) == "\"1\""
                                ));
                                assert!(matches!(
                                    elements[7],
                                    Expr::String { ref text, .. } if parse.source_slice(*text) == "\"8\""
                                ));
                                assert!(matches!(
                                    elements[8],
                                    Expr::String { ref text, .. } if parse.source_slice(*text) == "\"\""
                                ));
                            }
                            _ => panic!("expected array literal"),
                        },
                        _ => panic!("expected brace-list shell word"),
                    }
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_capture_vector_literal_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-capture-vector-literal.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::ShellLike {
                        head_range,
                        words,
                        captured,
                        ..
                    } => {
                        assert_eq!(parse.source_slice(*head_range), "hsv_to_rgb");
                        assert!(*captured);
                        assert!(matches!(
                            words[0],
                            ShellWord::VectorLiteral {
                                ref expr,
                                ..
                            } if matches!(&**expr, Expr::VectorLiteral { elements, .. } if elements.len() == 3)
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected invoke initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "text");
                    assert!(matches!(
                        words[2],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::ComponentAccess { .. })
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
fn parses_command_dotted_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotted-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "setDrivenKeyframe");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "N_arm_01.rotateX"
                    ));
                    assert!(matches!(
                        words[2],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "N_arm_01_H.rotateX"
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
fn parses_command_dotted_indexed_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotted-indexed-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "connectAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "foo.worldMatrix[0]"
                    ));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "bar.inputWorldMatrix"
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
fn parses_command_dotted_variable_indexed_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotted-variable-indexed-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "connectAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
                    ));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "LayerRegistry.layerSlot[$index]"
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
fn parses_command_dotted_global_attr_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotted-global-attr.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "getAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "defaultRenderGlobals.hyperShadeBinList"
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
fn parses_command_pipe_dag_path_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-pipe-dag-path.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "select");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "Null|Spine_00|Tail_00"
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
fn parses_command_pipe_wildcard_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-pipe-wildcard-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "select");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == "*|_x005"
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
fn parses_command_absolute_plug_path_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-absolute-plug-path.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "defaultNavigation");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(
                        matches!(words[1], ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == "shaderNodePreview1")
                    );
                    assert!(matches!(words[2], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[3],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "|geoPreview1|geoPreviewShape1.instObjGroups[0]"
                    ));
                    assert!(matches!(words[4], ShellWord::Flag { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_namespace_pipe_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-namespace-pipe-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "select");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "ns:root|ns:spine|ns:ctrl"
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
fn parses_command_leading_colon_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-leading-colon-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::ShellLike {
                        head_range, words, ..
                    } => {
                        assert_eq!(parse.source_slice(*head_range), "camera");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                                if parse.source_slice(*text) == ":previewViewportCamera"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected invoke initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_grouped_args_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-grouped-args.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "iconTextButton");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(words[1], ShellWord::QuotedString { .. }));
                    assert!(matches!(
                        words[5],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
                    ));
                    assert!(matches!(
                        words[7],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected command statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "menuItem");
                    assert!(matches!(
                        words[1],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
                    ));
                    assert!(matches!(
                        words[3],
                        ShellWord::Variable {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::MemberAccess { member, .. } if parse.source_slice(*member) == "name")
                    ));
                    assert!(matches!(words[5], ShellWord::Capture { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected command statement"),
    }
}

#[test]
fn parses_command_capture_grouped_function_call_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-capture-grouped-function-call.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Unary { expr, .. }) => match &**expr {
                    Expr::Invoke(invoke) => match &invoke.surface {
                        InvokeSurface::ShellLike {
                            head_range,
                            words,
                            captured,
                            ..
                        } => {
                            assert_eq!(parse.source_slice(*head_range), "optionVar");
                            assert!(*captured);
                            assert!(matches!(words[0], ShellWord::Flag { .. }));
                            assert!(matches!(
                                words[1],
                                ShellWord::GroupedExpr {
                                    ref expr,
                                    ..
                                } if matches!(&**expr, Expr::Invoke(_))
                            ));
                        }
                        _ => panic!("expected shell-like capture"),
                    },
                    _ => panic!("expected invoke under unary expression"),
                },
                _ => panic!("expected unary initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

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
fn parses_index_and_add_assign() {
    let parse = parse_source("$items[$i] += 1;");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Assign { op, lhs, .. },
                ..
            } => {
                assert!(matches!(op, AssignOp::AddAssign));
                assert!(matches!(**lhs, Expr::Index { .. }));
            }
            _ => panic!("expected add-assign statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_operator_precedence() {
    let parse = parse_source("$value = 1 + 2 * 3;");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary {
                    op: BinaryOp::Add,
                    rhs,
                    ..
                } => {
                    assert!(matches!(
                        **rhs,
                        Expr::Binary {
                            op: BinaryOp::Mul,
                            ..
                        }
                    ));
                }
                _ => panic!("expected additive expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_ternary_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/ternary-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 3);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => assert!(matches!(**rhs, Expr::Ternary { .. })),
            _ => panic!("expected ternary assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_exponent_float_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/exponent-float-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 3);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => assert!(matches!(
                **rhs,
                Expr::Float { ref text, .. } if parse.source_slice(*text) == "1.0e-3"
            )),
            _ => panic!("expected exponent assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "1e+3"
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "0.0e0"
                    ));
                }
                _ => panic!("expected exponent binary expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[2] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => {
                assert!(matches!(
                    decl.declarators[0].initializer,
                    Some(Expr::ArrayLiteral { .. })
                ));
            }
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_trailing_dot_float_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/trailing-dot-float-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 3);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => assert!(matches!(
                **rhs,
                Expr::Float { ref text, .. } if parse.source_slice(*text) == "1000."
            )),
            _ => panic!("expected trailing-dot float assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "0."
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "1."
                    ));
                }
                _ => panic!("expected binary expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[2] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::ArrayLiteral { elements, .. } => {
                    assert_eq!(elements.len(), 3);
                    assert!(matches!(
                        elements[0],
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "0."
                    ));
                    assert!(matches!(
                        elements[1],
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "1."
                    ));
                    assert!(matches!(
                        elements[2],
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "2."
                    ));
                }
                _ => panic!("expected brace-list assignment"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_for_in_and_for_loop_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/while-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::While { .. })
    ));

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/for-loop-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::For { .. })
    ));

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/for-loop-multi-init-update.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::For {
                init: Some(init),
                condition: Some(_),
                update: Some(update),
                ..
            } => {
                assert_eq!(init.len(), 2);
                assert_eq!(update.len(), 2);
            }
            _ => panic!("expected classic for statement with multi-clause init/update"),
        },
        _ => panic!("expected statement"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/for-in-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::ForIn { .. })
    ));
}

#[test]
fn parses_for_in_and_for_with_shared_prefix_tokens() {
    let parse = parse_source(
        "for ($item in some_array) {\n    print $item;\n}\nfor ($i = 0; $i < 3; ++$i) {\n    print $i;\n}\n",
    );
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::ForIn { binding, .. } => match &**binding {
                Expr::Ident {
                    name_range: _,
                    range: _,
                } => {}
                _ => panic!("expected variable binding"),
            },
            _ => panic!("expected for-in statement"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::For {
                init,
                condition,
                update,
                ..
            } => {
                assert!(init.is_some());
                assert!(condition.is_some());
                assert!(update.is_some());
            }
            _ => panic!("expected classic for statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_if_else_and_break_continue_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/if-else-command.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::If { .. })
    ));

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/break-continue.mel"
    ));
    assert!(parse.errors.is_empty());
}

#[test]
fn parses_switch_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/switch-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Switch { clauses, .. } => {
                assert_eq!(clauses.len(), 3);
                assert!(matches!(clauses[0].label, SwitchLabel::Default { .. }));
                assert!(clauses[0].statements.len() == 2);
                assert!(matches!(clauses[1].label, SwitchLabel::Case(_)));
                assert!(clauses[1].statements.is_empty());
                assert!(matches!(clauses[2].label, SwitchLabel::Case(_)));
                assert!(clauses[2].statements.len() == 2);
            }
            _ => panic!("expected switch statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_postfix_update_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/postfix-update.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::PostfixUpdate { op, .. },
                ..
            } => assert!(matches!(op, UpdateOp::Increment)),
            _ => panic!("expected postfix update"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_compound_assign_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/compound-assign-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 3);

    let expected = [
        AssignOp::SubAssign,
        AssignOp::MulAssign,
        AssignOp::DivAssign,
    ];

    for (item, expected_op) in parse.syntax.items.iter().zip(expected) {
        match item {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Assign { op, .. },
                    ..
                } => assert_eq!(*op, expected_op),
                _ => panic!("expected compound assignment"),
            },
            _ => panic!("expected statement"),
        }
    }
}

#[test]
fn parses_prefix_update_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/prefix-update-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::PrefixUpdate { op, .. },
                ..
            } => assert!(matches!(op, UpdateOp::Increment)),
            _ => panic!("expected prefix increment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::PrefixUpdate { op, .. },
                ..
            } => assert!(matches!(op, UpdateOp::Decrement)),
            _ => panic!("expected prefix decrement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_do_while_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/do-while-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::DoWhile { body, .. } => {
                assert!(matches!(&**body, Stmt::Block { .. }));
            }
            _ => panic!("expected do-while statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_variable_declaration_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/var-decl-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => {
                assert!(matches!(decl.ty, TypeName::Int));
                assert_eq!(decl.declarators.len(), 1);
                assert_eq!(parse.source_slice(decl.declarators[0].name_range), "$count");
            }
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/global-var-decl.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => {
                assert!(decl.is_global);
                assert!(matches!(decl.ty, TypeName::String));
                assert!(decl.declarators[0].array_size.is_some());
                assert!(matches!(
                    decl.declarators[0].initializer,
                    Some(Expr::ArrayLiteral { .. })
                ));
            }
            _ => panic!("expected global variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/var-decl-multi-array.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => {
                assert_eq!(decl.declarators.len(), 2);
                assert!(matches!(
                    decl.declarators[1].initializer,
                    Some(Expr::ArrayLiteral { .. })
                ));
            }
            _ => panic!("expected multi declarator"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_brace_list_assignment_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/brace-list-assign.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => {
                assert!(matches!(**rhs, Expr::ArrayLiteral { .. }));
            }
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_cast_expression_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/cast-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Cast { ty, expr, .. }) => {
                    assert!(matches!(ty, TypeName::String));
                    assert!(matches!(**expr, Expr::Ident { .. }));
                }
                _ => panic!("expected string cast initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Cast { ty, expr, .. }) => {
                    assert!(matches!(ty, TypeName::Int));
                    assert!(matches!(**expr, Expr::Binary { .. }));
                }
                _ => panic!("expected int cast initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[2] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Cast { ty, expr, .. } => {
                    assert!(matches!(ty, TypeName::String));
                    assert!(matches!(**expr, Expr::Invoke(_)));
                }
                _ => panic!("expected nested string cast"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_path_like_bareword_expression_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/path-like-bareword-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::BareWord { text, .. }) => {
                    assert_eq!(parse.source_slice(*text), "AA_Bar*|mdl|_XXa0|");
                }
                _ => panic!("expected path-like bareword initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_hex_integer_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/hex-int-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(**lhs, Expr::Int { value, .. } if value == 0x8000));
                    assert!(matches!(**rhs, Expr::Int { value, .. } if value == 0x0001));
                }
                _ => panic!("expected hex integer binary expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_caret_operator_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/caret-operator-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Binary { op, .. }) => assert_eq!(op, &BinaryOp::Caret),
                _ => panic!("expected caret binary expression"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_two_element_vector_literal_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/vector-literal-two-elements.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::VectorLiteral { elements, .. }) => assert_eq!(elements.len(), 2),
                _ => panic!("expected vector literal initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_unary_negate_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/unary-negate-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::Unary {
                            op: UnaryOp::Negate,
                            ..
                        }
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::Unary {
                            op: UnaryOp::Negate,
                            ..
                        }
                    ));
                }
                _ => panic!("expected binary negate expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_vector_literal_and_component_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/vector-literal-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => {
                assert!(matches!(**rhs, Expr::VectorLiteral { .. }));
            }
            _ => panic!("expected vector assignment"),
        },
        _ => panic!("expected statement"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/vector-component-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::ComponentAccess {
                            component: VectorComponent::X,
                            ..
                        }
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::ComponentAccess {
                            component: VectorComponent::Y,
                            ..
                        }
                    ));
                }
                _ => panic!("expected binary component access"),
            },
            _ => panic!("expected vector component assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_member_access_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/member-access-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::MemberAccess { ref member, .. } if parse.source_slice(*member) == "foo"
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::MemberAccess { ref member, .. } if parse.source_slice(*member) == "bar"
                    ));
                }
                _ => panic!("expected binary member access"),
            },
            _ => panic!("expected member access assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => assert!(matches!(
                **rhs,
                Expr::MemberAccess { ref member, .. } if parse.source_slice(*member) == "name"
            )),
            _ => panic!("expected indexed member access"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn reports_missing_index_bracket_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-index-bracket.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected ']' after index expression"
    );
}

#[test]
fn reports_missing_proc_body_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/proc/missing-proc-body.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected proc body block");
}

#[test]
fn reports_missing_proc_param_name_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/proc/missing-proc-param-name.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected '$' before proc parameter name"
    );
}

#[test]
fn reports_missing_compound_assign_rhs_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-compound-assign-rhs.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after operator"
    );
}

#[test]
fn reports_missing_ternary_colon_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-ternary-colon.mel"
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
        "../../../tests/corpus/parser/expressions/missing-prefix-update-operand.mel"
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
        "../../../tests/corpus/parser/statements/missing-do-while-semi.mel"
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
        "../../../tests/corpus/parser/statements/missing-for-clause-expr-after-comma.mel"
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
        "../../../tests/corpus/parser/statements/missing-var-declarator.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected variable declarator");
}

#[test]
fn reports_missing_while_condition_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-while-condition.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected while condition");
}

#[test]
fn reports_missing_while_body_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-while-body.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected while body");
}

#[test]
fn reports_missing_switch_case_value_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-switch-case-value.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected case value");
}

#[test]
fn reports_missing_switch_colon_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-switch-colon.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected ':' after switch label");
}

#[test]
fn reports_missing_unary_negate_operand_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-unary-negate-operand.mel"
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
        "../../../tests/corpus/parser/expressions/missing-caret-rhs.mel"
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
        "../../../tests/corpus/parser/expressions/missing-brace-list-close.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected '}' to close brace list");
}

#[test]
fn reports_missing_cast_operand_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-cast-operand.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected expression after cast");
}

#[test]
fn reports_malformed_path_like_bareword_missing_segment_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/malformed-path-like-bareword-missing-segment.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected expression inside index");
}

#[test]
fn reports_missing_vector_close_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-vector-close.mel"
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
        "../../../tests/corpus/parser/expressions/missing-member-name.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected member name after '.'");
}

#[test]
fn reports_trailing_dot_float_double_dot_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/trailing-dot-float-double-dot.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected member name after '.'");
}

#[test]
fn reports_malformed_command_word_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-word.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-missing-closing-backquote.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-signed-number.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-spaced-flag.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-single-dot.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-leading-dot-no-digit.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-spaced-dotted-bareword.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-spaced-pipe-bareword.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-empty-pipe-bareword.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-spaced-namespace-bareword.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-leading-colon-bareword.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-dotted-variable-indexed-bareword.mel"
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
        "../../../tests/corpus/parser/statements/malformed-command-brace-list.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected '}' to close brace list");
}

#[test]
fn reports_malformed_command_vector_literal_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-vector-literal.mel"
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
        "../../../tests/corpus/parser/statements/missing-statement-parens.mel"
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
        "../../../tests/corpus/parser/statements/missing-statement-semi-recovery.mel"
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
        include_str!("../../../tests/corpus/parser/statements/trailing-statement-no-semi.mel"),
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
