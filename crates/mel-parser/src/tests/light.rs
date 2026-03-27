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
