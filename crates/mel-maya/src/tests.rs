use crate::registry::{OverlayRegistry, push_synthetic_mode_flag};
use crate::*;
use encoding_rs::SHIFT_JIS;
use std::path::Path;

use mel_parser::{
    LightParseOptions, ParseOptions, SourceEncoding, parse_bytes, parse_light_bytes_with_encoding,
    parse_light_file, parse_light_shared_source, parse_light_source,
    parse_light_source_with_options, parse_shared_source, parse_source,
};
use mel_sema::{
    CommandKind, CommandModeMask, CommandRegistry, CommandSchema, CommandSourceKind,
    EmptyCommandRegistry, FlagArity, FlagArityByMode, FlagSchema, PositionalSchema,
    PositionalSourcePolicy, PositionalTailSchema, ReturnBehavior, StaticCommandRegistry,
    ValueShape,
};

fn test_registry(commands: Vec<CommandSchema>) -> StaticCommandRegistry {
    StaticCommandRegistry::try_new(commands).expect("valid test registry")
}

#[test]
fn overlay_registry_prefers_primary_then_embedded_fallback() {
    let primary_command = CommandSchema {
        name: "addAttr".into(),
        kind: CommandKind::Plugin,
        source_kind: CommandSourceKind::Command,
        mode_mask: CommandModeMask {
            create: true,
            edit: false,
            query: false,
        },
        return_behavior: ReturnBehavior::Unknown,
        positionals: PositionalSchema::unconstrained(),
        flags: Vec::new().into(),
    };
    let registry = test_registry(vec![primary_command]);
    let overlay = OverlayRegistry::new(&registry);

    let add_attr = overlay.lookup("addAttr").expect("primary command");
    assert_eq!(add_attr.kind, CommandKind::Plugin);

    let fallback = overlay.lookup("createNode").expect("embedded fallback");
    assert_eq!(fallback.kind, CommandKind::Builtin);
}

#[test]
fn embedded_registry_keeps_script_source_kind() {
    let registry = MayaCommandRegistry::new();
    let schema = registry
        .lookup("addNewShelfTab")
        .expect("embedded schema for addNewShelfTab");
    assert_eq!(schema.kind, CommandKind::Builtin);
    assert_eq!(schema.source_kind, CommandSourceKind::Script);
}

#[test]
fn embedded_registry_synthesizes_mode_flags() {
    let registry = MayaCommandRegistry::new();
    let schema = registry
        .lookup("addAttr")
        .expect("embedded schema for addAttr");
    assert!(
        schema
            .flags
            .iter()
            .any(|flag| flag.long_name.as_ref() == "create")
    );
    assert!(
        schema
            .flags
            .iter()
            .any(|flag| flag.long_name.as_ref() == "edit")
    );
    assert!(
        schema
            .flags
            .iter()
            .any(|flag| flag.long_name.as_ref() == "query")
    );
}

#[test]
fn embedded_registry_keeps_selection_aware_positional_policy() {
    let registry = MayaCommandRegistry::new();
    for command_name in ["ikHandle", "delete", "sets", "polyListComponentConversion"] {
        let schema = registry
            .lookup(command_name)
            .unwrap_or_else(|| panic!("embedded schema for {command_name}"));
        assert_eq!(
            schema.positionals.prefix[0].source_policy,
            PositionalSourcePolicy::ExplicitOrCurrentSelection
        );
    }
}

#[test]
fn embedded_registry_keeps_relaxed_backlog_positional_shapes() {
    let registry = MayaCommandRegistry::new();

    let filter_expand = registry
        .lookup("filterExpand")
        .expect("embedded schema for filterExpand");
    assert!(matches!(
        filter_expand.positionals.tail,
        PositionalTailSchema::Opaque { min: 0, max: None }
    ));

    let shading_node = registry
        .lookup("shadingNode")
        .expect("embedded schema for shadingNode");
    assert_eq!(shading_node.positionals.prefix.len(), 1);
    assert!(matches!(
        shading_node.positionals.tail,
        PositionalTailSchema::Shaped {
            min: 0,
            max: Some(1),
            value_shapes,
        } if value_shapes == [ValueShape::String]
    ));

    let attribute_exists = registry
        .lookup("attributeExists")
        .expect("embedded schema for attributeExists");
    assert_eq!(attribute_exists.positionals.prefix.len(), 2);

    let namespace_info = registry
        .lookup("namespaceInfo")
        .expect("embedded schema for namespaceInfo");
    assert!(namespace_info.positionals.prefix.is_empty());
    assert!(matches!(
        namespace_info.positionals.tail,
        PositionalTailSchema::Opaque { min: 0, max: None }
    ));

    let particle = registry
        .lookup("particle")
        .expect("embedded schema for particle");
    assert_eq!(
        particle.positionals.prefix[0].source_policy,
        PositionalSourcePolicy::ExplicitOrCurrentSelection
    );
    assert!(matches!(
        particle.positionals.tail,
        PositionalTailSchema::Opaque { min: 0, max: None }
    ));
}

#[test]
fn collects_top_level_command_proc_and_other_items() {
    let parse = parse_source("global proc foo() { }\nsetAttr \".tx\" 1;\nint $x = 1;\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    assert!(matches!(facts.items[0], MayaTopLevelItem::Proc { .. }));
    assert!(matches!(facts.items[1], MayaTopLevelItem::Command(_)));
    assert!(matches!(facts.items[2], MayaTopLevelItem::Other { .. }));
}

#[test]
fn shared_full_parse_collects_same_top_level_facts() {
    let parse = parse_shared_source("global proc foo() { }\nsetAttr \".tx\" 1;\n".into());
    let facts = collect_top_level_facts_shared(&parse);
    assert!(matches!(facts.items[0], MayaTopLevelItem::Proc { .. }));
    assert!(matches!(facts.items[1], MayaTopLevelItem::Command(_)));
}

#[test]
fn top_level_command_uses_its_own_normalized_invoke_when_capture_contains_command() {
    let parse = parse_source("print `setAttr \".tx\" 1`;\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    let normalized = command
        .normalized
        .as_ref()
        .expect("expected normalized command");
    assert_eq!(normalized.head, "print");
    assert_eq!(normalized.schema_name, "print");
}

#[test]
fn proc_like_shell_invoke_does_not_disturb_later_command_matching() {
    let parse = parse_source(
        "global proc foo(string $name) { }\ncreateNode transform;\nfoo bar;\nsetAttr \".v\" 1;\n",
    );
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(create_node) = &facts.items[1] else {
        panic!("expected createNode command");
    };
    assert_eq!(
        create_node
            .normalized
            .as_ref()
            .map(|command| command.schema_name.as_str()),
        Some("createNode")
    );
    let MayaTopLevelItem::Command(proc_like) = &facts.items[2] else {
        panic!("expected proc-like shell invoke");
    };
    assert!(proc_like.normalized.is_none());
    let MayaTopLevelItem::Command(set_attr) = &facts.items[3] else {
        panic!("expected setAttr command");
    };
    assert_eq!(
        set_attr
            .normalized
            .as_ref()
            .map(|command| command.schema_name.as_str()),
        Some("setAttr")
    );
}

#[test]
fn raw_items_preserve_exponent_numeric_literals() {
    let parse = parse_source("setAttr \".v\" .5e+2;\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.raw_items[1].kind, MayaRawShellItemKind::Numeric);
    assert_eq!(
        command.raw_items[1].value_text(parse.source_view()),
        Some(".5e+2")
    );
    assert_eq!(
        command.raw_items[1].source_text(parse.source_view()),
        ".5e+2"
    );
}

#[test]
fn shared_light_parse_collects_same_light_and_hybrid_facts() {
    let parse =
        parse_light_shared_source("createNode transform -n \"pCube1\" -p \"|group1\";\n".into());
    let light = collect_top_level_facts_light_shared(&parse);
    assert!(matches!(light.items[0], MayaLightTopLevelItem::Command(_)));

    let hybrid = collect_top_level_facts_hybrid_shared(&parse).expect("hybrid facts");
    let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.head, "createNode");
}

#[test]
fn set_attr_data_reference_edits_is_specialized_losslessly() {
    let parse = parse_source("setAttr \".ed\" -type \"dataReferenceEdits\" \"rootRN\" \"\" 5;\n");
    assert!(parse.errors.is_empty());
    let mut flags = vec![FlagSchema {
        long_name: "type".into(),
        short_name: Some("typ".into()),
        mode_mask: CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        arity_by_mode: FlagArityByMode {
            create: FlagArity::Exact(1),
            edit: FlagArity::Exact(1),
            query: FlagArity::Exact(1),
        },
        value_shapes: vec![ValueShape::String].into(),
        allows_multiple: false,
    }];
    push_synthetic_mode_flag(&mut flags, true, "create", "c");
    push_synthetic_mode_flag(&mut flags, true, "edit", "e");
    push_synthetic_mode_flag(&mut flags, true, "query", "q");
    let command = CommandSchema {
        name: "setAttr".into(),
        kind: CommandKind::Builtin,
        source_kind: CommandSourceKind::Command,
        mode_mask: CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        return_behavior: ReturnBehavior::Unknown,
        positionals: PositionalSchema {
            prefix: &[mel_sema::PositionalSlotSchema {
                value_shapes: &[ValueShape::String],
                source_policy: PositionalSourcePolicy::ExplicitOnly,
            }],
            tail: PositionalTailSchema::Opaque { min: 0, max: None },
        },
        flags: flags.into(),
    };
    let registry = test_registry(vec![command]);

    let facts = collect_top_level_facts_with_registry(&parse, &registry);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    let Some(MayaSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref() else {
        panic!("expected setAttr specialization");
    };
    assert_eq!(
        set_attr.value_kind,
        MayaSetAttrValueKind::DataReferenceEdits
    );
    assert_eq!(
        set_attr
            .type_name
            .as_ref()
            .and_then(|item| item.value_text(parse.source_view())),
        Some("dataReferenceEdits")
    );
    assert_eq!(set_attr.values.len(), 3);
    assert_eq!(
        set_attr.values[2].value_text(parse.source_view()),
        Some("5")
    );
}

#[test]
fn create_node_specialization_extracts_parent_and_name() {
    let parse = parse_source("createNode transform -n \"pCube1\" -p \"|group1\";\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    let Some(MayaSpecializedCommand::CreateNode(create_node)) = command.specialized.as_ref() else {
        panic!("expected createNode specialization");
    };
    assert_eq!(
        create_node
            .name
            .as_ref()
            .and_then(|item| item.value_text(parse.source_view())),
        Some("pCube1")
    );
    assert_eq!(
        create_node
            .parent
            .as_ref()
            .and_then(|item| item.value_text(parse.source_view())),
        Some("|group1")
    );
}

#[test]
fn normalized_flag_accessor_preserves_flag_source_text() {
    let parse = parse_source("setAttr \".v\" -type \"string\" \"hi\";\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    let normalized = command
        .normalized
        .as_ref()
        .expect("expected normalized command");
    let flag = normalized
        .items
        .iter()
        .find_map(|item| match item {
            MayaNormalizedCommandItem::Flag(flag) => Some(flag),
            MayaNormalizedCommandItem::Positional(_) => None,
        })
        .expect("expected flag");
    assert_eq!(flag.source_text(parse.source_view()), "-type");
}

#[test]
fn light_flag_accessor_preserves_flag_source_text() {
    let parse = parse_light_source("setAttr \".v\" -type \"string\" \"hi\";\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts_light(&parse);
    let MayaLightTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    let Some(MayaLightSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref() else {
        panic!("expected light setAttr specialization");
    };
    let flag = set_attr.flags.first().expect("expected flag");
    assert_eq!(flag.source_text(parse.source_view()), "-type");
}

#[test]
fn grouped_expr_raw_item_preserves_full_source_text() {
    let parse = parse_source("setAttr \".b\" -type \"string\" (\"a\" + \"b\");\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.raw_items[3].kind, MayaRawShellItemKind::GroupedExpr);
    assert_eq!(
        command.raw_items[3].source_text(parse.source_view()),
        "(\"a\" + \"b\")"
    );
}

#[test]
fn variable_raw_item_preserves_full_source_text() {
    let parse = parse_source("python $cmd;\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.raw_items[0].kind, MayaRawShellItemKind::Variable);
    assert_eq!(
        command.raw_items[0].source_text(parse.source_view()),
        "$cmd"
    );
}

#[test]
fn brace_list_raw_item_preserves_full_source_text() {
    let parse = parse_source("foo {\"a\", \"b\"};\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.raw_items[0].kind, MayaRawShellItemKind::BraceList);
    assert_eq!(
        command.raw_items[0].source_text(parse.source_view()),
        "{\"a\", \"b\"}"
    );
}

#[test]
fn vector_literal_raw_item_preserves_full_source_text() {
    let parse = parse_source("move <<1, 2, 3>>;\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(
        command.raw_items[0].kind,
        MayaRawShellItemKind::VectorLiteral
    );
    assert_eq!(
        command.raw_items[0].source_text(parse.source_view()),
        "<<1, 2, 3>>"
    );
}

#[test]
fn capture_raw_item_preserves_full_source_text() {
    let parse = parse_source("python `someCmd -q`;\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.raw_items[0].kind, MayaRawShellItemKind::Capture);
    assert_eq!(
        command.raw_items[0].source_text(parse.source_view()),
        "`someCmd -q`"
    );
}

#[test]
fn mixed_shell_word_kinds_remain_lossless() {
    let parse = parse_source("python $cmd (\"a\" + \"b\") {\"x\"} <<1, 2, 3>> `someCmd -q`;\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    let actual = command
        .raw_items
        .iter()
        .map(|item| item.source_text(parse.source_view()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            "$cmd",
            "(\"a\" + \"b\")",
            "{\"x\"}",
            "<<1, 2, 3>>",
            "`someCmd -q`"
        ]
    );
}

#[test]
fn grouped_expr_raw_item_uses_display_range_for_lossless_slice() {
    let bytes = b"setAttr \".b\" -type \"string\" (\"\xFF\");\n";
    let parse = parse_bytes(bytes);
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.raw_items[3].kind, MayaRawShellItemKind::GroupedExpr);
    assert_eq!(
        command.raw_items[3].source_text(parse.source_view()),
        "(\"\u{FFFD}\")"
    );
}

#[test]
fn quoted_string_raw_item_exposes_text_range_used_for_value_text() {
    let parse = parse_source("setAttr \".label\" -type \"string\" \"value\";\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    let item = &command.raw_items[3];
    assert_eq!(item.kind, MayaRawShellItemKind::QuotedString);
    assert_eq!(item.text_range(), Some(item.span));
    assert_eq!(item.source_text(parse.source_view()), "\"value\"");
    assert_eq!(item.value_text(parse.source_view()), Some("value"));
}

#[test]
fn grouped_expr_raw_item_has_no_text_range() {
    let parse = parse_source("setAttr \".b\" -type \"string\" (\"a\" + \"b\");\n");
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts(&parse);
    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    let item = &command.raw_items[3];
    assert_eq!(item.kind, MayaRawShellItemKind::GroupedExpr);
    assert_eq!(item.text_range(), None);
    assert_eq!(item.value_text(parse.source_view()), None);
}

#[test]
fn light_collector_keeps_heavy_set_attr_tail_opaque() {
    let parse = parse_light_source(
        "createNode mesh -n \"meshShape\";\nsetAttr \".fc[0]\" -type \"polyFaces\" f 4 0 1 2 3 mu 0 4 0 1 2 3;\n",
    );
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts_light(&parse);
    assert!(matches!(facts.items[0], MayaLightTopLevelItem::Command(_)));
    let MayaLightTopLevelItem::Command(command) = &facts.items[1] else {
        panic!("expected command");
    };
    let Some(MayaLightSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref() else {
        panic!("expected light setAttr specialization");
    };
    assert_eq!(
        set_attr
            .attr_path
            .as_ref()
            .and_then(|item| item.value_text(parse.source_view())),
        Some(".fc[0]")
    );
    assert_eq!(
        set_attr
            .type_name
            .as_ref()
            .and_then(|item| item.value_text(parse.source_view())),
        Some("polyFaces")
    );
    assert!(command.opaque_tail.is_none());
    assert_eq!(
        set_attr.prefix_values[0].value_text(parse.source_view()),
        Some("f")
    );
}

#[test]
fn light_collector_uses_opaque_tail_when_prefix_limit_hits() {
    let parse = mel_parser::parse_light_source_with_options(
        "setAttr \".pt\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n",
        mel_parser::LightParseOptions {
            max_prefix_words: 5,
            max_prefix_bytes: 32,
        },
    );
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts_light(&parse);
    let MayaLightTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    let Some(MayaLightSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref() else {
        panic!("expected light setAttr specialization");
    };
    assert!(command.opaque_tail.is_some());
    assert!(set_attr.opaque_tail.is_some());
    assert_eq!(set_attr.prefix_values.len(), 2);
}

#[test]
fn light_collector_can_smoke_tmp_mesh_sample_when_present() {
    let path = Path::new("/mnt/e/Projects/RnD/MayaMelParser/tmp/Test_Mesh_Horizon.ma");
    if !path.exists() {
        return;
    }
    let parse = parse_light_file(path).expect("light parse sample");
    assert!(parse.decode_errors.is_empty());
    assert!(parse.errors.is_empty());
    let facts = collect_top_level_facts_light(&parse);
    assert!(!facts.items.is_empty());
}

#[test]
fn hybrid_collector_matches_full_shape_for_non_opaque_command() {
    let full = collect_top_level_facts(&parse_source(
        "createNode transform -n \"pCube1\" -p \"|group1\";\n",
    ));
    let light = parse_light_source("createNode transform -n \"pCube1\" -p \"|group1\";\n");
    let hybrid = collect_top_level_facts_hybrid(&light).expect("hybrid facts");

    let MayaTopLevelItem::Command(full_command) = &full.items[0] else {
        panic!("expected full command");
    };
    let MayaTopLevelItem::Command(hybrid_command) = &hybrid.items[0] else {
        panic!("expected hybrid command");
    };
    assert_eq!(hybrid_command.head, full_command.head);
    assert_eq!(hybrid_command.raw_items, full_command.raw_items);
    assert_eq!(hybrid_command.normalized, full_command.normalized);
    assert_eq!(hybrid_command.specialized, full_command.specialized);
    assert_eq!(full_command.promotion_kind, MayaPromotionKind::FullParse);
    assert_eq!(
        hybrid_command.promotion_kind,
        MayaPromotionKind::LightSynthesized
    );
}

#[test]
fn hybrid_collector_promotes_opaque_set_attr_tail_to_full_raw_items() {
    let parse = parse_light_source_with_options(
        "setAttr \".pt\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n",
        LightParseOptions {
            max_prefix_words: 5,
            max_prefix_bytes: 32,
        },
    );
    let hybrid = collect_top_level_facts_hybrid(&parse).expect("hybrid facts");
    let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
        panic!("expected command");
    };
    assert!(command.raw_items.len() > 5);
    assert_eq!(
        command.promotion_kind,
        MayaPromotionKind::OpaqueTailPromoted
    );
    let Some(MayaSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref() else {
        panic!("expected setAttr specialization");
    };
    assert_eq!(set_attr.values.len(), 10);
}

#[test]
fn promoted_command_keeps_cp932_source_slices() {
    let parse = parse_light_bytes_with_encoding(
        SHIFT_JIS
            .encode("setAttr \".名\" -type \"string\" \"値\";\n")
            .0
            .as_ref(),
        mel_parser::SourceEncoding::Cp932,
    );
    let hybrid = collect_top_level_facts_hybrid_with_registry(
        &parse,
        &EmptyCommandRegistry,
        MayaPromotionPolicy::Always,
    )
    .expect("hybrid facts");
    let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.promotion_kind, MayaPromotionKind::PolicyPromoted);
    assert_eq!(
        command.raw_items[0].source_text(parse.source_view()),
        "\".名\""
    );
    assert_eq!(
        command.raw_items[3].value_text(parse.source_view()),
        Some("値")
    );
}

#[test]
fn hybrid_always_policy_promotes_file_command_with_grouped_expr() {
    let parse = parse_light_source("file -command (\"print \\\"hi\\\";\");\n");
    let hybrid = collect_top_level_facts_hybrid_with_registry(
        &parse,
        &EmptyCommandRegistry,
        MayaPromotionPolicy::Always,
    )
    .expect("hybrid facts");
    let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.promotion_kind, MayaPromotionKind::PolicyPromoted);
    assert_eq!(command.raw_items[1].kind, MayaRawShellItemKind::GroupedExpr);
    let Some(MayaSpecializedCommand::File(file)) = command.specialized.as_ref() else {
        panic!("expected file specialization");
    };
    assert_eq!(file.flags.len(), 1);
}

#[test]
fn hybrid_custom_decider_promotes_grouped_expr_command() {
    let parse = parse_light_source("file -command (\"print \\\"hi\\\";\");\n");
    let decider: &dyn MayaPromotionDecider = &|candidate: MayaPromotionCandidate<'_>| {
        candidate
            .command
            .words
            .iter()
            .any(|word| matches!(word, mel_parser::LightWord::GroupedExpr { .. }))
    };
    let hybrid = collect_top_level_facts_hybrid_with_decider(
        &parse,
        &MayaPromotionOptions::default(),
        decider,
    )
    .expect("hybrid facts");

    let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
        panic!("expected command");
    };
    assert_eq!(
        command.promotion_kind,
        MayaPromotionKind::CustomDeciderPromoted
    );
    assert_eq!(command.raw_items[1].kind, MayaRawShellItemKind::GroupedExpr);
}

#[test]
fn opaque_tail_promotion_takes_precedence_over_custom_decider() {
    let parse = parse_light_source_with_options(
        "setAttr \".pt\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n",
        LightParseOptions {
            max_prefix_words: 5,
            max_prefix_bytes: 32,
        },
    );
    let hybrid = collect_top_level_facts_hybrid_with_decider(
        &parse,
        &MayaPromotionOptions::default(),
        &|_: MayaPromotionCandidate<'_>| true,
    )
    .expect("hybrid facts");

    let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
        panic!("expected command");
    };
    assert_eq!(
        command.promotion_kind,
        MayaPromotionKind::OpaqueTailPromoted
    );
}

#[test]
fn hybrid_promotes_data_reference_edits_tail() {
    let parse = parse_light_source_with_options(
        "setAttr \".ed\" -type \"dataReferenceEdits\" \"rootRN\" \"\" 5;\n",
        LightParseOptions {
            max_prefix_words: 3,
            max_prefix_bytes: 24,
        },
    );
    let mut flags = vec![FlagSchema {
        long_name: "type".into(),
        short_name: Some("typ".into()),
        mode_mask: CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        arity_by_mode: FlagArityByMode {
            create: FlagArity::Exact(1),
            edit: FlagArity::Exact(1),
            query: FlagArity::Exact(1),
        },
        value_shapes: vec![ValueShape::String].into(),
        allows_multiple: false,
    }];
    push_synthetic_mode_flag(&mut flags, true, "create", "c");
    push_synthetic_mode_flag(&mut flags, true, "edit", "e");
    push_synthetic_mode_flag(&mut flags, true, "query", "q");
    let command = CommandSchema {
        name: "setAttr".into(),
        kind: CommandKind::Builtin,
        source_kind: CommandSourceKind::Command,
        mode_mask: CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        return_behavior: ReturnBehavior::Unknown,
        positionals: PositionalSchema {
            prefix: &[mel_sema::PositionalSlotSchema {
                value_shapes: &[ValueShape::String],
                source_policy: PositionalSourcePolicy::ExplicitOnly,
            }],
            tail: PositionalTailSchema::Opaque { min: 0, max: None },
        },
        flags: flags.into(),
    };
    let registry = test_registry(vec![command]);

    let hybrid = collect_top_level_facts_hybrid_with_registry(
        &parse,
        &registry,
        MayaPromotionPolicy::default(),
    )
    .expect("hybrid facts");
    let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
        panic!("expected command");
    };
    assert_eq!(
        command.promotion_kind,
        MayaPromotionKind::OpaqueTailPromoted
    );
    let Some(MayaSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref() else {
        panic!("expected setAttr specialization");
    };
    assert_eq!(
        set_attr.value_kind,
        MayaSetAttrValueKind::DataReferenceEdits
    );
    assert_eq!(set_attr.values.len(), 3);
}

#[test]
fn hybrid_report_keeps_light_command_when_policy_promotion_fails() {
    let parse = parse_light_source("createNode transform -n \"pCube1\"");
    let report = collect_top_level_facts_hybrid_report(
        &parse,
        &MayaPromotionOptions {
            policy: MayaPromotionPolicy::Always,
            ..MayaPromotionOptions::default()
        },
    );

    assert_eq!(report.promotion_diagnostics.len(), 1);
    assert_eq!(
        report.promotion_diagnostics[0].attempted_kind,
        MayaPromotionKind::PolicyPromoted
    );
    let MayaTopLevelItem::Command(command) = &report.facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.promotion_kind, MayaPromotionKind::LightSynthesized);
}

#[test]
fn hybrid_report_keeps_light_command_when_custom_decider_promotion_fails() {
    let parse = parse_light_source("file -command (\"print \\\"hi\\\";\")");
    let report = collect_top_level_facts_hybrid_report_with_decider(
        &parse,
        &MayaPromotionOptions::default(),
        &|candidate: MayaPromotionCandidate<'_>| {
            candidate
                .command
                .words
                .iter()
                .any(|word| matches!(word, mel_parser::LightWord::GroupedExpr { .. }))
        },
    );

    assert_eq!(report.promotion_diagnostics.len(), 1);
    assert_eq!(
        report.promotion_diagnostics[0].attempted_kind,
        MayaPromotionKind::CustomDeciderPromoted
    );
    let MayaTopLevelItem::Command(command) = &report.facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.promotion_kind, MayaPromotionKind::LightSynthesized);
}

#[test]
fn hybrid_report_collects_set_attr_validation_diagnostic() {
    let parse = parse_light_source("setAttr \".tx\" -type \"string\";\n");
    let report = collect_top_level_facts_hybrid_report(&parse, &MayaPromotionOptions::default());

    assert_eq!(report.validation_diagnostics.len(), 1);
    assert_eq!(
        report.validation_diagnostics[0].head.as_deref(),
        Some("setAttr")
    );
    assert_eq!(
        report.validation_diagnostics[0].message,
        "setAttr requires at least one value after the attribute path when -type is present"
    );
}

#[test]
fn hybrid_report_leaves_validation_diagnostics_empty_for_valid_set_attr() {
    let parse = parse_light_source("setAttr \".tx\" -type \"string\" \"value\";\n");
    let report = collect_top_level_facts_hybrid_report(&parse, &MayaPromotionOptions::default());

    assert!(report.validation_diagnostics.is_empty());
}

#[test]
fn hybrid_strict_options_forward_parse_mode_to_promotion() {
    let parse = parse_light_source("createNode transform -n \"pCube1\"");
    let facts = collect_top_level_facts_hybrid_with_registry_and_options(
        &parse,
        &EmptyCommandRegistry,
        &MayaPromotionOptions {
            policy: MayaPromotionPolicy::Always,
            parse_options: ParseOptions {
                mode: mel_parser::ParseMode::AllowTrailingStmtWithoutSemi,
            },
        },
    )
    .expect("hybrid facts");

    let MayaTopLevelItem::Command(command) = &facts.items[0] else {
        panic!("expected command");
    };
    assert_eq!(command.promotion_kind, MayaPromotionKind::PolicyPromoted);
    assert_eq!(
        command.raw_items[0].source_text(parse.source_view()),
        "transform"
    );
}

#[test]
fn selective_collector_extracts_target_commands_only() {
    let source = "rename \"a\" \"b\";\ncreateNode mesh -n \"meshShape\";\nsetAttr \".b\" yes;\n";
    let mut items = Vec::new();
    let report =
        collect_selective_top_level_source_with_sink(source, &mut |item: MayaSelectiveItem| {
            items.push(item)
        });

    assert!(report.errors.is_empty());
    assert_eq!(items.len(), 2);
    let MayaSelectiveItem::CreateNode(create_node) = &items[0] else {
        panic!("expected createNode item");
    };
    assert_eq!(
        create_node
            .node_type_range
            .map(|range| report.source_slice(range)),
        Some("mesh")
    );
    let MayaSelectiveItem::SetAttr(set_attr) = &items[1] else {
        panic!("expected setAttr item");
    };
    assert_eq!(set_attr.tracked_attr, Some(MayaTrackedSetAttrAttr::B));
}

#[test]
fn selective_collector_can_include_other_commands_as_passthrough() {
    let source = "rename \"a\" \"b\";\ncreateNode transform;\n";
    let mut items = Vec::new();
    let report = collect_selective_top_level_source_with_options_and_sink(
        source,
        LightParseOptions::default(),
        &MayaSelectiveOptions {
            passthrough: MayaSelectivePassthrough::IncludeOtherCommands,
        },
        &DefaultMayaSelectiveSetAttrSelector,
        &mut |item: MayaSelectiveItem| items.push(item),
    );

    assert!(report.errors.is_empty());
    assert_eq!(items.len(), 2);
    let MayaSelectiveItem::OtherCommand { head_range, .. } = &items[0] else {
        panic!("expected passthrough command");
    };
    assert_eq!(report.source_slice(*head_range), "rename");
}

#[test]
fn selective_collector_keeps_opaque_set_attr_without_promotion() {
    let source = "setAttr \".f\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n";
    let mut items = Vec::new();
    let report = collect_selective_top_level_source_with_options_and_sink(
        source,
        LightParseOptions {
            max_prefix_words: 4,
            max_prefix_bytes: 24,
        },
        &MayaSelectiveOptions::default(),
        &DefaultMayaSelectiveSetAttrSelector,
        &mut |item: MayaSelectiveItem| items.push(item),
    );

    assert!(report.errors.is_empty());
    let MayaSelectiveItem::SetAttr(set_attr) = &items[0] else {
        panic!("expected setAttr item");
    };
    assert_eq!(set_attr.tracked_attr, Some(MayaTrackedSetAttrAttr::F));
    assert!(set_attr.opaque_tail.is_some());
}

#[test]
fn selective_collector_uses_custom_set_attr_selector() {
    let source = "setAttr \".custom\" 1;\n";
    let mut items = Vec::new();
    let report = collect_selective_top_level_source_with_options_and_sink(
        source,
        LightParseOptions::default(),
        &MayaSelectiveOptions::default(),
        &|attr_path: &str| (attr_path == ".custom").then_some(MayaTrackedSetAttrAttr::Fn),
        &mut |item: MayaSelectiveItem| items.push(item),
    );

    assert!(report.errors.is_empty());
    let MayaSelectiveItem::SetAttr(set_attr) = &items[0] else {
        panic!("expected setAttr item");
    };
    assert_eq!(set_attr.tracked_attr, Some(MayaTrackedSetAttrAttr::Fn));
}

#[test]
fn selective_collector_preserves_cp932_source_slices() {
    let bytes = SHIFT_JIS
        .encode("setAttr \".名\" -type \"string\" \"値\";\n")
        .0;
    let mut items = Vec::new();
    let report = collect_selective_top_level_bytes_with_encoding_and_sink(
        bytes.as_ref(),
        SourceEncoding::Cp932,
        &MayaSelectiveOptions::default(),
        &DefaultMayaSelectiveSetAttrSelector,
        &mut |item: MayaSelectiveItem| items.push(item),
    );

    let MayaSelectiveItem::SetAttr(set_attr) = &items[0] else {
        panic!("expected setAttr item");
    };
    assert_eq!(
        set_attr
            .attr_path_range
            .map(|range| report.source_slice(range)),
        Some("\".名\"")
    );
}
