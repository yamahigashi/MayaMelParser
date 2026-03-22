use mel_ast::ShellWord;
use mel_syntax::{TextRange, range_end, range_start, text_range};

use crate::{
    CommandModeMask, CommandSchema, Diagnostic, DiagnosticSeverity, FlagArity, FlagArityByMode,
    ScopeId, command_schema::CommandKind,
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
    pub source_text: String,
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
    pub head: String,
    pub schema_name: String,
    pub kind: CommandKind,
    pub mode: CommandMode,
    pub items: Vec<NormalizedCommandItem>,
}

pub(crate) fn normalize_shell_like_invoke(
    command: &CommandSchema,
    scope: ScopeId,
    head: &str,
    words: &[ShellWord],
    range: TextRange,
) -> (NormalizedCommandInvoke, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut items = Vec::new();
    let mut seen_flags = Vec::<String>::new();
    let (create_ranges, edit_ranges, query_ranges) = collect_mode_flag_ranges(command, words);
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
                ),
                range,
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
                let Some(schema) = find_flag_schema(command, text) else {
                    diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Warning,
                        message: format!(
                            "unknown flag \"{text}\" for command \"{}\"",
                            command.name
                        ),
                        range: *flag_range,
                    });
                    items.push(NormalizedCommandItem::Flag(NormalizedFlag {
                        source_text: text.clone(),
                        canonical_name: None,
                        args: Vec::new(),
                        range: *flag_range,
                    }));
                    index += 1;
                    continue;
                };

                if !schema.allows_multiple
                    && seen_flags.iter().any(|name| name == &schema.long_name)
                {
                    diagnostics.push(Diagnostic {
                        severity: DiagnosticSeverity::Error,
                        message: format!(
                            "flag \"-{0}\" cannot be repeated for command \"{1}\"",
                            schema.long_name, command.name
                        ),
                        range: *flag_range,
                    });
                } else {
                    seen_flags.push(schema.long_name.clone());
                }

                let expected_arity = arity_for_mode(schema.arity_by_mode, mode);
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
                    args.push(PositionalArg {
                        word: next_word.clone(),
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
                            schema.long_name,
                            format_arity(expected_arity),
                            command.name
                        ),
                        range: *flag_range,
                    });
                }

                let item_range = args.last().map_or(*flag_range, |arg| {
                    text_range(range_start(*flag_range), range_end(arg.range))
                });
                items.push(NormalizedCommandItem::Flag(NormalizedFlag {
                    source_text: text.clone(),
                    canonical_name: Some(schema.long_name.clone()),
                    args,
                    range: item_range,
                }));
                index += 1 + consumed;
            }
            word => {
                items.push(NormalizedCommandItem::Positional(PositionalArg {
                    word: word.clone(),
                    range: shell_word_range(word),
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
            ),
            range,
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
        if !mode_allows(schema.mode_mask, mode) {
            diagnostics.push(Diagnostic {
                severity: if matches!(mode, CommandMode::Query) {
                    DiagnosticSeverity::Warning
                } else {
                    DiagnosticSeverity::Error
                },
                message: format!(
                    "flag \"-{0}\" is not available in {1} mode for command \"{2}\"",
                    schema.long_name,
                    mode_label(mode),
                    command.name
                ),
                range: flag.range,
            });
        }
    }

    (
        NormalizedCommandInvoke {
            range,
            scope,
            head: head.to_owned(),
            schema_name: command.name.clone(),
            kind: command.kind,
            mode,
            items,
        },
        diagnostics,
    )
}

fn find_flag_schema(command: &CommandSchema, text: &str) -> Option<crate::FlagSchema> {
    let normalized = text.strip_prefix('-').unwrap_or(text);
    command
        .flags
        .iter()
        .find(|flag| {
            normalized == flag.long_name
                || flag
                    .short_name
                    .as_deref()
                    .is_some_and(|short| short == normalized)
        })
        .cloned()
        .or_else(|| synthetic_mode_flag_for_name(command, normalized))
}

fn find_flag_schema_by_canonical_name(
    command: &CommandSchema,
    canonical_name: &str,
) -> Option<crate::FlagSchema> {
    command
        .flags
        .iter()
        .find(|flag| flag.long_name == canonical_name)
        .cloned()
        .or_else(|| synthetic_mode_flag_for_name(command, canonical_name))
}

fn synthetic_mode_flag_for_name(command: &CommandSchema, name: &str) -> Option<crate::FlagSchema> {
    match name {
        "create" | "c" if command.mode_mask.create => Some(synthetic_mode_flag("create", "c")),
        "edit" | "e" if command.mode_mask.edit => Some(synthetic_mode_flag("edit", "e")),
        "query" | "q" if command.mode_mask.query => Some(synthetic_mode_flag("query", "q")),
        _ => None,
    }
}

fn synthetic_mode_flag(long_name: &str, short_name: &str) -> crate::FlagSchema {
    crate::FlagSchema {
        long_name: long_name.to_owned(),
        short_name: Some(short_name.to_owned()),
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
        value_shapes: Vec::new(),
        allows_multiple: false,
    }
}

fn collect_mode_flag_ranges(
    command: &CommandSchema,
    words: &[ShellWord],
) -> (Vec<TextRange>, Vec<TextRange>, Vec<TextRange>) {
    let mut create_ranges = Vec::new();
    let mut edit_ranges = Vec::new();
    let mut query_ranges = Vec::new();

    for word in words {
        let ShellWord::Flag { text, range } = word else {
            continue;
        };
        let Some(schema) = find_flag_schema(command, text) else {
            continue;
        };
        match schema.long_name.as_str() {
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
