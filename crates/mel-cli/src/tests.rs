use crate::{
    app::{cli_parse_budgets, print_path_output},
    args::{Args, CliDiagnosticLevel, parse_cli_args},
    diagnostics::{
        compute_display_line_starts, compute_normalized_line_starts,
        normalize_diagnostic_source_text, normalized_line_col_for_offset,
    },
    report::{
        CorpusSummary, FileSummary, LightCorpusSummary, LightFileSummary,
        format_light_corpus_summary, format_light_single_file_output, format_single_file_output,
        format_single_file_output_with_style, write_single_file_output,
    },
};
use clap::{CommandFactory, error::ErrorKind};
use maya_mel::parser::{
    LightParseOptions, parse_light_bytes_with_encoding, parse_light_source_with_options,
};
use maya_mel::{
    ParseBudgets, ParseMode, ParseOptions, SourceEncoding, parse_bytes_with_encoding, parse_source,
    parse_source_with_options,
};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn render_snapshot(label: &str, source: &str) -> String {
    format_single_file_output(label, &parse_source(source), CliDiagnosticLevel::All)
        .expect("snapshot should render")
}

#[test]
fn normalize_diagnostic_source_text_collapses_crlf_offsets() {
    let (display, map) = normalize_diagnostic_source_text("a\t\r\nb\r\n");
    assert_eq!(display, "a \nb\n");
    assert_eq!(map.display_offset(0), 0);
    assert_eq!(map.display_offset(1), 1);
    assert_eq!(map.display_offset(2), 2);
    assert_eq!(map.display_offset(3), 2);
    assert_eq!(map.display_offset(4), 3);
    assert_eq!(map.display_offset(5), 4);
    assert_eq!(map.display_offset(6), 4);
    assert_eq!(map.display_offset(7), 5);
}

#[test]
fn compute_display_line_starts_tracks_normalized_lines() {
    let starts = compute_display_line_starts("first\nsecond\nthird");
    assert_eq!(starts, vec![0, 6, 13]);
}

#[test]
fn normalized_line_col_matches_compact_display_rules() {
    let source = "a\t\r\nbc\r\ndef\n";
    let starts = compute_normalized_line_starts(source);
    assert_eq!(starts, vec![0, 4, 8, 12]);
    assert_eq!(normalized_line_col_for_offset(source, &starts, 0), (0, 0));
    assert_eq!(normalized_line_col_for_offset(source, &starts, 1), (0, 1));
    assert_eq!(normalized_line_col_for_offset(source, &starts, 2), (0, 2));
    assert_eq!(normalized_line_col_for_offset(source, &starts, 3), (0, 2));
    assert_eq!(normalized_line_col_for_offset(source, &starts, 4), (1, 0));
    assert_eq!(normalized_line_col_for_offset(source, &starts, 6), (1, 2));
    assert_eq!(normalized_line_col_for_offset(source, &starts, 7), (1, 2));
    assert_eq!(normalized_line_col_for_offset(source, &starts, 8), (2, 0));
}

#[test]
fn format_single_file_output_handles_gbk_source_without_panicking() {
    let parse = parse_bytes_with_encoding(b"print \"\xB0\xB4\xC5\xA5\";", SourceEncoding::Gbk);
    let output = format_single_file_output("gbk-fixture", &parse, CliDiagnosticLevel::All)
        .expect("gbk output should render");
    assert!(output.contains("encoding: gbk"));
}

#[test]
fn format_light_single_file_output_handles_gbk_source_without_panicking() {
    let parse = parse_light_bytes_with_encoding(
        b"setAttr \".\xB0\xB4\" -type \"string\" \"\xC5\xA5\";",
        SourceEncoding::Gbk,
    );
    let output = format_light_single_file_output("gbk-fixture", &parse, CliDiagnosticLevel::All)
        .expect("light gbk output should render");
    assert!(output.contains("mode: lightweight"));
    assert!(output.contains("encoding: gbk"));
}

#[test]
fn inline_mode_accepts_single_trailing_statement_without_semicolon() {
    let parse = parse_source_with_options(
        r#"print "hello""#,
        ParseOptions {
            mode: ParseMode::AllowTrailingStmtWithoutSemi,
            ..ParseOptions::default()
        },
    );
    assert!(parse.errors.is_empty());
}

#[test]
fn cli_accepts_positional_path() {
    let args = parse_cli_args(["mel-inspect", "private-corpus"]).expect("path should parse");
    assert_eq!(args.path, Some(PathBuf::from("private-corpus")));
}

#[test]
fn cli_accepts_lightweight_flag() {
    let args = parse_cli_args(["mel-inspect", "--lightweight", "private-corpus"]).expect("light");
    assert!(args.lightweight);
}

#[test]
fn cli_accepts_inline_flag() {
    let args = parse_cli_args(["mel-inspect", "--inline", r#"print "hello""#])
        .expect("inline should parse");
    assert_eq!(args.inline_input.as_deref(), Some(r#"print "hello""#));
}

#[test]
fn cli_accepts_diagnostic_level_flag() {
    let args = parse_cli_args(["mel-inspect", "--diagnostic-level", "error", "fixture.mel"])
        .expect("diagnostic level should parse");
    assert_eq!(args.diagnostic_level, CliDiagnosticLevel::Error);
}

#[test]
fn cli_accepts_max_bytes_flag() {
    let args =
        parse_cli_args(["mel-inspect", "--max-bytes", "1024", "fixture.mel"]).expect("max bytes");
    assert_eq!(args.max_bytes, Some(1024));
}

#[test]
fn cli_rejects_zero_max_bytes() {
    let error = parse_cli_args(["mel-inspect", "--max-bytes", "0", "fixture.mel"])
        .expect_err("zero max bytes should fail");
    assert_eq!(error.kind(), ErrorKind::ValueValidation);
}

#[test]
fn cli_rejects_removed_file_flag() {
    let error = parse_cli_args(["mel-inspect", "--file", "a.mel"])
        .expect_err("removed file flag should fail");
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn cli_rejects_removed_directory_flag() {
    let error = parse_cli_args(["mel-inspect", "--directory", "private-corpus"])
        .expect_err("removed directory flag should fail");
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn cli_rejects_removed_path_flag() {
    let error = parse_cli_args(["mel-inspect", "--path", "private-corpus"])
        .expect_err("removed path flag should fail");
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn cli_rejects_conflicting_input_modes() {
    let error = parse_cli_args([
        "mel-inspect",
        "private-corpus",
        "--inline",
        r#"print "hello""#,
    ])
    .expect_err("conflicting modes should fail");
    assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
}

#[test]
fn cli_rejects_lightweight_with_inline() {
    let error = parse_cli_args(["mel-inspect", "--lightweight", "--inline", "print 1"])
        .expect_err("lightweight inline should fail");
    assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
}

#[test]
fn cli_rejects_invalid_encoding() {
    let error = parse_cli_args([
        "mel-inspect",
        "--encoding",
        "latin1",
        "--inline",
        "`ls -sl`;",
    ])
    .expect_err("invalid encoding should fail");
    assert_eq!(error.kind(), ErrorKind::InvalidValue);
}

#[test]
fn help_mentions_directory_flag_and_encoding_values() {
    let mut help = Vec::new();
    let mut command = Args::command();
    command
        .write_long_help(&mut help)
        .expect("help should render");
    let help = String::from_utf8(help).expect("help should be utf8");
    assert!(help.contains("[PATH]"));
    assert!(help.contains("--lightweight"));
    assert!(help.contains("--inline <SOURCE>"));
    assert!(help.contains("--max-bytes <MAX_BYTES>"));
    assert!(help.contains("--diagnostic-level <DIAGNOSTIC_LEVEL>"));
    assert!(help.contains("other parser budgets scale proportionally from defaults"));
    assert!(help.contains("[possible values: auto, utf8, cp932, gbk]"));
}

#[test]
fn error_diagnostic_level_hides_warnings_and_zeroes_summary_count() {
    let output = format_single_file_output(
        "warning-fixture",
        &parse_source("global proc foo() { string $name; if ($name == \"\") { } }\nfoo();\n"),
        CliDiagnosticLevel::Error,
    )
    .expect("filtered output");
    assert!(output.contains("semantic diagnostics: 0"));
    assert!(!output.contains("Warning:"));
}

#[test]
fn none_diagnostic_level_hides_all_diagnostic_output() {
    let output = format_single_file_output(
        "parse-fixture",
        &parse_source("print(\n"),
        CliDiagnosticLevel::None,
    )
    .expect("filtered output");
    assert!(output.contains("decode diagnostics: 0"));
    assert!(output.contains("lexical diagnostics: 0"));
    assert!(output.contains("parse errors: 0"));
    assert!(output.contains("semantic diagnostics: 0"));
    assert!(!output.contains("Error:"));
    assert!(!output.contains("Warning:"));
}

#[test]
fn error_diagnostic_level_keeps_semantic_error_count() {
    let output = format_single_file_output(
        "sema-fixture",
        &parse_source(include_str!(
            "../../../tests/corpus/sema/proc/typed-missing-value-return.mel"
        )),
        CliDiagnosticLevel::Error,
    )
    .expect("filtered output");
    assert!(output.contains("semantic diagnostics: 1"));
    assert!(output.contains("Error:"));
    assert!(output.contains("declares a return type but never returns a value"));
}

#[test]
fn compact_output_uses_single_line_diagnostics_for_non_terminal_output() {
    let output = format_single_file_output_with_style(
        "sema-fixture",
        &parse_source(include_str!(
            "../../../tests/corpus/sema/proc/typed-missing-value-return.mel"
        )),
        CliDiagnosticLevel::Error,
        false,
    )
    .expect("compact output");
    assert!(output.contains("semantic diagnostics: 1"));
    assert!(output.contains("Error: sema: proc \"helper\" declares a return type"));
    assert!(output.contains("@ 1:1"));
    assert!(!output.contains("╭"));
}

#[test]
fn compact_output_keeps_parse_error_locations() {
    let output = format_single_file_output_with_style(
        "parse-fixture",
        &parse_source("print(\n"),
        CliDiagnosticLevel::Error,
        false,
    )
    .expect("compact output");
    assert!(output.contains("parse errors: 3"));
    assert!(output.contains("Error: parse: expected expression as function argument"));
    assert!(output.contains("@ 2:1"));
}

#[test]
fn write_single_file_output_matches_compact_formatter() {
    let parse = parse_source("addAttr;\n");
    let expected = format_single_file_output_with_style(
        "sema-fixture",
        &parse,
        CliDiagnosticLevel::Error,
        false,
    )
    .expect("compact output");
    let mut actual = Vec::new();
    write_single_file_output(
        &mut actual,
        "sema-fixture",
        &parse,
        CliDiagnosticLevel::Error,
    )
    .expect("write output");
    let actual = String::from_utf8(actual).expect("writer output should stay utf8");
    assert_eq!(actual, expected);
}

#[test]
fn path_mode_rejects_non_file_non_directory() {
    let path = unique_test_path("socket");
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixListener;

        let _listener = UnixListener::bind(&path).expect("socket should bind");
        let error = print_path_output(
            &path,
            None,
            false,
            CliDiagnosticLevel::All,
            ParseBudgets::default(),
        )
        .expect_err("socket path should fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(not(unix))]
    {
        fs::create_dir_all(path.parent().expect("temp dir should exist"))
            .expect("temp dir should exist");
        fs::write(&path, []).expect("temp file should exist");
        fs::remove_file(&path).expect("temp file should be removable");
    }

    cleanup_test_path(&path);
}

#[test]
fn cli_parse_budgets_returns_defaults_without_override() {
    assert_eq!(cli_parse_budgets(None), ParseBudgets::default());
}

#[test]
fn cli_parse_budgets_scales_other_limits_from_max_bytes() {
    let default = ParseBudgets::default();
    let budgets = cli_parse_budgets(Some(default.max_bytes / 2));

    assert_eq!(budgets.max_bytes, default.max_bytes / 2);
    assert_eq!(budgets.max_tokens, default.max_tokens / 2);
    assert_eq!(budgets.max_statements, default.max_statements / 2);
    assert_eq!(budgets.max_nesting_depth, default.max_nesting_depth / 2);
    assert_eq!(budgets.max_literal_bytes, default.max_literal_bytes / 2);
}

#[test]
fn cli_parse_budgets_clamps_tiny_values_to_non_zero_limits() {
    let budgets = cli_parse_budgets(Some(1));

    assert_eq!(budgets.max_bytes, 1);
    assert_eq!(budgets.max_tokens, 1);
    assert_eq!(budgets.max_statements, 1);
    assert_eq!(budgets.max_nesting_depth, 1);
    assert_eq!(budgets.max_literal_bytes, 1);
}

#[test]
fn path_mode_reports_budget_parse_errors_for_files() {
    let path = unique_test_path("budget-file");
    fs::write(&path, "print \"hello\";\n").expect("temp file");

    let output = {
        let parse = print_path_output(
            &path,
            None,
            false,
            CliDiagnosticLevel::All,
            cli_parse_budgets(Some(4)),
        );
        parse.expect("file output should render");
        format_single_file_output(
            &path.display().to_string(),
            &maya_mel::parse_file_with_options(
                &path,
                ParseOptions {
                    budgets: cli_parse_budgets(Some(4)),
                    ..ParseOptions::default()
                },
            )
            .expect("parse"),
            CliDiagnosticLevel::All,
        )
        .expect("formatted output")
    };

    cleanup_test_path(&path);

    assert!(output.contains("source exceeds parse budget: max_bytes"));
}

#[test]
fn corpus_summary_counts_budget_failures_as_parse_errors() {
    let root = unique_test_path("budget-corpus");
    fs::create_dir_all(&root).expect("temp dir");
    fs::write(root.join("a.mel"), "print 1;\n").expect("mel file");

    let parse = maya_mel::parse_file_with_options(
        root.join("a.mel"),
        ParseOptions {
            budgets: cli_parse_budgets(Some(4)),
            ..ParseOptions::default()
        },
    )
    .expect("budgeted parse");

    let file_summary =
        crate::report::summarize_parse_file(&root.join("a.mel"), &parse, CliDiagnosticLevel::All);

    cleanup_test_path(&root);

    assert_eq!(file_summary.parse_errors, 1);
    assert!(
        file_summary
            .parse_error_messages
            .contains(&"source exceeds parse budget: max_bytes".to_owned())
    );
}

#[test]
fn top_parse_error_files_are_sorted_by_count_then_path() {
    let mut summary = CorpusSummary::default();
    summary.record(FileSummary {
        path: "b.mel".to_owned(),
        decode_errors: 0,
        lex_errors: 0,
        parse_errors: 3,
        parse_error_messages: vec!["missing ;".to_owned()],
        semantic_diagnostics: 0,
    });
    summary.record(FileSummary {
        path: "a.mel".to_owned(),
        decode_errors: 0,
        lex_errors: 0,
        parse_errors: 3,
        parse_error_messages: vec!["missing ;".to_owned()],
        semantic_diagnostics: 0,
    });
    summary.record(FileSummary {
        path: "c.mel".to_owned(),
        decode_errors: 0,
        lex_errors: 0,
        parse_errors: 1,
        parse_error_messages: vec!["missing )".to_owned()],
        semantic_diagnostics: 0,
    });

    let ranked = summary.top_parse_error_files();
    assert_eq!(
        ranked,
        vec![
            ("a.mel".to_owned(), 3),
            ("b.mel".to_owned(), 3),
            ("c.mel".to_owned(), 1),
        ]
    );
}

#[test]
fn top_parse_error_messages_are_aggregated_and_sorted() {
    let mut summary = CorpusSummary::default();
    summary.record(FileSummary {
        path: "a.mel".to_owned(),
        decode_errors: 0,
        lex_errors: 0,
        parse_errors: 2,
        parse_error_messages: vec!["missing ;".to_owned(), "missing )".to_owned()],
        semantic_diagnostics: 0,
    });
    summary.record(FileSummary {
        path: "b.mel".to_owned(),
        decode_errors: 0,
        lex_errors: 0,
        parse_errors: 2,
        parse_error_messages: vec!["missing ;".to_owned(), "missing ]".to_owned()],
        semantic_diagnostics: 0,
    });

    let ranked = summary.top_parse_error_messages();
    assert_eq!(
        ranked,
        vec![
            ("missing ;".to_owned(), 2),
            ("missing )".to_owned(), 1),
            ("missing ]".to_owned(), 1),
        ]
    );
}

#[test]
fn light_output_reports_opaque_tail_counts() {
    let parse = parse_light_source_with_options(
        "setAttr \".pt\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n",
        LightParseOptions {
            max_prefix_words: 5,
            max_prefix_bytes: 32,
            ..LightParseOptions::default()
        },
    );
    let output = format_light_single_file_output("light-fixture", &parse, CliDiagnosticLevel::All)
        .expect("light output");
    assert!(output.contains("opaque-tail commands: 1"));
    assert!(output.contains("setAttr with opaque tail: 1"));
}

#[test]
fn light_none_diagnostic_level_zeroes_rendered_counts() {
    let parse =
        parse_light_source_with_options("setAttr \".tx\" -type;\n", LightParseOptions::default());
    let output = format_light_single_file_output("light-fixture", &parse, CliDiagnosticLevel::None)
        .expect("light output");
    assert!(output.contains("decode diagnostics: 0"));
    assert!(output.contains("light parse errors: 0"));
    assert!(!output.contains("Error:"));
}

#[test]
fn collect_source_files_in_lightweight_mode_includes_ma_files() {
    let root = unique_test_path("light-corpus");
    fs::create_dir_all(&root).expect("temp dir");
    fs::write(root.join("a.mel"), "print 1;\n").expect("mel file");
    fs::write(root.join("b.ma"), "setAttr \".tx\" 1;\n").expect("ma file");
    fs::write(root.join("c.txt"), "ignore\n").expect("txt file");

    let mel_only = crate::report::collect_source_files(&root, false).expect("mel files");
    let light_files = crate::report::collect_source_files(&root, true).expect("light files");
    assert_eq!(mel_only.len(), 1);
    assert_eq!(light_files.len(), 2);

    cleanup_test_path(&root);
}

#[test]
fn format_light_corpus_summary_reports_lightweight_counts() {
    let mut summary = LightCorpusSummary::default();
    summary.record(LightFileSummary {
        path: "a.ma".to_owned(),
        decode_errors: 1,
        light_parse_errors: 0,
        items: 10,
        command_items: 8,
        proc_items: 1,
        other_items: 1,
        opaque_tail_commands: 2,
        specialized_set_attr: 3,
        set_attr_with_opaque_tail: 2,
    });
    let output = format_light_corpus_summary(&summary);
    assert!(output.contains("files with light parse errors: 0"));
    assert!(output.contains("total opaque-tail commands: 2"));
    assert!(output.contains("total light-specialized setAttr: 3"));
}

#[test]
fn snapshot_lexer_unterminated_string_fixture() {
    insta::assert_snapshot!(
        "lexer_unterminated_string",
        render_snapshot(
            "lexer/strings/unterminated-string.mel",
            include_str!("../../../tests/corpus/lexer/strings/unterminated-string.mel"),
        )
    );
}

#[test]
fn snapshot_lexer_unknown_char_fixture() {
    insta::assert_snapshot!(
        "lexer_unknown_char",
        render_snapshot(
            "lexer/symbols/unknown-char.mel",
            include_str!("../../../tests/corpus/lexer/symbols/unknown-char.mel"),
        )
    );
}

#[test]
fn snapshot_parser_missing_ternary_colon_fixture() {
    insta::assert_snapshot!(
        "parser_missing_ternary_colon",
        render_snapshot(
            "parser/expressions/missing-ternary-colon.mel",
            include_str!("../../../tests/corpus/parser/expressions/missing-ternary-colon.mel"),
        )
    );
}

#[test]
fn snapshot_parser_missing_proc_param_name_fixture() {
    insta::assert_snapshot!(
        "parser_missing_proc_param_name",
        render_snapshot(
            "parser/proc/missing-proc-param-name.mel",
            include_str!("../../../tests/corpus/parser/proc/missing-proc-param-name.mel"),
        )
    );
}

#[test]
fn snapshot_sema_local_proc_forward_reference_fixture() {
    insta::assert_snapshot!(
        "sema_local_proc_forward_reference",
        render_snapshot(
            "sema/proc/local-forward-reference.mel",
            include_str!("../../../tests/corpus/sema/proc/local-forward-reference.mel"),
        )
    );
}

#[test]
fn snapshot_sema_local_proc_shell_unresolved_fixture() {
    insta::assert_snapshot!(
        "sema_local_proc_shell_unresolved",
        render_snapshot(
            "sema/proc/local-shell-unresolved.mel",
            include_str!("../../../tests/corpus/sema/proc/local-shell-unresolved.mel"),
        )
    );
}

#[test]
fn snapshot_sema_local_proc_shell_forward_reference_fixture() {
    insta::assert_snapshot!(
        "sema_local_proc_shell_forward_reference",
        render_snapshot(
            "sema/proc/local-shell-forward-reference.mel",
            include_str!("../../../tests/corpus/sema/proc/local-shell-forward-reference.mel"),
        )
    );
}

#[test]
fn snapshot_sema_typed_missing_value_return_fixture() {
    insta::assert_snapshot!(
        "sema_typed_missing_value_return",
        render_snapshot(
            "sema/proc/typed-missing-value-return.mel",
            include_str!("../../../tests/corpus/sema/proc/typed-missing-value-return.mel"),
        )
    );
}

#[test]
fn snapshot_sema_void_return_value_fixture() {
    insta::assert_snapshot!(
        "sema_void_return_value",
        render_snapshot(
            "sema/proc/void-return-value.mel",
            include_str!("../../../tests/corpus/sema/proc/void-return-value.mel"),
        )
    );
}

#[test]
fn snapshot_sema_typed_return_type_mismatch_fixture() {
    insta::assert_snapshot!(
        "sema_typed_return_type_mismatch",
        render_snapshot(
            "sema/proc/typed-return-type-mismatch.mel",
            include_str!("../../../tests/corpus/sema/proc/typed-return-type-mismatch.mel"),
        )
    );
}

#[test]
fn snapshot_sema_var_init_type_mismatch_fixture() {
    insta::assert_snapshot!(
        "sema_var_init_type_mismatch",
        render_snapshot(
            "sema/proc/var-init-type-mismatch.mel",
            include_str!("../../../tests/corpus/sema/proc/var-init-type-mismatch.mel"),
        )
    );
}

#[test]
fn snapshot_sema_typed_return_type_mismatch_via_call_fixture() {
    insta::assert_snapshot!(
        "sema_typed_return_type_mismatch_via_call",
        render_snapshot(
            "sema/proc/typed-return-type-mismatch-via-call.mel",
            include_str!("../../../tests/corpus/sema/proc/typed-return-type-mismatch-via-call.mel"),
        )
    );
}

#[test]
fn snapshot_sema_var_init_type_mismatch_via_call_fixture() {
    insta::assert_snapshot!(
        "sema_var_init_type_mismatch_via_call",
        render_snapshot(
            "sema/proc/var-init-type-mismatch-via-call.mel",
            include_str!("../../../tests/corpus/sema/proc/var-init-type-mismatch-via-call.mel"),
        )
    );
}

#[test]
fn snapshot_sema_read_before_write_and_shadowing_fixture() {
    insta::assert_snapshot!(
        "sema_read_before_write_and_shadowing",
        render_snapshot(
            "sema/lint/read-before-write-and-shadowing.mel",
            include_str!("../../../tests/corpus/sema/lint/read-before-write-and-shadowing.mel"),
        )
    );
}

#[test]
fn snapshot_sema_unresolved_variable_fixture() {
    insta::assert_snapshot!(
        "sema_unresolved_variable",
        render_snapshot(
            "sema/lint/unresolved-variable.mel",
            include_str!("../../../tests/corpus/sema/lint/unresolved-variable.mel"),
        )
    );
}

#[test]
fn snapshot_sema_delete_selection_omission_fixture() {
    insta::assert_snapshot!(
        "sema_delete_selection_omission",
        render_snapshot(
            "sema/command-schema/delete-selection-omission.mel",
            include_str!("../../../tests/corpus/sema/command-schema/delete-selection-omission.mel"),
        )
    );
}

#[test]
fn snapshot_sema_sets_selection_omission_fixture() {
    insta::assert_snapshot!(
        "sema_sets_selection_omission",
        render_snapshot(
            "sema/command-schema/sets-selection-omission.mel",
            include_str!("../../../tests/corpus/sema/command-schema/sets-selection-omission.mel"),
        )
    );
}

#[test]
fn snapshot_sema_poly_list_component_conversion_selection_omission_fixture() {
    insta::assert_snapshot!(
        "sema_poly_list_component_conversion_selection_omission",
        render_snapshot(
            "sema/command-schema/poly-list-component-conversion-selection-omission.mel",
            include_str!(
                "../../../tests/corpus/sema/command-schema/poly-list-component-conversion-selection-omission.mel"
            ),
        )
    );
}

#[test]
fn snapshot_sema_filter_expand_explicit_list_fixture() {
    insta::assert_snapshot!(
        "sema_filter_expand_explicit_list",
        render_snapshot(
            "sema/command-schema/filter-expand-explicit-list.mel",
            include_str!(
                "../../../tests/corpus/sema/command-schema/filter-expand-explicit-list.mel"
            ),
        )
    );
}

#[test]
fn snapshot_sema_eval_echo_single_script_fixture() {
    insta::assert_snapshot!(
        "sema_eval_echo_single_script",
        render_snapshot(
            "sema/command-schema/eval-echo-single-script.mel",
            include_str!("../../../tests/corpus/sema/command-schema/eval-echo-single-script.mel"),
        )
    );
}

#[test]
fn snapshot_sema_shading_node_single_type_fixture() {
    insta::assert_snapshot!(
        "sema_shading_node_single_type",
        render_snapshot(
            "sema/command-schema/shading-node-single-type.mel",
            include_str!("../../../tests/corpus/sema/command-schema/shading-node-single-type.mel"),
        )
    );
}

#[test]
fn snapshot_sema_poly_edit_uv_explicit_target_fixture() {
    insta::assert_snapshot!(
        "sema_poly_edit_uv_explicit_target",
        render_snapshot(
            "sema/command-schema/poly-edit-uv-explicit-target.mel",
            include_str!(
                "../../../tests/corpus/sema/command-schema/poly-edit-uv-explicit-target.mel"
            ),
        )
    );
}

#[test]
fn snapshot_sema_anim_layer_target_fixture() {
    insta::assert_snapshot!(
        "sema_anim_layer_target",
        render_snapshot(
            "sema/command-schema/anim-layer-target.mel",
            include_str!("../../../tests/corpus/sema/command-schema/anim-layer-target.mel"),
        )
    );
}

#[test]
fn snapshot_sema_reference_query_target_fixture() {
    insta::assert_snapshot!(
        "sema_reference_query_target",
        render_snapshot(
            "sema/command-schema/reference-query-target.mel",
            include_str!("../../../tests/corpus/sema/command-schema/reference-query-target.mel"),
        )
    );
}

#[test]
fn snapshot_sema_tree_view_query_item_fixture() {
    insta::assert_snapshot!(
        "sema_tree_view_query_item",
        render_snapshot(
            "sema/command-schema/tree-view-query-item.mel",
            include_str!("../../../tests/corpus/sema/command-schema/tree-view-query-item.mel"),
        )
    );
}

#[test]
fn snapshot_sema_attribute_exists_two_args_fixture() {
    insta::assert_snapshot!(
        "sema_attribute_exists_two_args",
        render_snapshot(
            "sema/command-schema/attribute-exists-two-args.mel",
            include_str!("../../../tests/corpus/sema/command-schema/attribute-exists-two-args.mel"),
        )
    );
}

#[test]
fn snapshot_sema_set_render_pass_type_target_fixture() {
    insta::assert_snapshot!(
        "sema_set_render_pass_type_target",
        render_snapshot(
            "sema/command-schema/set-render-pass-type-target.mel",
            include_str!(
                "../../../tests/corpus/sema/command-schema/set-render-pass-type-target.mel"
            ),
        )
    );
}

#[test]
fn snapshot_sema_namespace_info_current_fixture() {
    insta::assert_snapshot!(
        "sema_namespace_info_current",
        render_snapshot(
            "sema/command-schema/namespace-info-current.mel",
            include_str!("../../../tests/corpus/sema/command-schema/namespace-info-current.mel"),
        )
    );
}

#[test]
fn snapshot_sema_particle_query_target_fixture() {
    insta::assert_snapshot!(
        "sema_particle_query_target",
        render_snapshot(
            "sema/command-schema/particle-query-target.mel",
            include_str!("../../../tests/corpus/sema/command-schema/particle-query-target.mel"),
        )
    );
}

#[test]
fn snapshot_sema_list_transforms_single_arg_fixture() {
    insta::assert_snapshot!(
        "sema_list_transforms_single_arg",
        render_snapshot(
            "sema/command-schema/list-transforms-single-arg.mel",
            include_str!(
                "../../../tests/corpus/sema/command-schema/list-transforms-single-arg.mel"
            ),
        )
    );
}

#[test]
fn snapshot_sema_move_target_tail_fixture() {
    insta::assert_snapshot!(
        "sema_move_target_tail",
        render_snapshot(
            "sema/command-schema/move-target-tail.mel",
            include_str!("../../../tests/corpus/sema/command-schema/move-target-tail.mel"),
        )
    );
}

#[test]
fn snapshot_sema_for_in_binding_implicit_fixture() {
    insta::assert_snapshot!(
        "sema_for_in_binding_implicit",
        render_snapshot(
            "sema/lint/for-in-binding-implicit.mel",
            include_str!("../../../tests/corpus/sema/lint/for-in-binding-implicit.mel"),
        )
    );
}

#[test]
fn snapshot_sema_boolean_alias_return_fixture() {
    insta::assert_snapshot!(
        "sema_boolean_alias_return",
        render_snapshot(
            "sema/proc/boolean-alias-return.mel",
            include_str!("../../../tests/corpus/sema/proc/boolean-alias-return.mel"),
        )
    );
}

#[test]
fn snapshot_sema_var_init_comparison_int_result_fixture() {
    insta::assert_snapshot!(
        "sema_var_init_comparison_int_result",
        render_snapshot(
            "sema/proc/var-init-comparison-int-result.mel",
            include_str!("../../../tests/corpus/sema/proc/var-init-comparison-int-result.mel"),
        )
    );
}

#[test]
fn snapshot_sema_var_init_comparison_string_target_fixture() {
    insta::assert_snapshot!(
        "sema_var_init_comparison_string_target",
        render_snapshot(
            "sema/proc/var-init-comparison-string-target.mel",
            include_str!("../../../tests/corpus/sema/proc/var-init-comparison-string-target.mel"),
        )
    );
}

#[test]
fn snapshot_sema_var_assign_type_match_fixture() {
    insta::assert_snapshot!(
        "sema_var_assign_type_match",
        render_snapshot(
            "sema/proc/var-assign-type-match.mel",
            include_str!("../../../tests/corpus/sema/proc/var-assign-type-match.mel"),
        )
    );
}

#[test]
fn snapshot_sema_var_assign_type_mismatch_fixture() {
    insta::assert_snapshot!(
        "sema_var_assign_type_mismatch",
        render_snapshot(
            "sema/proc/var-assign-type-mismatch.mel",
            include_str!("../../../tests/corpus/sema/proc/var-assign-type-mismatch.mel"),
        )
    );
}

#[test]
fn snapshot_sema_scripted_panel_flag_mode_span_fixture() {
    insta::assert_snapshot!(
        "sema_scripted_panel_flag_mode_span",
        render_snapshot(
            "sema/lint/scripted-panel-flag-mode-span.mel",
            include_str!("../../../tests/corpus/sema/lint/scripted-panel-flag-mode-span.mel"),
        )
    );
}

#[test]
fn snapshot_sema_scripted_panel_flag_mode_span_tabbed_fixture() {
    insta::assert_snapshot!(
        "sema_scripted_panel_flag_mode_span_tabbed",
        render_snapshot(
            "sema/lint/scripted-panel-flag-mode-span-tabbed.mel",
            include_str!(
                "../../../tests/corpus/sema/lint/scripted-panel-flag-mode-span-tabbed.mel"
            ),
        )
    );
}

#[test]
fn snapshot_sema_scripted_panel_flag_mode_span_tabbed_crlf_inline() {
    insta::assert_snapshot!(
        "sema_scripted_panel_flag_mode_span_tabbed_crlf_inline",
        render_snapshot(
            "inline-crlf-scripted-panel.mel",
            concat!(
                "global string $gMainPane;\r\n",
                "proc string test() {\r\n",
                "\t\t\t$panelName = `scriptedPanel -menuBarVisible true -parent $gMainPane -l \"anyLabel\" -tearOff -type \"acPanelType\"`;\r\n",
                "}\r\n",
            ),
        )
    );
}

#[test]
fn diagnostics_keep_correct_columns_on_triple_digit_line_numbers() {
    let mut source = String::new();
    for _ in 0..99 {
        source.push('\n');
    }
    source.push_str(
        "\t\t\t$panelName = `scriptedPanel -menuBarVisible true -parent $gMainPane -l \"anyLabel\" -tearOff -type \"acPanelType\"`;\n",
    );

    let output = render_snapshot("inline-triple-digit-scripted-panel.mel", &source);
    assert!(output.contains("inline-triple-digit-scripted-panel.mel:100:61"));
    assert!(!output.contains("inline-triple-digit-scripted-panel.mel:100:69"));
}

fn unique_test_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic enough")
        .as_nanos();
    std::env::temp_dir().join(format!("mel-cli-{label}-{nanos}"))
}

fn cleanup_test_path(path: &PathBuf) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_dir_all(path);
}
