use super::*;

#[test]
fn parse_shared_source_reuses_arc_text_and_matches_owned_parse() {
    let source: Arc<str> = Arc::from("setAttr \".tx\" 1;\n");
    let parse = parse_shared_source(Arc::clone(&source));
    let owned = parse_source(source.as_ref());

    assert!(Arc::ptr_eq(&parse.source_text, &source));
    assert_eq!(parse.syntax, owned.syntax);
    assert_eq!(parse.source_map, owned.source_map);
    assert_eq!(parse.lex_errors, owned.lex_errors);
    assert_eq!(parse.errors, owned.errors);
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
fn parse_shared_bytes_matches_owned_utf8_bytes_path() {
    let input = b"setAttr \".tx\" 1;\n";
    let parse = parse_shared_bytes(input);
    let owned = parse_bytes(input);

    assert_eq!(parse.syntax, owned.syntax);
    assert_eq!(parse.source_map, owned.source_map);
    assert_eq!(parse.source_encoding, owned.source_encoding);
    assert_eq!(parse.decode_errors, owned.decode_errors);
    assert_eq!(parse.lex_errors, owned.lex_errors);
    assert_eq!(parse.errors, owned.errors);
    assert_eq!(parse.source_text.as_ref(), owned.source_text);
}

#[test]
fn parse_shared_bytes_with_encoding_matches_owned_cp932_path() {
    let (bytes, _, _) = SHIFT_JIS.encode("setAttr \".名\" -type \"string\" \"値\";\n");
    let parse = parse_shared_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Cp932);
    let owned = parse_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Cp932);

    assert_eq!(parse.syntax, owned.syntax);
    assert_eq!(parse.source_map, owned.source_map);
    assert_eq!(parse.source_encoding, owned.source_encoding);
    assert_eq!(parse.decode_errors, owned.decode_errors);
    assert_eq!(parse.lex_errors, owned.lex_errors);
    assert_eq!(parse.errors, owned.errors);

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
    let InvokeSurface::ShellLike { words, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    let ShellWord::QuotedString { text, .. } = &words[0] else {
        panic!("expected quoted attr path");
    };
    assert_eq!(parse.source_slice(*text), "\".名\"");
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
fn parse_file_with_options_respects_max_bytes_budget() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-budget-file-{unique}.mel"));
    fs::write(&path, "print \"hello\";\n").expect("temp fixture should be writable");

    let parse = parse_file_with_options(
        &path,
        ParseOptions {
            budgets: ParseBudgets {
                max_bytes: 4,
                ..ParseBudgets::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("budgeted file parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert!(parse.syntax.items.is_empty());
    assert_eq!(parse.errors.len(), 1);
    assert_eq!(
        parse.errors[0].message,
        "source exceeds parse budget: max_bytes"
    );
}

#[test]
fn parse_shared_file_matches_owned_utf8_file_path() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-shared-utf8-{unique}.mel"));
    fs::write(&path, "print \"hello\";\n").expect("temp fixture should be writable");

    let parse = parse_shared_file(&path).expect("shared utf8 parse should succeed");
    let owned = parse_file(&path).expect("owned utf8 parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert_eq!(parse.syntax, owned.syntax);
    assert_eq!(parse.source_map, owned.source_map);
    assert_eq!(parse.source_encoding, owned.source_encoding);
    assert_eq!(parse.decode_errors, owned.decode_errors);
    assert_eq!(parse.lex_errors, owned.lex_errors);
    assert_eq!(parse.errors, owned.errors);
    assert_eq!(parse.source_text.as_ref(), owned.source_text);
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
fn parse_file_with_encoding_and_options_respects_max_bytes_budget() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-budget-file-encoding-{unique}.mel"));
    fs::write(&path, b"setAttr \".tx\" 1;\n").expect("temp fixture should be writable");

    let parse = parse_file_with_encoding_and_options(
        &path,
        SourceEncoding::Utf8,
        ParseOptions {
            budgets: ParseBudgets {
                max_bytes: 4,
                ..ParseBudgets::default()
            },
            ..ParseOptions::default()
        },
    )
    .expect("budgeted encoded file parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert!(parse.syntax.items.is_empty());
    assert_eq!(parse.errors.len(), 1);
    assert_eq!(
        parse.errors[0].message,
        "source exceeds parse budget: max_bytes"
    );
}

#[test]
fn parse_shared_file_with_encoding_matches_owned_cp932_file_path() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-shared-cp932-{unique}.mel"));
    let (bytes, _, had_errors) = SHIFT_JIS.encode("print \"設定\";\n");
    assert!(!had_errors);
    fs::write(&path, bytes.as_ref()).expect("temp fixture should be writable");

    let parse = parse_shared_file_with_encoding(&path, SourceEncoding::Cp932)
        .expect("shared cp932 parse should succeed");
    let owned = parse_file_with_encoding(&path, SourceEncoding::Cp932)
        .expect("owned cp932 parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert_eq!(parse.syntax, owned.syntax);
    assert_eq!(parse.source_map, owned.source_map);
    assert_eq!(parse.source_encoding, owned.source_encoding);
    assert_eq!(parse.decode_errors, owned.decode_errors);
    assert_eq!(parse.lex_errors, owned.lex_errors);
    assert_eq!(parse.errors, owned.errors);
    assert_eq!(parse.source_text.as_ref(), owned.source_text);
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
        "../../../../../tests/corpus/parser/statements/nested-proc-definition.mel"
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
        "../../../../../tests/corpus/parser/statements/malformed-nested-proc-missing-body.mel"
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
