use crate::model::{
    MayaNormalizedCommand, MayaNormalizedCommandItem, MayaNormalizedFlag, MayaPositionalArg,
    MayaRawShellItem, MayaRawShellItemKind, MayaTopLevelItem,
};
use mel_ast::{ProcDef, ShellWord, Stmt};
use mel_parser::{LightParse, LightWord, Parse};
use mel_sema::{
    CommandMode, CommandSchema, FlagArity, FlagSchema, NormalizedCommandItem, NormalizedFlag,
    PositionalArg,
};
use mel_syntax::{SourceView, TextRange, range_end, range_start, text_range};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NormalizedInvokeKey {
    head_range: TextRange,
    span: TextRange,
}

impl NormalizedInvokeKey {
    const fn new(head_range: TextRange, span: TextRange) -> Self {
        Self { head_range, span }
    }
}

pub(crate) fn normalized_invoke_lookup_from_parse(
    parse: &Parse,
    invokes: Vec<mel_sema::NormalizedCommandInvoke>,
) -> HashMap<NormalizedInvokeKey, MayaNormalizedCommand> {
    normalized_invoke_lookup_from_source(parse.source_view(), invokes)
}

pub(crate) fn normalized_invoke_lookup_from_source(
    source: SourceView<'_>,
    invokes: Vec<mel_sema::NormalizedCommandInvoke>,
) -> HashMap<NormalizedInvokeKey, MayaNormalizedCommand> {
    let mut lookup = HashMap::with_capacity(invokes.len());
    for invoke in invokes {
        let normalized = maya_normalized_command_from_source(source, invoke);
        let key = NormalizedInvokeKey::new(normalized.head_range, normalized.span);
        let previous = lookup.insert(key, normalized);
        debug_assert!(
            previous.is_none(),
            "duplicate normalized invoke for {key:?}"
        );
    }
    lookup
}

pub(crate) fn take_matching_normalized(
    invokes: &mut HashMap<NormalizedInvokeKey, MayaNormalizedCommand>,
    head_range: TextRange,
    range: TextRange,
) -> Option<MayaNormalizedCommand> {
    invokes.remove(&NormalizedInvokeKey::new(head_range, range))
}

pub(crate) fn stmt_range(stmt: &Stmt) -> TextRange {
    match stmt {
        Stmt::Empty { range }
        | Stmt::Proc { range, .. }
        | Stmt::Block { range, .. }
        | Stmt::Expr { range, .. }
        | Stmt::VarDecl { range, .. }
        | Stmt::If { range, .. }
        | Stmt::While { range, .. }
        | Stmt::DoWhile { range, .. }
        | Stmt::Switch { range, .. }
        | Stmt::For { range, .. }
        | Stmt::ForIn { range, .. }
        | Stmt::Return { range, .. }
        | Stmt::Break { range }
        | Stmt::Continue { range } => *range,
    }
}

pub(crate) fn proc_item(parse: &Parse, proc_def: &ProcDef) -> MayaTopLevelItem {
    MayaTopLevelItem::Proc {
        name: parse.source_slice(proc_def.name_range).to_owned(),
        is_global: proc_def.is_global,
        span: proc_def.range,
    }
}

pub(crate) fn normalized_flags(command: &MayaNormalizedCommand) -> Vec<MayaNormalizedFlag> {
    command
        .items
        .iter()
        .filter_map(|item| match item {
            MayaNormalizedCommandItem::Flag(flag) => Some(flag.clone()),
            MayaNormalizedCommandItem::Positional(_) => None,
        })
        .collect()
}

pub(crate) fn normalized_positionals(command: &MayaNormalizedCommand) -> Vec<MayaRawShellItem> {
    command
        .items
        .iter()
        .filter_map(|item| match item {
            MayaNormalizedCommandItem::Flag(_) => None,
            MayaNormalizedCommandItem::Positional(arg) => Some(arg.item.clone()),
        })
        .collect()
}

pub(crate) fn raw_item_from_shell_word(parse: &Parse, word: &ShellWord) -> MayaRawShellItem {
    raw_item_from_shell_word_with_source(parse.source_view(), word)
}

pub(crate) fn raw_item_from_shell_word_with_source(
    source: SourceView<'_>,
    word: &ShellWord,
) -> MayaRawShellItem {
    let _ = source;
    let (kind, span) = match word {
        ShellWord::Flag { range, .. } => (MayaRawShellItemKind::Flag, *range),
        ShellWord::NumericLiteral { range, .. } => (MayaRawShellItemKind::Numeric, *range),
        ShellWord::BareWord { range, .. } => (MayaRawShellItemKind::BareWord, *range),
        ShellWord::QuotedString { range, .. } => (MayaRawShellItemKind::QuotedString, *range),
        ShellWord::Variable { range, .. } => (MayaRawShellItemKind::Variable, *range),
        ShellWord::GroupedExpr { range, .. } => (MayaRawShellItemKind::GroupedExpr, *range),
        ShellWord::BraceList { range, .. } => (MayaRawShellItemKind::BraceList, *range),
        ShellWord::VectorLiteral { range, .. } => (MayaRawShellItemKind::VectorLiteral, *range),
        ShellWord::Capture { range, .. } => (MayaRawShellItemKind::Capture, *range),
    };
    MayaRawShellItem {
        kind,
        span,
        text_range: word.text_range(),
    }
}

pub(crate) fn raw_item_from_light_word(parse: &LightParse, word: &LightWord) -> MayaRawShellItem {
    let span = word.range();
    let _ = parse;
    let kind = match word {
        LightWord::Flag { .. } => MayaRawShellItemKind::Flag,
        LightWord::NumericLiteral { .. } => MayaRawShellItemKind::Numeric,
        LightWord::BareWord { .. } => MayaRawShellItemKind::BareWord,
        LightWord::QuotedString { .. } => MayaRawShellItemKind::QuotedString,
        LightWord::Variable { .. } => MayaRawShellItemKind::Variable,
        LightWord::GroupedExpr { .. } => MayaRawShellItemKind::GroupedExpr,
        LightWord::BraceList { .. } => MayaRawShellItemKind::BraceList,
        LightWord::VectorLiteral { .. } => MayaRawShellItemKind::VectorLiteral,
        LightWord::Capture { .. } => MayaRawShellItemKind::Capture,
    };
    MayaRawShellItem {
        kind,
        span,
        text_range: light_word_text_range(word),
    }
}

pub(crate) fn light_word_text_range(word: &LightWord) -> Option<TextRange> {
    match word {
        LightWord::Flag { text, .. }
        | LightWord::NumericLiteral { text, .. }
        | LightWord::BareWord { text, .. }
        | LightWord::QuotedString { text, .. } => Some(*text),
        LightWord::Variable { .. }
        | LightWord::GroupedExpr { .. }
        | LightWord::BraceList { .. }
        | LightWord::VectorLiteral { .. }
        | LightWord::Capture { .. } => None,
    }
}

pub(crate) fn maya_normalized_command_from_source(
    source: SourceView<'_>,
    value: mel_sema::NormalizedCommandInvoke,
) -> MayaNormalizedCommand {
    MayaNormalizedCommand {
        head: source.slice(value.head_range).to_owned(),
        head_range: value.head_range,
        schema_name: value.schema_name.to_string(),
        kind: value.kind,
        mode: value.mode,
        items: value
            .items
            .into_iter()
            .map(|item| maya_normalized_command_item_from_source(source, item))
            .collect(),
        span: value.range,
    }
}

fn maya_normalized_command_item_from_source(
    source: SourceView<'_>,
    value: NormalizedCommandItem,
) -> MayaNormalizedCommandItem {
    match value {
        NormalizedCommandItem::Flag(flag) => {
            MayaNormalizedCommandItem::Flag(maya_normalized_flag_from_source(source, flag))
        }
        NormalizedCommandItem::Positional(arg) => {
            MayaNormalizedCommandItem::Positional(maya_positional_arg_from_source(source, arg))
        }
    }
}

fn maya_normalized_flag_from_source(
    source: SourceView<'_>,
    value: NormalizedFlag,
) -> MayaNormalizedFlag {
    MayaNormalizedFlag {
        source_range: value.source_range,
        canonical_name: value.canonical_name.map(|name| name.to_string()),
        args: value
            .args
            .into_iter()
            .map(|arg| maya_positional_arg_from_source(source, arg))
            .collect(),
        span: value.range,
    }
}

fn maya_positional_arg_from_source(
    source: SourceView<'_>,
    value: PositionalArg,
) -> MayaPositionalArg {
    MayaPositionalArg {
        item: raw_item_from_shell_word_with_source(source, &value.word),
    }
}

pub(crate) fn command_payload_span(
    head_range: TextRange,
    raw_items: &[MayaRawShellItem],
) -> TextRange {
    let end = raw_items
        .last()
        .map(|item| range_end(item.span))
        .unwrap_or_else(|| range_end(head_range));
    text_range(range_start(head_range), end)
}

pub(crate) fn normalize_light_command(
    parse: &LightParse,
    head: &str,
    head_range: TextRange,
    span: TextRange,
    schema: &CommandSchema,
    items: &[MayaRawShellItem],
) -> MayaNormalizedCommand {
    let mode = detect_light_mode(parse, schema, items);
    let mut normalized_items = Vec::new();
    let mut index = 0;

    while index < items.len() {
        let item = &items[index];
        if item.kind != MayaRawShellItemKind::Flag {
            normalized_items.push(MayaNormalizedCommandItem::Positional(MayaPositionalArg {
                item: item.clone(),
            }));
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
            args.push(MayaPositionalArg {
                item: next_item.clone(),
            });
            consumed += 1;
        }
        let item_span = args.last().map_or(item.span, |arg| {
            text_range(range_start(item.span), range_end(arg.item.span))
        });
        normalized_items.push(MayaNormalizedCommandItem::Flag(MayaNormalizedFlag {
            source_range: item.span,
            canonical_name: schema_flag.as_ref().map(|flag| flag.long_name.to_string()),
            args,
            span: item_span,
        }));
        index += 1 + consumed;
    }

    let normalized_span = normalized_items.last().map_or(span, |item| {
        let end = match item {
            MayaNormalizedCommandItem::Flag(flag) => range_end(flag.span),
            MayaNormalizedCommandItem::Positional(arg) => range_end(arg.item.span),
        };
        text_range(range_start(head_range), end)
    });

    MayaNormalizedCommand {
        head: head.to_owned(),
        head_range,
        schema_name: schema.name.to_string(),
        kind: schema.kind,
        mode,
        items: normalized_items,
        span: normalized_span,
    }
}

fn detect_light_mode(
    parse: &LightParse,
    schema: &CommandSchema,
    items: &[MayaRawShellItem],
) -> CommandMode {
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
        (false, false, false) | (true, false, false) => CommandMode::Create,
        (false, true, false) => CommandMode::Edit,
        (false, false, true) => CommandMode::Query,
        _ => CommandMode::Unknown,
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

fn arity_for_mode(arity_by_mode: mel_sema::FlagArityByMode, mode: CommandMode) -> FlagArity {
    match mode {
        CommandMode::Create | CommandMode::Unknown => arity_by_mode.create,
        CommandMode::Edit => arity_by_mode.edit,
        CommandMode::Query => arity_by_mode.query,
    }
}

fn arity_bounds(arity: FlagArity) -> (usize, usize) {
    match arity {
        FlagArity::None => (0, 0),
        FlagArity::Exact(value) => (usize::from(value), usize::from(value)),
        FlagArity::Range { min, max } => (usize::from(min), usize::from(max)),
    }
}
