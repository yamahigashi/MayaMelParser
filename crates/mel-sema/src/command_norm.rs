use mel_ast::ShellWord;
use mel_syntax::{TextRange, range_end, range_start, text_range};

use crate::{
    CommandModeMask, CommandSchema, Diagnostic, FlagArity, ScopeId, command_schema::CommandKind,
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
    let mut query_ranges = Vec::new();
    let mut edit_ranges = Vec::new();
    let mut index = 0;

    while index < words.len() {
        match &words[index] {
            ShellWord::Flag {
                text,
                range: flag_range,
            } => {
                let Some(schema) = find_flag_schema(command, text) else {
                    diagnostics.push(Diagnostic {
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

                if schema.long_name == "query" {
                    query_ranges.push(*flag_range);
                } else if schema.long_name == "edit" {
                    edit_ranges.push(*flag_range);
                }

                if !schema.allows_multiple
                    && seen_flags.iter().any(|name| name == &schema.long_name)
                {
                    diagnostics.push(Diagnostic {
                        message: format!(
                            "flag \"-{0}\" cannot be repeated for command \"{1}\"",
                            schema.long_name, command.name
                        ),
                        range: *flag_range,
                    });
                } else {
                    seen_flags.push(schema.long_name.clone());
                }

                let expected_arity = match schema.arity {
                    FlagArity::None => 0,
                    FlagArity::Exact(value) => usize::from(value),
                };
                let mut args = Vec::new();
                let mut consumed = 0;
                while consumed < expected_arity {
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

                if args.len() != expected_arity {
                    diagnostics.push(Diagnostic {
                        message: format!(
                            "flag \"-{0}\" expects {1} argument(s) for command \"{2}\"",
                            schema.long_name, expected_arity, command.name
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

    let mode = match (edit_ranges.is_empty(), query_ranges.is_empty()) {
        (true, true) => CommandMode::Create,
        (false, true) => CommandMode::Edit,
        (true, false) => CommandMode::Query,
        (false, false) => {
            diagnostics.push(Diagnostic {
                message: format!(
                    "command \"{}\" cannot use both query and edit mode flags together",
                    command.name
                ),
                range,
            });
            CommandMode::Unknown
        }
    };

    for item in &items {
        let NormalizedCommandItem::Flag(flag) = item else {
            continue;
        };
        let Some(canonical_name) = flag.canonical_name.as_deref() else {
            continue;
        };
        let Some(schema) = command
            .flags
            .iter()
            .find(|candidate| candidate.long_name == canonical_name)
        else {
            continue;
        };
        if !mode_allows(schema.mode_mask, mode) {
            diagnostics.push(Diagnostic {
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

fn find_flag_schema<'a>(command: &'a CommandSchema, text: &str) -> Option<&'a crate::FlagSchema> {
    let normalized = text.strip_prefix('-').unwrap_or(text);
    command.flags.iter().find(|flag| {
        normalized == flag.long_name
            || flag
                .short_name
                .as_deref()
                .is_some_and(|short| short == normalized)
    })
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
