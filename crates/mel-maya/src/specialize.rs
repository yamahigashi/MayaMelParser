use crate::model::{
    MayaAddAttrCommand, MayaAddAttrTailKind, MayaConnectAttrCommand, MayaCreateNodeCommand,
    MayaCurrentUnitCommand, MayaFileCommand, MayaFileInfoCommand, MayaLightAddAttrCommand,
    MayaLightConnectAttrCommand, MayaLightCreateNodeCommand, MayaLightCurrentUnitCommand,
    MayaLightFileCommand, MayaLightFileInfoCommand, MayaLightFlag, MayaLightRelationshipCommand,
    MayaLightRenameCommand, MayaLightRequiresCommand, MayaLightSelectCommand,
    MayaLightSetAttrCommand, MayaLightSpecializedCommand, MayaNormalizedCommand,
    MayaNormalizedFlag, MayaRawShellItem, MayaRawShellItemKind, MayaRelationshipCommand,
    MayaRenameCommand, MayaRequiresCommand, MayaSelectCommand, MayaSetAttrCommand,
    MayaSetAttrValueKind, MayaSpecializedCommand,
};
use crate::normalize::{normalized_flags, normalized_positionals};
use mel_parser::LightParse;
use mel_sema::{CommandSchema, FlagArity, FlagSchema};
use mel_syntax::{SourceView, TextRange, range_end, range_start, text_range};

pub(crate) fn specialize_command(
    source: SourceView<'_>,
    head: &str,
    span: TextRange,
    normalized: Option<&MayaNormalizedCommand>,
    raw_items: &[MayaRawShellItem],
) -> Option<MayaSpecializedCommand> {
    let normalized = normalized?;
    let flags = normalized_flags(normalized);
    let positionals = normalized_positionals(normalized);

    match normalized.schema_name.as_str() {
        "requires" => Some(MayaSpecializedCommand::Requires(MayaRequiresCommand {
            requirements: positionals,
            flags,
            span,
        })),
        "currentUnit" => Some(MayaSpecializedCommand::CurrentUnit(
            MayaCurrentUnitCommand { flags, span },
        )),
        "fileInfo" => Some(MayaSpecializedCommand::FileInfo(MayaFileInfoCommand {
            key: positionals.first().cloned(),
            value: positionals.get(1).cloned(),
            flags,
            span,
        })),
        "createNode" => Some(MayaSpecializedCommand::CreateNode(MayaCreateNodeCommand {
            node_type: positionals.first().cloned(),
            name: first_flag_arg(&flags, "name"),
            parent: first_flag_arg(&flags, "parent"),
            flags,
            span,
        })),
        "rename" => Some(MayaSpecializedCommand::Rename(MayaRenameCommand {
            source: positionals.first().cloned(),
            target: positionals.get(1).cloned(),
            flags,
            span,
        })),
        "select" => Some(MayaSpecializedCommand::Select(MayaSelectCommand {
            targets: positionals,
            flags,
            span,
        })),
        "setAttr" => Some(MayaSpecializedCommand::SetAttr(specialize_set_attr(
            source,
            span,
            &flags,
            &positionals,
        ))),
        "addAttr" => Some(MayaSpecializedCommand::AddAttr(MayaAddAttrCommand {
            tail_kind: classify_add_attr_tail(&positionals),
            tail: positionals,
            flags,
            span,
        })),
        "connectAttr" => Some(MayaSpecializedCommand::ConnectAttr(
            MayaConnectAttrCommand {
                source_attr: positionals.first().cloned(),
                target_attr: positionals.get(1).cloned(),
                flags,
                span,
            },
        )),
        "relationship" => Some(MayaSpecializedCommand::Relationship(
            MayaRelationshipCommand {
                relationship: positionals.first().cloned(),
                members: positionals.into_iter().skip(1).collect(),
                flags,
                span,
            },
        )),
        "file" => Some(MayaSpecializedCommand::File(MayaFileCommand {
            path: positionals
                .last()
                .cloned()
                .or_else(|| raw_items.last().cloned()),
            flags,
            span,
        })),
        _ => {
            let _ = source;
            let _ = head;
            None
        }
    }
}

pub(crate) fn specialize_light_command(
    parse: &LightParse,
    head: &str,
    span: TextRange,
    opaque_tail: Option<TextRange>,
    schema: &CommandSchema,
    prefix_items: &[MayaRawShellItem],
) -> Option<MayaLightSpecializedCommand> {
    let (flags, positionals) = normalize_light_items(parse, schema, prefix_items);
    match schema.name.as_ref() {
        "requires" => Some(MayaLightSpecializedCommand::Requires(
            MayaLightRequiresCommand {
                requirements: positionals,
                flags,
                opaque_tail,
                span,
            },
        )),
        "currentUnit" => Some(MayaLightSpecializedCommand::CurrentUnit(
            MayaLightCurrentUnitCommand {
                flags,
                opaque_tail,
                span,
            },
        )),
        "fileInfo" => Some(MayaLightSpecializedCommand::FileInfo(
            MayaLightFileInfoCommand {
                key: positionals.first().cloned(),
                value: positionals.get(1).cloned(),
                flags,
                opaque_tail,
                span,
            },
        )),
        "createNode" => Some(MayaLightSpecializedCommand::CreateNode(
            MayaLightCreateNodeCommand {
                node_type: positionals.first().cloned(),
                name: first_light_flag_arg(&flags, "name"),
                parent: first_light_flag_arg(&flags, "parent"),
                flags,
                opaque_tail,
                span,
            },
        )),
        "rename" => Some(MayaLightSpecializedCommand::Rename(
            MayaLightRenameCommand {
                source: positionals.first().cloned(),
                target: positionals.get(1).cloned(),
                flags,
                opaque_tail,
                span,
            },
        )),
        "select" => Some(MayaLightSpecializedCommand::Select(
            MayaLightSelectCommand {
                targets: positionals,
                flags,
                opaque_tail,
                span,
            },
        )),
        "setAttr" => Some(MayaLightSpecializedCommand::SetAttr(
            specialize_light_set_attr(parse, span, opaque_tail, &flags, &positionals),
        )),
        "addAttr" => Some(MayaLightSpecializedCommand::AddAttr(
            MayaLightAddAttrCommand {
                tail_kind: classify_add_attr_tail(&positionals),
                tail: positionals,
                flags,
                opaque_tail,
                span,
            },
        )),
        "connectAttr" => Some(MayaLightSpecializedCommand::ConnectAttr(
            MayaLightConnectAttrCommand {
                source_attr: positionals.first().cloned(),
                target_attr: positionals.get(1).cloned(),
                flags,
                opaque_tail,
                span,
            },
        )),
        "relationship" => Some(MayaLightSpecializedCommand::Relationship(
            MayaLightRelationshipCommand {
                relationship: positionals.first().cloned(),
                members: positionals.into_iter().skip(1).collect(),
                flags,
                opaque_tail,
                span,
            },
        )),
        "file" => Some(MayaLightSpecializedCommand::File(MayaLightFileCommand {
            path: positionals.last().cloned(),
            flags,
            opaque_tail,
            span,
        })),
        _ => {
            let _ = parse;
            let _ = head;
            None
        }
    }
}

fn specialize_set_attr(
    source: SourceView<'_>,
    span: TextRange,
    flags: &[MayaNormalizedFlag],
    positionals: &[MayaRawShellItem],
) -> MayaSetAttrCommand {
    let attr_path = positionals.first().cloned();
    let values = positionals.iter().skip(1).cloned().collect::<Vec<_>>();
    let type_name = first_flag_arg(flags, "type");
    let type_text = type_name
        .as_ref()
        .and_then(|item| item.value_text(source))
        .unwrap_or_default();
    let value_kind = match type_text {
        "string" if values.len() == 1 => MayaSetAttrValueKind::String,
        "stringArray" => MayaSetAttrValueKind::StringArray,
        "Int32Array" => MayaSetAttrValueKind::Int32Array,
        "componentList" => MayaSetAttrValueKind::ComponentList,
        "matrix" | "matrixXform" => MayaSetAttrValueKind::MatrixXform,
        "dataReferenceEdits" => MayaSetAttrValueKind::DataReferenceEdits,
        "" if values.len() == 1 && matches!(values[0].kind, MayaRawShellItemKind::QuotedString) => {
            MayaSetAttrValueKind::String
        }
        _ if values.iter().all(is_numeric_like) => MayaSetAttrValueKind::TypedNumbers,
        _ if !type_text.is_empty() => MayaSetAttrValueKind::OpaqueTyped,
        _ => MayaSetAttrValueKind::Unknown,
    };

    MayaSetAttrCommand {
        attr_path,
        type_name,
        value_kind,
        values,
        flags: flags.to_vec(),
        span,
    }
}

fn specialize_light_set_attr(
    parse: &LightParse,
    span: TextRange,
    opaque_tail: Option<TextRange>,
    flags: &[MayaLightFlag],
    positionals: &[MayaRawShellItem],
) -> MayaLightSetAttrCommand {
    let attr_path = positionals.first().cloned();
    let prefix_values = positionals.iter().skip(1).cloned().collect::<Vec<_>>();
    let type_name = first_light_flag_arg(flags, "type");
    let type_text = type_name
        .as_ref()
        .and_then(|item| item.value_text(parse.source_view()))
        .unwrap_or_default();
    let value_kind = match type_text {
        "string" if prefix_values.len() == 1 && opaque_tail.is_none() => {
            MayaSetAttrValueKind::String
        }
        "stringArray" => MayaSetAttrValueKind::StringArray,
        "Int32Array" => MayaSetAttrValueKind::Int32Array,
        "componentList" => MayaSetAttrValueKind::ComponentList,
        "matrix" | "matrixXform" => MayaSetAttrValueKind::MatrixXform,
        "dataReferenceEdits" => MayaSetAttrValueKind::DataReferenceEdits,
        "" if prefix_values.len() == 1
            && opaque_tail.is_none()
            && matches!(prefix_values[0].kind, MayaRawShellItemKind::QuotedString) =>
        {
            MayaSetAttrValueKind::String
        }
        _ if opaque_tail.is_none() && prefix_values.iter().all(is_numeric_like) => {
            MayaSetAttrValueKind::TypedNumbers
        }
        _ if !type_text.is_empty() => MayaSetAttrValueKind::OpaqueTyped,
        _ => MayaSetAttrValueKind::Unknown,
    };

    MayaLightSetAttrCommand {
        attr_path,
        type_name,
        value_kind,
        prefix_values,
        flags: flags.to_vec(),
        opaque_tail,
        span,
    }
}

pub(crate) fn classify_add_attr_tail(positionals: &[MayaRawShellItem]) -> MayaAddAttrTailKind {
    if positionals.is_empty() {
        return MayaAddAttrTailKind::None;
    }
    if positionals.iter().all(is_numeric_like) {
        return MayaAddAttrTailKind::Numeric;
    }
    if positionals
        .iter()
        .all(|item| matches!(item.kind, MayaRawShellItemKind::QuotedString))
    {
        return MayaAddAttrTailKind::String;
    }
    MayaAddAttrTailKind::Mixed
}

pub(crate) fn first_flag_arg(
    flags: &[MayaNormalizedFlag],
    canonical_name: &str,
) -> Option<MayaRawShellItem> {
    flags
        .iter()
        .find(|flag| flag.canonical_name.as_deref() == Some(canonical_name))
        .and_then(|flag| flag.args.first())
        .map(|arg| arg.item.clone())
}

pub(crate) fn first_light_flag_arg(
    flags: &[MayaLightFlag],
    canonical_name: &str,
) -> Option<MayaRawShellItem> {
    flags
        .iter()
        .find(|flag| flag.canonical_name.as_deref() == Some(canonical_name))
        .and_then(|flag| flag.args.first())
        .map(|arg| arg.item.clone())
}

pub(crate) fn is_numeric_like(item: &MayaRawShellItem) -> bool {
    matches!(item.kind, MayaRawShellItemKind::Numeric)
}

fn normalize_light_items(
    parse: &LightParse,
    schema: &CommandSchema,
    items: &[MayaRawShellItem],
) -> (Vec<MayaLightFlag>, Vec<MayaRawShellItem>) {
    let mode = detect_light_mode(parse, schema, items);
    let mut index = 0;
    let mut flags = Vec::new();
    let mut positionals = Vec::new();

    while index < items.len() {
        let item = &items[index];
        if item.kind != MayaRawShellItemKind::Flag {
            positionals.push(item.clone());
            index += 1;
            continue;
        }

        let schema_flag = find_flag_schema(schema, item.source_text(parse.source_view()));
        let expected_arity = schema_flag
            .as_ref()
            .map(|flag| arity_for_mode(flag.arity_by_mode, mode))
            .unwrap_or(FlagArity::None);
        let (_, max_arity) = arity_bounds(expected_arity);
        let mut args = Vec::new();
        let mut consumed = 0;
        while consumed < max_arity {
            let Some(next_item) = items.get(index + 1 + consumed) else {
                break;
            };
            if next_item.kind == MayaRawShellItemKind::Flag {
                break;
            }
            args.push(crate::model::MayaPositionalArg {
                item: next_item.clone(),
            });
            consumed += 1;
        }
        let span = args.last().map_or(item.span, |arg| {
            text_range(range_start(item.span), range_end(arg.item.span))
        });
        flags.push(MayaLightFlag {
            source_range: item.span,
            canonical_name: schema_flag.as_ref().map(|flag| flag.long_name.to_string()),
            args,
            span,
        });
        index += 1 + consumed;
    }

    (flags, positionals)
}

fn detect_light_mode(
    parse: &LightParse,
    schema: &CommandSchema,
    items: &[MayaRawShellItem],
) -> mel_sema::CommandMode {
    let mut create = false;
    let mut edit = false;
    let mut query = false;
    for item in items {
        if item.kind != MayaRawShellItemKind::Flag {
            continue;
        }
        match item
            .source_text(parse.source_view())
            .trim_start_matches('-')
        {
            "create" | "c" if schema.mode_mask.create => create = true,
            "edit" | "e" if schema.mode_mask.edit => edit = true,
            "query" | "q" if schema.mode_mask.query => query = true,
            _ => {}
        }
    }
    match (create, edit, query) {
        (false, false, false) | (true, false, false) => mel_sema::CommandMode::Create,
        (false, true, false) => mel_sema::CommandMode::Edit,
        (false, false, true) => mel_sema::CommandMode::Query,
        _ => mel_sema::CommandMode::Unknown,
    }
}

fn find_flag_schema(command: &CommandSchema, text: &str) -> Option<FlagSchema> {
    let normalized = text.strip_prefix('-').unwrap_or(text);
    command
        .flags
        .iter()
        .find(|flag| {
            normalized == flag.long_name.as_ref()
                || flag
                    .short_name
                    .as_deref()
                    .is_some_and(|short| short == normalized)
        })
        .cloned()
        .or_else(|| synthetic_mode_flag_for_name(command, normalized))
}

fn synthetic_mode_flag_for_name(command: &CommandSchema, name: &str) -> Option<FlagSchema> {
    match name {
        "create" | "c" if command.mode_mask.create => Some(synthetic_mode_flag("create", "c")),
        "edit" | "e" if command.mode_mask.edit => Some(synthetic_mode_flag("edit", "e")),
        "query" | "q" if command.mode_mask.query => Some(synthetic_mode_flag("query", "q")),
        _ => None,
    }
}

fn synthetic_mode_flag(long_name: &str, short_name: &str) -> FlagSchema {
    FlagSchema {
        long_name: long_name.into(),
        short_name: Some(short_name.into()),
        mode_mask: mel_sema::CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        arity_by_mode: mel_sema::FlagArityByMode {
            create: FlagArity::None,
            edit: FlagArity::None,
            query: FlagArity::None,
        },
        value_shapes: Vec::new().into(),
        allows_multiple: false,
    }
}

fn arity_for_mode(
    arity_by_mode: mel_sema::FlagArityByMode,
    mode: mel_sema::CommandMode,
) -> FlagArity {
    match mode {
        mel_sema::CommandMode::Create | mel_sema::CommandMode::Unknown => arity_by_mode.create,
        mel_sema::CommandMode::Edit => arity_by_mode.edit,
        mel_sema::CommandMode::Query => arity_by_mode.query,
    }
}

fn arity_bounds(arity: FlagArity) -> (usize, usize) {
    match arity {
        FlagArity::None => (0, 0),
        FlagArity::Exact(value) => (usize::from(value), usize::from(value)),
        FlagArity::Range { min, max } => (usize::from(min), usize::from(max)),
    }
}
