use super::*;

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
fn shared_light_parse_reuses_arc_text_and_matches_owned_parse() {
    let source: Arc<str> = Arc::from("global proc foo() { }\nsetAttr \".tx\" 1;\n");
    let parse = parse_light_shared_source(Arc::clone(&source));
    let owned = parse_light_source(source.as_ref());

    assert!(Arc::ptr_eq(&parse.source_text, &source));
    assert_eq!(parse.source, owned.source);
    assert_eq!(parse.source_map, owned.source_map);
    assert_eq!(parse.errors, owned.errors);
    let LightItem::Command(command) = &parse.source.items[1] else {
        panic!("expected command item");
    };
    assert_eq!(parse.source_slice(command.head_range), "setAttr");
}

#[test]
fn shared_light_parse_bytes_matches_owned_utf8_bytes_path() {
    let input = b"global proc foo() { }\nsetAttr \".tx\" 1;\n";
    let parse = parse_light_shared_bytes(input);
    let owned = parse_light_bytes(input);

    assert_eq!(parse.source, owned.source);
    assert_eq!(parse.source_map, owned.source_map);
    assert_eq!(parse.source_encoding, owned.source_encoding);
    assert_eq!(parse.decode_errors, owned.decode_errors);
    assert_eq!(parse.errors, owned.errors);
    assert_eq!(parse.source_text.as_ref(), owned.source_text);
}

#[test]
fn shared_light_parse_bytes_with_encoding_matches_owned_cp932_path() {
    let (bytes, _, _) = SHIFT_JIS.encode("setAttr \".名\" -type \"string\" \"値\";\n");
    let parse = parse_light_shared_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Cp932);
    let owned = parse_light_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Cp932);

    assert_eq!(parse.source, owned.source);
    assert_eq!(parse.source_map, owned.source_map);
    assert_eq!(parse.source_encoding, owned.source_encoding);
    assert_eq!(parse.decode_errors, owned.decode_errors);
    assert_eq!(parse.errors, owned.errors);

    let LightItem::Command(command) = &parse.source.items[0] else {
        panic!("expected command item");
    };
    assert_eq!(parse.source_slice(command.words[0].range()), "\".名\"");
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
fn streaming_shared_light_scan_matches_materialized_items() {
    let source: Arc<str> = Arc::from("global proc foo() { }\nsetAttr \".tx\" 1;\n");
    let materialized = parse_light_shared_source(Arc::clone(&source));
    let mut streamed = Vec::new();
    let report = scan_light_shared_source_with_options_and_sink(
        Arc::clone(&source),
        LightParseOptions::default(),
        &mut |_: mel_syntax::SourceView<'_>, item: LightItem| streamed.push(item),
    );

    assert!(Arc::ptr_eq(&report.source_text, &source));
    assert_eq!(streamed, materialized.source.items);
    assert_eq!(report.errors, materialized.errors);
}

#[test]
fn streaming_shared_light_scan_bytes_matches_materialized_items() {
    let (bytes, _, _) = SHIFT_JIS.encode("setAttr \".名\" -type \"string\" \"値\";\n");
    let materialized =
        parse_light_shared_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Cp932);
    let mut streamed = Vec::new();
    let report = scan_light_shared_bytes_with_encoding_and_options_and_sink(
        bytes.as_ref(),
        SourceEncoding::Cp932,
        LightParseOptions::default(),
        &mut |_: mel_syntax::SourceView<'_>, item: LightItem| streamed.push(item),
    );

    assert_eq!(streamed, materialized.source.items);
    assert_eq!(report.errors, materialized.errors);
    let LightItem::Command(command) = &streamed[0] else {
        panic!("expected command item");
    };
    assert_eq!(report.source_slice(command.words[0].range()), "\".名\"");
}

#[test]
fn streaming_shared_light_scan_utf8_bytes_matches_materialized_items() {
    let source = b"global proc foo() { }\nsetAttr \".tx\" 1;\n";
    let materialized = parse_light_shared_bytes(source);
    let mut streamed = Vec::new();
    let report = scan_light_shared_bytes_with_options_and_sink(
        source,
        LightParseOptions::default(),
        &mut |_: mel_syntax::SourceView<'_>, item: LightItem| streamed.push(item),
    );

    assert_eq!(streamed, materialized.source.items);
    assert_eq!(report.errors, materialized.errors);
}

#[test]
fn parse_light_shared_file_matches_owned_utf8_file_path() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-light-shared-utf8-{unique}.mel"));
    fs::write(&path, "global proc foo() { }\nsetAttr \".tx\" 1;\n")
        .expect("temp fixture should be writable");

    let parse = parse_light_shared_file(&path).expect("shared light parse should succeed");
    let owned = parse_light_file(&path).expect("owned light parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert_eq!(parse.source, owned.source);
    assert_eq!(parse.source_map, owned.source_map);
    assert_eq!(parse.source_encoding, owned.source_encoding);
    assert_eq!(parse.decode_errors, owned.decode_errors);
    assert_eq!(parse.errors, owned.errors);
    assert_eq!(parse.source_text.as_ref(), owned.source_text);
}

#[test]
fn parse_light_file_with_options_respects_max_bytes_budget() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-light-budget-file-{unique}.mel"));
    fs::write(&path, "setAttr \".tx\" 1;\n").expect("temp fixture should be writable");

    let parse = parse_light_file_with_options(
        &path,
        LightParseOptions {
            budgets: ParseBudgets {
                max_bytes: 4,
                ..ParseBudgets::default()
            },
            ..LightParseOptions::default()
        },
    )
    .expect("budgeted light parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert!(parse.source.items.is_empty());
    assert_eq!(parse.errors.len(), 1);
    assert_eq!(
        parse.errors[0].message,
        "source exceeds parse budget: max_bytes"
    );
}

#[test]
fn scan_light_shared_file_with_encoding_matches_owned_cp932_file_path() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mel-parser-light-shared-cp932-{unique}.mel"));
    let (bytes, _, _) = SHIFT_JIS.encode("setAttr \".名\" -type \"string\" \"値\";\n");
    fs::write(&path, bytes.as_ref()).expect("temp fixture should be writable");

    let mut shared_items = Vec::new();
    let shared = scan_light_shared_file_with_encoding_and_options_and_sink(
        &path,
        SourceEncoding::Cp932,
        LightParseOptions::default(),
        &mut |_: mel_syntax::SourceView<'_>, item: LightItem| shared_items.push(item),
    )
    .expect("shared light scan should succeed");
    let mut owned_items = Vec::new();
    let owned = scan_light_file_with_encoding_and_options_and_sink(
        &path,
        SourceEncoding::Cp932,
        LightParseOptions::default(),
        &mut |_: mel_syntax::SourceView<'_>, item: LightItem| owned_items.push(item),
    )
    .expect("owned light scan should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert_eq!(shared_items, owned_items);
    assert_eq!(shared.source_map, owned.source_map);
    assert_eq!(shared.source_encoding, owned.source_encoding);
    assert_eq!(shared.decode_errors, owned.decode_errors);
    assert_eq!(shared.errors, owned.errors);
    assert_eq!(shared.source_text.as_ref(), owned.source_text);
}

#[test]
fn parse_light_file_with_encoding_and_options_respects_max_bytes_budget() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "mel-parser-light-budget-file-encoding-{unique}.mel"
    ));
    fs::write(&path, b"setAttr \".tx\" 1;\n").expect("temp fixture should be writable");

    let parse = parse_light_file_with_encoding_and_options(
        &path,
        SourceEncoding::Utf8,
        LightParseOptions {
            budgets: ParseBudgets {
                max_bytes: 4,
                ..ParseBudgets::default()
            },
            ..LightParseOptions::default()
        },
    )
    .expect("budgeted encoded light parse should succeed");

    fs::remove_file(&path).expect("temp fixture should be removable");

    assert!(parse.source.items.is_empty());
    assert_eq!(parse.errors.len(), 1);
    assert_eq!(
        parse.errors[0].message,
        "source exceeds parse budget: max_bytes"
    );
}

#[test]
fn light_parse_tracks_multiline_command_tail_as_single_statement() {
    let source = "setAttr \".fc[0]\" -type \"polyFaces\"\n    f 4 0 1 2 3\n    mu 0 4 0 1 2 3;\n";
    let parse = parse_light_source_with_options(
        source,
        LightParseOptions {
            max_prefix_words: 4,
            max_prefix_bytes: 48,
            ..LightParseOptions::default()
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
            ..LightParseOptions::default()
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
fn light_parse_reports_max_bytes_budget_before_scan_starts() {
    let parse = parse_light_source_with_options(
        "setAttr \".tx\" 1;\n",
        LightParseOptions {
            budgets: ParseBudgets {
                max_bytes: 4,
                ..ParseBudgets::default()
            },
            ..LightParseOptions::default()
        },
    );

    assert!(parse.source.items.is_empty());
    assert_eq!(parse.errors.len(), 1);
    assert_eq!(
        parse.errors[0].message,
        "source exceeds parse budget: max_bytes"
    );
}

#[test]
fn light_parse_reports_max_statements_budget_and_drops_tail_items() {
    let parse = parse_light_source_with_options(
        "setAttr \".tx\" 1;\nsetAttr \".ty\" 2;\n",
        LightParseOptions {
            budgets: ParseBudgets {
                max_statements: 1,
                ..ParseBudgets::default()
            },
            ..LightParseOptions::default()
        },
    );

    assert_eq!(parse.source.items.len(), 1);
    assert_eq!(parse.errors.len(), 1);
    assert_eq!(
        parse.errors[0].message,
        "source exceeds parse budget: max_statements"
    );
}

#[test]
fn light_parse_reports_max_literal_bytes_budget_for_string_word() {
    let parse = parse_light_source_with_options(
        "setAttr \".tx\" \"abcdef\";\n",
        LightParseOptions {
            budgets: ParseBudgets {
                max_literal_bytes: 4,
                ..ParseBudgets::default()
            },
            ..LightParseOptions::default()
        },
    );

    assert_eq!(parse.errors.len(), 1);
    assert_eq!(
        parse.errors[0].message,
        "source exceeds parse budget: max_literal_bytes"
    );
}

#[test]
fn light_parse_reports_max_nesting_depth_budget_for_grouped_expr() {
    let parse = parse_light_source_with_options(
        "file -command ((((\"x\"))));\n",
        LightParseOptions {
            budgets: ParseBudgets {
                max_nesting_depth: 2,
                ..ParseBudgets::default()
            },
            ..LightParseOptions::default()
        },
    );

    assert_eq!(parse.errors.len(), 1);
    assert_eq!(
        parse.errors[0].message,
        "source exceeds parse budget: max_nesting_depth"
    );
}

#[test]
fn light_parse_reports_max_tokens_budget_for_long_command() {
    let parse = parse_light_source_with_options(
        "setAttr \".pt\" 1 2 3 4 5 6;\n",
        LightParseOptions {
            budgets: ParseBudgets {
                max_tokens: 6,
                ..ParseBudgets::default()
            },
            ..LightParseOptions::default()
        },
    );

    assert_eq!(parse.errors.len(), 1);
    assert_eq!(
        parse.errors[0].message,
        "source exceeds parse budget: max_tokens"
    );
}

#[test]
fn light_parse_does_not_double_count_command_head_tokens_against_budget() {
    let parse = parse_light_source_with_options(
        "createNode -n foo;\n",
        LightParseOptions {
            budgets: ParseBudgets {
                max_tokens: 4,
                ..ParseBudgets::default()
            },
            ..LightParseOptions::default()
        },
    );

    assert_eq!(parse.source.items.len(), 1);
    assert!(parse.errors.is_empty());
}

#[test]
fn light_parse_does_not_double_count_global_statement_proc_probe_tokens() {
    let parse = parse_light_source_with_options(
        "global int $x;\n",
        LightParseOptions {
            budgets: ParseBudgets {
                max_tokens: 4,
                ..ParseBudgets::default()
            },
            ..LightParseOptions::default()
        },
    );

    assert_eq!(parse.source.items.len(), 1);
    assert!(parse.errors.is_empty());
}
