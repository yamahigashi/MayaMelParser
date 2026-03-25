use mel_ast::ShellWord;
use mel_syntax::{SourceView, TextRange, range_end, range_start, text_range};
use std::sync::Arc;

use crate::{
    CommandModeMask, CommandSchema, Diagnostic, DiagnosticSeverity, FlagArity, FlagArityByMode,
    PositionalSchema, PositionalSourcePolicy, PositionalTailSchema, ScopeId, ValueShape,
    command_schema::CommandKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandMode {
    Create,
    Edit,
    Query,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionalArg {
    pub word: ShellWord,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedFlag {
    pub source_range: TextRange,
    pub canonical_name: Option<String>,
    pub args: Vec<PositionalArg>,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedCommandItem {
    Flag(NormalizedFlag),
    Positional(PositionalArg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCommandInvoke {
    pub range: TextRange,
    pub scope: ScopeId,
    pub head_range: TextRange,
    pub schema_name: String,
    pub kind: CommandKind,
    pub mode: CommandMode,
    pub items: Vec<NormalizedCommandItem>,
}

#[derive(Debug, Clone, Copy)]
struct BorrowedPositionalArg<'a> {
    word: &'a ShellWord,
    range: TextRange,
}

#[derive(Debug, Clone, Copy)]
struct SyntheticFlagSchema {
    long_name: &'static str,
    mode_mask: CommandModeMask,
    arity_by_mode: FlagArityByMode,
    allows_multiple: bool,
}

#[derive(Debug, Clone, Copy)]
enum ResolvedFlagSchema<'a> {
    Borrowed(&'a crate::FlagSchema),
    Synthetic(SyntheticFlagSchema),
}

impl ResolvedFlagSchema<'_> {
    fn long_name(&self) -> &str {
        match self {
            Self::Borrowed(schema) => &schema.long_name,
            Self::Synthetic(schema) => schema.long_name,
        }
    }

    fn mode_mask(self) -> CommandModeMask {
        match self {
            Self::Borrowed(schema) => schema.mode_mask,
            Self::Synthetic(schema) => schema.mode_mask,
        }
    }

    fn arity_by_mode(self) -> FlagArityByMode {
        match self {
            Self::Borrowed(schema) => schema.arity_by_mode,
            Self::Synthetic(schema) => schema.arity_by_mode,
        }
    }

    fn allows_multiple(self) -> bool {
        match self {
            Self::Borrowed(schema) => schema.allows_multiple,
            Self::Synthetic(schema) => schema.allows_multiple,
        }
    }
}

fn push_primary_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    severity: DiagnosticSeverity,
    range: TextRange,
    message: impl Into<Arc<str>>,
) {
    let message = message.into();
    diagnostics.push(Diagnostic {
        severity,
        message: message.clone(),
        range,
        labels: vec![crate::DiagnosticLabel {
            range,
            message,
            is_primary: true,
        }],
    });
}

pub(crate) fn normalize_shell_like_invoke(
    command: &CommandSchema,
    scope: ScopeId,
    head_range: TextRange,
    words: &[ShellWord],
    range: TextRange,
    source: SourceView<'_>,
) -> (NormalizedCommandInvoke, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut items = Vec::new();
    let mut seen_flags = Vec::<ResolvedFlagSchema<'_>>::new();
    let mut positional_args = Vec::<BorrowedPositionalArg<'_>>::new();
    let (create_ranges, edit_ranges, query_ranges) =
        collect_mode_flag_ranges(command, words, source);
    let active_mode_count = usize::from(!create_ranges.is_empty())
        + usize::from(!edit_ranges.is_empty())
        + usize::from(!query_ranges.is_empty());
    let mode = match active_mode_count {
        0 => CommandMode::Create,
        1 if !create_ranges.is_empty() => CommandMode::Create,
        1 if !edit_ranges.is_empty() => CommandMode::Edit,
        1 if !query_ranges.is_empty() => CommandMode::Query,
        _ => {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!(
                    "command \"{}\" cannot combine create/edit/query mode flags",
                    command.name
                )
                .into(),
                range,
                labels: vec![crate::DiagnosticLabel {
                    range,
                    message: format!(
                        "command \"{}\" cannot combine create/edit/query mode flags",
                        command.name
                    )
                    .into(),
                    is_primary: true,
                }],
            });
            CommandMode::Unknown
        }
    };
    let mut index = 0;

    while index < words.len() {
        match &words[index] {
            ShellWord::Flag {
                text,
                range: flag_range,
            } => {
                let flag_text = source.slice(*text);
                let Some(schema) = find_flag_schema(command, flag_text) else {
                    diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        message: format!(
                            "unknown flag \"{flag_text}\" for command \"{}\"",
                            command.name
                        )
                        .into(),
                        range: *flag_range,
                        labels: vec![crate::DiagnosticLabel {
                            range: *flag_range,
                            message: format!(
                                "unknown flag \"{flag_text}\" for command \"{}\"",
                                command.name
                            )
                            .into(),
                            is_primary: true,
                        }],
                    });
                    items.push(NormalizedCommandItem::Flag(NormalizedFlag {
                        source_range: *flag_range,
                        canonical_name: None,
                        args: Vec::new(),
                        range: *flag_range,
                    }));
                    index += 1;
                    continue;
                };

                if !schema.allows_multiple()
                    && seen_flags
                        .iter()
                        .any(|seen_schema| seen_schema.long_name() == schema.long_name())
                {
                    diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!(
                            "flag \"-{0}\" cannot be repeated for command \"{1}\"",
                            schema.long_name(),
                            command.name
                        )
                        .into(),
                        range: *flag_range,
                        labels: vec![crate::DiagnosticLabel {
                            range: *flag_range,
                            message: format!(
                                "flag \"-{0}\" cannot be repeated for command \"{1}\"",
                                schema.long_name(),
                                command.name
                            )
                            .into(),
                            is_primary: true,
                        }],
                    });
                } else {
                    seen_flags.push(schema);
                }

                let expected_arity = arity_for_mode(schema.arity_by_mode(), mode);
                let (min_arity, max_arity) = arity_bounds(expected_arity);
                let mut args = Vec::new();
                let mut consumed = 0;
                while consumed < max_arity {
                    let next_index = index + 1 + consumed;
                    let Some(next_word) = words.get(next_index) else {
                        break;
                    };
                    if matches!(next_word, ShellWord::Flag { .. }) {
                        break;
                    }
                    args.push(BorrowedPositionalArg {
                        word: next_word,
                        range: shell_word_range(next_word),
                    });
                    consumed += 1;
                }

                let owned_args = args
                    .iter()
                    .map(|arg| PositionalArg {
                        word: arg.word.clone(),
                        range: arg.range,
                    })
                    .collect::<Vec<_>>();

                if args.len() < min_arity {
                    diagnostics.push(Diagnostic {
                        severity: if matches!(mode, CommandMode::Query) {
                            DiagnosticSeverity::Warning
                        } else {
                            DiagnosticSeverity::Error
                        },
                        message: format!(
                            "flag \"-{0}\" expects {1} argument(s) for command \"{2}\"",
                            schema.long_name(),
                            format_arity(expected_arity),
                            command.name
                        )
                        .into(),
                        range: *flag_range,
                        labels: vec![crate::DiagnosticLabel {
                            range: *flag_range,
                            message: format!(
                                "flag \"-{0}\" expects {1} argument(s) for command \"{2}\"",
                                schema.long_name(),
                                format_arity(expected_arity),
                                command.name
                            )
                            .into(),
                            is_primary: true,
                        }],
                    });
                }

                let item_range = args.last().map_or(*flag_range, |arg| {
                    text_range(range_start(*flag_range), range_end(arg.range))
                });
                items.push(NormalizedCommandItem::Flag(NormalizedFlag {
                    source_range: *flag_range,
                    canonical_name: Some(schema.long_name().to_owned()),
                    args: owned_args,
                    range: item_range,
                }));
                index += 1 + consumed;
            }
            word => {
                let positional_arg = BorrowedPositionalArg {
                    word,
                    range: shell_word_range(word),
                };
                positional_args.push(positional_arg);
                items.push(NormalizedCommandItem::Positional(PositionalArg {
                    word: word.clone(),
                    range: positional_arg.range,
                }));
                index += 1;
            }
        }
    }

    if !mode_allows(command.mode_mask, mode) {
        diagnostics.push(Diagnostic {
            severity: if matches!(mode, CommandMode::Query) {
                DiagnosticSeverity::Warning
            } else {
                DiagnosticSeverity::Error
            },
            message: format!(
                "command \"{}\" is not available in {} mode",
                command.name,
                mode_label(mode)
            )
            .into(),
            range,
            labels: vec![crate::DiagnosticLabel {
                range,
                message: format!(
                    "command \"{}\" is not available in {} mode",
                    command.name,
                    mode_label(mode)
                )
                .into(),
                is_primary: true,
            }],
        });
    }

    for item in &items {
        let NormalizedCommandItem::Flag(flag) = item else {
            continue;
        };
        let Some(canonical_name) = flag.canonical_name.as_deref() else {
            continue;
        };
        let Some(schema) = find_flag_schema_by_canonical_name(command, canonical_name) else {
            continue;
        };
        if !mode_allows(schema.mode_mask(), mode) {
            diagnostics.push(Diagnostic {
                severity: if matches!(mode, CommandMode::Query) {
                    DiagnosticSeverity::Warning
                } else {
                    DiagnosticSeverity::Error
                },
                message: format!(
                    "flag \"-{0}\" is not available in {1} mode for command \"{2}\"",
                    schema.long_name(),
                    mode_label(mode),
                    command.name
                )
                .into(),
                range: flag.source_range,
                labels: vec![crate::DiagnosticLabel {
                    range: flag.source_range,
                    message: format!(
                        "flag \"-{0}\" is not available in {1} mode for command \"{2}\"",
                        schema.long_name(),
                        mode_label(mode),
                        command.name
                    )
                    .into(),
                    is_primary: true,
                }],
            });
        }
    }

    let positional_args = positional_args.iter().collect::<Vec<_>>();
    validate_positionals(
        command,
        &command.positionals,
        &positional_args,
        range,
        &mut diagnostics,
        source,
    );

    (
        NormalizedCommandInvoke {
            range,
            scope,
            head_range,
            schema_name: command.name.to_string(),
            kind: command.kind,
            mode,
            items,
        },
        diagnostics,
    )
}

pub(crate) fn collect_command_diagnostics(
    command: &CommandSchema,
    words: &[ShellWord],
    range: TextRange,
    source: SourceView<'_>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen_flags = Vec::<ResolvedFlagSchema<'_>>::new();
    let mut positional_args = Vec::<BorrowedPositionalArg<'_>>::new();
    let mut seen_flag_instances = Vec::<(TextRange, Option<ResolvedFlagSchema<'_>>)>::new();
    let (create_ranges, edit_ranges, query_ranges) =
        collect_mode_flag_ranges(command, words, source);
    let active_mode_count = usize::from(!create_ranges.is_empty())
        + usize::from(!edit_ranges.is_empty())
        + usize::from(!query_ranges.is_empty());
    let mode = match active_mode_count {
        0 => CommandMode::Create,
        1 if !create_ranges.is_empty() => CommandMode::Create,
        1 if !edit_ranges.is_empty() => CommandMode::Edit,
        1 if !query_ranges.is_empty() => CommandMode::Query,
        _ => {
            diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!(
                    "command \"{}\" cannot combine create/edit/query mode flags",
                    command.name
                )
                .into(),
                range,
                labels: vec![crate::DiagnosticLabel {
                    range,
                    message: format!(
                        "command \"{}\" cannot combine create/edit/query mode flags",
                        command.name
                    )
                    .into(),
                    is_primary: true,
                }],
            });
            CommandMode::Unknown
        }
    };
    let mut index = 0;

    while index < words.len() {
        match &words[index] {
            ShellWord::Flag {
                text,
                range: flag_range,
            } => {
                let flag_text = source.slice(*text);
                let Some(schema) = find_flag_schema(command, flag_text) else {
                    diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        message: format!(
                            "unknown flag \"{flag_text}\" for command \"{}\"",
                            command.name
                        )
                        .into(),
                        range: *flag_range,
                        labels: vec![crate::DiagnosticLabel {
                            range: *flag_range,
                            message: format!(
                                "unknown flag \"{flag_text}\" for command \"{}\"",
                                command.name
                            )
                            .into(),
                            is_primary: true,
                        }],
                    });
                    seen_flag_instances.push((*flag_range, None));
                    index += 1;
                    continue;
                };

                if !schema.allows_multiple()
                    && seen_flags
                        .iter()
                        .any(|seen_schema| seen_schema.long_name() == schema.long_name())
                {
                    diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!(
                            "flag \"-{0}\" cannot be repeated for command \"{1}\"",
                            schema.long_name(),
                            command.name
                        )
                        .into(),
                        range: *flag_range,
                        labels: vec![crate::DiagnosticLabel {
                            range: *flag_range,
                            message: format!(
                                "flag \"-{0}\" cannot be repeated for command \"{1}\"",
                                schema.long_name(),
                                command.name
                            )
                            .into(),
                            is_primary: true,
                        }],
                    });
                } else {
                    seen_flags.push(schema);
                }

                let expected_arity = arity_for_mode(schema.arity_by_mode(), mode);
                let (min_arity, max_arity) = arity_bounds(expected_arity);
                let mut args = Vec::new();
                let mut consumed = 0;
                while consumed < max_arity {
                    let next_index = index + 1 + consumed;
                    let Some(next_word) = words.get(next_index) else {
                        break;
                    };
                    if matches!(next_word, ShellWord::Flag { .. }) {
                        break;
                    }
                    args.push(BorrowedPositionalArg {
                        word: next_word,
                        range: shell_word_range(next_word),
                    });
                    consumed += 1;
                }

                if args.len() < min_arity {
                    diagnostics.push(Diagnostic {
                        severity: if matches!(mode, CommandMode::Query) {
                            DiagnosticSeverity::Warning
                        } else {
                            DiagnosticSeverity::Error
                        },
                        message: format!(
                            "flag \"-{0}\" expects {1} argument(s) for command \"{2}\"",
                            schema.long_name(),
                            format_arity(expected_arity),
                            command.name
                        )
                        .into(),
                        range: *flag_range,
                        labels: vec![crate::DiagnosticLabel {
                            range: *flag_range,
                            message: format!(
                                "flag \"-{0}\" expects {1} argument(s) for command \"{2}\"",
                                schema.long_name(),
                                format_arity(expected_arity),
                                command.name
                            )
                            .into(),
                            is_primary: true,
                        }],
                    });
                }
                seen_flag_instances.push((*flag_range, Some(schema)));
                index += 1 + consumed;
            }
            word => {
                positional_args.push(BorrowedPositionalArg {
                    word,
                    range: shell_word_range(word),
                });
                index += 1;
            }
        }
    }

    if !mode_allows(command.mode_mask, mode) {
        diagnostics.push(Diagnostic {
            severity: if matches!(mode, CommandMode::Query) {
                DiagnosticSeverity::Warning
            } else {
                DiagnosticSeverity::Error
            },
            message: format!(
                "command \"{}\" is not available in {} mode",
                command.name,
                mode_label(mode)
            )
            .into(),
            range,
            labels: vec![crate::DiagnosticLabel {
                range,
                message: format!(
                    "command \"{}\" is not available in {} mode",
                    command.name,
                    mode_label(mode)
                )
                .into(),
                is_primary: true,
            }],
        });
    }

    for (flag_range, schema) in seen_flag_instances {
        let Some(schema) = schema else {
            continue;
        };
        if !mode_allows(schema.mode_mask(), mode) {
            diagnostics.push(Diagnostic {
                severity: if matches!(mode, CommandMode::Query) {
                    DiagnosticSeverity::Warning
                } else {
                    DiagnosticSeverity::Error
                },
                message: format!(
                    "flag \"-{0}\" is not available in {1} mode for command \"{2}\"",
                    schema.long_name(),
                    mode_label(mode),
                    command.name
                )
                .into(),
                range: flag_range,
                labels: vec![crate::DiagnosticLabel {
                    range: flag_range,
                    message: format!(
                        "flag \"-{0}\" is not available in {1} mode for command \"{2}\"",
                        schema.long_name(),
                        mode_label(mode),
                        command.name
                    )
                    .into(),
                    is_primary: true,
                }],
            });
        }
    }

    let positional_args = positional_args.iter().collect::<Vec<_>>();
    validate_positionals(
        command,
        &command.positionals,
        &positional_args,
        range,
        &mut diagnostics,
        source,
    );

    diagnostics
}

fn find_flag_schema<'a>(command: &'a CommandSchema, text: &str) -> Option<ResolvedFlagSchema<'a>> {
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
        .map(ResolvedFlagSchema::Borrowed)
        .or_else(|| synthetic_mode_flag_for_name(command, normalized))
}

fn find_flag_schema_by_canonical_name<'a>(
    command: &'a CommandSchema,
    canonical_name: &str,
) -> Option<ResolvedFlagSchema<'a>> {
    command
        .flags
        .iter()
        .find(|flag| flag.long_name.as_ref() == canonical_name)
        .map(ResolvedFlagSchema::Borrowed)
        .or_else(|| synthetic_mode_flag_for_name(command, canonical_name))
}

fn synthetic_mode_flag_for_name(
    command: &CommandSchema,
    name: &str,
) -> Option<ResolvedFlagSchema<'static>> {
    match name {
        "create" | "c" if command.mode_mask.create => Some(ResolvedFlagSchema::Synthetic(
            synthetic_mode_flag("create", "c"),
        )),
        "edit" | "e" if command.mode_mask.edit => Some(ResolvedFlagSchema::Synthetic(
            synthetic_mode_flag("edit", "e"),
        )),
        "query" | "q" if command.mode_mask.query => Some(ResolvedFlagSchema::Synthetic(
            synthetic_mode_flag("query", "q"),
        )),
        _ => None,
    }
}

fn synthetic_mode_flag(long_name: &'static str, short_name: &'static str) -> SyntheticFlagSchema {
    let _ = short_name;
    SyntheticFlagSchema {
        long_name,
        mode_mask: CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        arity_by_mode: FlagArityByMode {
            create: FlagArity::None,
            edit: FlagArity::None,
            query: FlagArity::None,
        },
        allows_multiple: false,
    }
}

fn collect_mode_flag_ranges(
    command: &CommandSchema,
    words: &[ShellWord],
    source: SourceView<'_>,
) -> (Vec<TextRange>, Vec<TextRange>, Vec<TextRange>) {
    let mut create_ranges = Vec::new();
    let mut edit_ranges = Vec::new();
    let mut query_ranges = Vec::new();

    for word in words {
        let ShellWord::Flag { text, range } = word else {
            continue;
        };
        let Some(schema) = find_flag_schema(command, source.slice(*text)) else {
            continue;
        };
        match schema.long_name() {
            "create" => create_ranges.push(*range),
            "edit" => edit_ranges.push(*range),
            "query" => query_ranges.push(*range),
            _ => {}
        }
    }

    (create_ranges, edit_ranges, query_ranges)
}

fn arity_for_mode(arity_by_mode: FlagArityByMode, mode: CommandMode) -> FlagArity {
    match mode {
        CommandMode::Create | CommandMode::Unknown => arity_by_mode.create,
        CommandMode::Edit => arity_by_mode.edit,
        CommandMode::Query => arity_by_mode.query,
    }
}

fn arity_bounds(arity: FlagArity) -> (usize, usize) {
    match arity {
        FlagArity::None => (0, 0),
        FlagArity::Exact(value) => {
            let value = usize::from(value);
            (value, value)
        }
        FlagArity::Range { min, max } => (usize::from(min), usize::from(max)),
    }
}

fn format_arity(arity: FlagArity) -> String {
    match arity {
        FlagArity::None => "0".to_owned(),
        FlagArity::Exact(value) => value.to_string(),
        FlagArity::Range { min, max } if min == max => min.to_string(),
        FlagArity::Range { min, max } => format!("{min} to {max}"),
    }
}

fn validate_positionals(
    command: &CommandSchema,
    schema: &PositionalSchema,
    positional_args: &[&BorrowedPositionalArg<'_>],
    command_range: TextRange,
    diagnostics: &mut Vec<Diagnostic>,
    source: SourceView<'_>,
) {
    let prefix_len = schema.prefix.len();
    let required_prefix_len = required_prefix_len(schema.prefix);
    let positional_len = positional_args.len();

    if prefix_len == 0 && matches!(schema.tail, PositionalTailSchema::None) && positional_len > 0 {
        push_primary_diagnostic(
            diagnostics,
            DiagnosticSeverity::Error,
            command_range,
            format!(
                "command \"{}\" does not accept positional arguments",
                command.name
            ),
        );
        return;
    }

    if positional_len < required_prefix_len {
        push_primary_diagnostic(
            diagnostics,
            DiagnosticSeverity::Error,
            command_range,
            format!(
                "command \"{}\" expects {} positional argument(s) but call provides {}",
                command.name, required_prefix_len, positional_len
            ),
        );
        return;
    }

    for (index, slot) in schema.prefix.iter().enumerate() {
        if let Some(actual_shape) = positional_args
            .get(index)
            .and_then(|arg| inferred_value_shape(arg.word, source))
        {
            validate_positional_shape(
                command,
                index,
                positional_args[index].range,
                actual_shape,
                slot.value_shapes,
                diagnostics,
            );
        }
    }

    let tail_args = &positional_args[prefix_len.min(positional_len)..];
    match schema.tail {
        PositionalTailSchema::None => {
            if !tail_args.is_empty() {
                push_primary_diagnostic(
                    diagnostics,
                    DiagnosticSeverity::Error,
                    command_range,
                    format!(
                        "command \"{}\" expects {} positional argument(s) but call provides {}",
                        command.name, prefix_len, positional_len
                    ),
                );
            }
        }
        PositionalTailSchema::Opaque { min, max } => {
            validate_tail_arity(
                command,
                min,
                max,
                tail_args.len(),
                prefix_len,
                command_range,
                diagnostics,
            );
        }
        PositionalTailSchema::Shaped {
            min,
            max,
            value_shapes,
        } => {
            validate_tail_arity(
                command,
                min,
                max,
                tail_args.len(),
                prefix_len,
                command_range,
                diagnostics,
            );
            for (tail_index, arg) in tail_args.iter().enumerate() {
                let Some(actual_shape) = inferred_value_shape(arg.word, source) else {
                    continue;
                };
                validate_positional_shape(
                    command,
                    prefix_len + tail_index,
                    arg.range,
                    actual_shape,
                    value_shapes,
                    diagnostics,
                );
            }
        }
    }
}

fn required_prefix_len(prefix: &[crate::PositionalSlotSchema]) -> usize {
    let mut seen_optional = false;
    let mut required = 0;
    for slot in prefix {
        let is_optional = matches!(
            slot.source_policy,
            PositionalSourcePolicy::ExplicitOrCurrentSelection
        );
        if is_optional {
            seen_optional = true;
        } else if !seen_optional {
            required += 1;
        } else {
            panic!("selection-aware positional slots must form a trailing suffix");
        }
    }
    required
}

fn validate_tail_arity(
    command: &CommandSchema,
    min: u8,
    max: Option<u8>,
    actual_tail_len: usize,
    prefix_len: usize,
    command_range: TextRange,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let min = usize::from(min);
    let max = max.map(usize::from);
    let actual_total = prefix_len + actual_tail_len;
    let min_total = prefix_len + min;
    let max_total = max.map(|max| prefix_len + max);
    let too_few = actual_tail_len < min;
    let too_many = max.is_some_and(|max| actual_tail_len > max);
    if !too_few && !too_many {
        return;
    }

    let expected = match max_total {
        Some(max_total) if min_total == max_total => min_total.to_string(),
        Some(max_total) => format!("{min_total} to {max_total}"),
        None => format!("{min_total}+"),
    };
    let message = format!(
        "command \"{}\" expects {expected} positional argument(s) but call provides {actual_total}",
        command.name
    );
    let message: std::sync::Arc<str> = message.into();
    diagnostics.push(Diagnostic {
        severity: DiagnosticSeverity::Error,
        message: message.clone(),
        range: command_range,
        labels: vec![crate::DiagnosticLabel {
            range: command_range,
            message,
            is_primary: true,
        }],
    });
}

fn validate_positional_shape(
    command: &CommandSchema,
    index: usize,
    arg_range: TextRange,
    actual_shape: ValueShape,
    allowed_shapes: &[ValueShape],
    diagnostics: &mut Vec<Diagnostic>,
) {
    if allowed_shapes.is_empty()
        || allowed_shapes
            .iter()
            .any(|shape| value_shape_matches(*shape, actual_shape))
    {
        return;
    }

    let expected = format_value_shapes(allowed_shapes);
    let actual = format_value_shape(actual_shape);
    let message = format!(
        "positional argument {} for command \"{}\" expects {} but got {}",
        index + 1,
        command.name,
        expected,
        actual
    );
    let message: std::sync::Arc<str> = message.into();
    diagnostics.push(Diagnostic {
        severity: DiagnosticSeverity::Error,
        message: message.clone(),
        range: arg_range,
        labels: vec![crate::DiagnosticLabel {
            range: arg_range,
            message,
            is_primary: true,
        }],
    });
}

fn inferred_value_shape(word: &ShellWord, source: SourceView<'_>) -> Option<ValueShape> {
    match word {
        ShellWord::NumericLiteral { text, .. } => {
            let text = source.slice(*text);
            if text.contains('.') || text.contains('e') || text.contains('E') {
                Some(ValueShape::Float)
            } else {
                Some(ValueShape::Int)
            }
        }
        ShellWord::QuotedString { .. } => Some(ValueShape::String),
        ShellWord::BareWord { text, .. } => {
            let text = source.slice(*text);
            match text {
                "true" | "false" | "on" | "off" | "yes" | "no" => Some(ValueShape::Bool),
                _ => None,
            }
        }
        ShellWord::VectorLiteral { .. } => Some(ValueShape::FloatTuple(3)),
        ShellWord::Flag { .. }
        | ShellWord::Variable { .. }
        | ShellWord::GroupedExpr { .. }
        | ShellWord::BraceList { .. }
        | ShellWord::Capture { .. } => None,
    }
}

fn value_shape_matches(expected: ValueShape, actual: ValueShape) -> bool {
    matches!(expected, ValueShape::Unknown | ValueShape::Script) || expected == actual
}

fn format_value_shapes(shapes: &[ValueShape]) -> String {
    shapes
        .iter()
        .map(|shape| format_value_shape(*shape))
        .collect::<Vec<_>>()
        .join(" or ")
}

fn format_value_shape(shape: ValueShape) -> String {
    match shape {
        ValueShape::Bool => "bool".to_owned(),
        ValueShape::Int => "int".to_owned(),
        ValueShape::Float => "float".to_owned(),
        ValueShape::String => "string".to_owned(),
        ValueShape::Script => "script".to_owned(),
        ValueShape::StringArray => "string[]".to_owned(),
        ValueShape::FloatTuple(size) => format!("float[{size}]"),
        ValueShape::IntTuple(size) => format!("int[{size}]"),
        ValueShape::NodeName => "node name".to_owned(),
        ValueShape::Unknown => "unknown".to_owned(),
    }
}

fn shell_word_range(word: &ShellWord) -> TextRange {
    match word {
        ShellWord::Flag { range, .. }
        | ShellWord::NumericLiteral { range, .. }
        | ShellWord::BareWord { range, .. }
        | ShellWord::QuotedString { range, .. }
        | ShellWord::Variable { range, .. }
        | ShellWord::GroupedExpr { range, .. }
        | ShellWord::BraceList { range, .. }
        | ShellWord::VectorLiteral { range, .. }
        | ShellWord::Capture { range, .. } => *range,
    }
}

fn mode_allows(mask: CommandModeMask, mode: CommandMode) -> bool {
    match mode {
        CommandMode::Create => mask.create,
        CommandMode::Edit => mask.edit,
        CommandMode::Query => mask.query,
        CommandMode::Unknown => true,
    }
}

fn mode_label(mode: CommandMode) -> &'static str {
    match mode {
        CommandMode::Create => "create",
        CommandMode::Edit => "edit",
        CommandMode::Query => "query",
        CommandMode::Unknown => "unknown",
    }
}
