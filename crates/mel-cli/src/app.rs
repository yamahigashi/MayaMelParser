use crate::{
    args::{CliDiagnosticLevel, parse_cli_args, print_help},
    report::{
        CorpusSummary, LightCorpusSummary, collect_source_files, format_light_corpus_summary,
        light_file_summary, print_parse_summary, summarize_parse_file,
        write_light_single_file_output_with_style, write_single_file_output_with_style,
    },
};
use maya_mel::parser::{
    LightParseOptions, parse_light_file_with_encoding_and_options, parse_light_file_with_options,
};
use maya_mel::{
    ParseBudgets, ParseMode, ParseOptions, SourceEncoding, parse_file_with_encoding_and_options,
    parse_file_with_options, parse_source_with_options,
};
use std::{fs, io, io::IsTerminal, path::Path};

#[derive(Debug)]
pub(crate) enum RunError {
    Cli(clap::Error),
    Message(String),
}

pub(crate) fn run() -> Result<(), RunError> {
    let args = parse_cli_args(std::env::args_os()).map_err(RunError::Cli)?;

    if !args.has_input() {
        print_help().map_err(|error| RunError::Message(error.to_string()))?;
        return Ok(());
    }

    let selected_encoding = args.encoding.into_source_encoding();
    let diagnostic_level = args.diagnostic_level;
    let budgets = cli_parse_budgets(args.max_bytes);

    if let Some(path) = args.path {
        return print_path_output(
            &path,
            selected_encoding,
            args.lightweight,
            diagnostic_level,
            budgets,
        )
        .map_err(|error| RunError::Message(error.to_string()));
    }

    if let Some(input) = args.inline_input {
        let parse = parse_source_with_options(
            &input,
            ParseOptions {
                mode: ParseMode::AllowTrailingStmtWithoutSemi,
                budgets,
            },
        );
        print_parse_summary("inline", &parse);
        return Ok(());
    }

    Ok(())
}

pub(crate) fn print_path_output(
    path: &Path,
    encoding: Option<SourceEncoding>,
    lightweight: bool,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    let metadata = fs::metadata(path)?;

    if metadata.is_dir() {
        print_corpus_summary(path, encoding, lightweight, diagnostic_level, budgets)
    } else if metadata.is_file() {
        print_single_file(path, encoding, lightweight, diagnostic_level, budgets)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "path is neither a regular file nor a directory: {}",
                path.display()
            ),
        ))
    }
}

fn print_single_file(
    path: &Path,
    encoding: Option<SourceEncoding>,
    lightweight: bool,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    let label = path.display().to_string();
    let fancy_diagnostics = io::stdout().is_terminal();
    if lightweight {
        let parse = if let Some(encoding) = encoding {
            parse_light_file_with_encoding_and_options(
                path,
                encoding,
                LightParseOptions {
                    budgets,
                    ..LightParseOptions::default()
                },
            )?
        } else {
            parse_light_file_with_options(
                path,
                LightParseOptions {
                    budgets,
                    ..LightParseOptions::default()
                },
            )?
        };
        write_light_single_file_output_with_style(
            io::stdout().lock(),
            &label,
            &parse,
            diagnostic_level,
            fancy_diagnostics,
        )?;
    } else {
        let parse = if let Some(encoding) = encoding {
            parse_file_with_encoding_and_options(
                path,
                encoding,
                ParseOptions {
                    budgets,
                    ..ParseOptions::default()
                },
            )?
        } else {
            parse_file_with_options(
                path,
                ParseOptions {
                    budgets,
                    ..ParseOptions::default()
                },
            )?
        };
        write_single_file_output_with_style(
            io::stdout().lock(),
            &label,
            &parse,
            diagnostic_level,
            fancy_diagnostics,
        )?;
    }
    Ok(())
}

fn print_corpus_summary(
    root: &Path,
    encoding: Option<SourceEncoding>,
    lightweight: bool,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    let files = collect_source_files(root, lightweight)?;
    if lightweight {
        let mut summary = LightCorpusSummary::default();
        for path in files {
            summary.files += 1;

            match encoding
                .map(|encoding| {
                    parse_light_file_with_encoding_and_options(
                        &path,
                        encoding,
                        LightParseOptions {
                            budgets,
                            ..LightParseOptions::default()
                        },
                    )
                })
                .unwrap_or_else(|| {
                    parse_light_file_with_options(
                        &path,
                        LightParseOptions {
                            budgets,
                            ..LightParseOptions::default()
                        },
                    )
                }) {
                Ok(parse) => {
                    let diagnostics =
                        crate::diagnostics::filtered_light_diagnostics(&parse, diagnostic_level);
                    let file_summary = light_file_summary(
                        &path,
                        &parse,
                        crate::diagnostics::diagnostic_counts(&diagnostics),
                    );
                    summary.record(file_summary);
                }
                Err(error) => {
                    summary.io_errors += 1;
                    summary
                        .samples
                        .push(format!("io error: {} ({error})", path.display()));
                }
            }
        }

        println!("{}", format_light_corpus_summary(&summary));
        return Ok(());
    }

    let mut summary = CorpusSummary::default();

    for path in files {
        summary.files += 1;

        match encoding
            .map(|encoding| {
                parse_file_with_encoding_and_options(
                    &path,
                    encoding,
                    ParseOptions {
                        budgets,
                        ..ParseOptions::default()
                    },
                )
            })
            .unwrap_or_else(|| {
                parse_file_with_options(
                    &path,
                    ParseOptions {
                        budgets,
                        ..ParseOptions::default()
                    },
                )
            }) {
            Ok(parse) => {
                let file_summary = summarize_parse_file(&path, &parse, diagnostic_level);
                summary.record(file_summary);
            }
            Err(error) => {
                summary.io_errors += 1;
                summary
                    .samples
                    .push(format!("io error: {} ({error})", path.display()));
            }
        }
    }

    println!("corpus files: {}", summary.files);
    println!(
        "files with decode issues: {}",
        summary.files_with_decode_issues
    );
    println!("files with lex errors: {}", summary.files_with_lex_errors);
    println!(
        "files with parse errors: {}",
        summary.files_with_parse_errors
    );
    println!(
        "files with semantic diagnostics: {}",
        summary.files_with_semantic_diagnostics
    );
    println!("io errors: {}", summary.io_errors);

    if !summary.samples.is_empty() {
        println!("sample issues:");
        for sample in summary.samples.iter().take(10) {
            println!("  {sample}");
        }
    }

    let top_parse_error_files = summary.top_parse_error_files();
    if !top_parse_error_files.is_empty() {
        println!("top parse-error files:");
        for (path, count) in top_parse_error_files {
            println!("  {count:>4} {path}");
        }
    }

    let top_parse_error_messages = summary.top_parse_error_messages();
    if !top_parse_error_messages.is_empty() {
        println!("top parse error messages:");
        for (message, count) in top_parse_error_messages {
            println!("  {count:>4} {message}");
        }
    }

    Ok(())
}

pub(crate) fn cli_parse_budgets(max_bytes: Option<usize>) -> ParseBudgets {
    let default = ParseBudgets::default();
    let Some(max_bytes) = max_bytes else {
        return default;
    };
    if max_bytes == default.max_bytes {
        return default;
    }

    ParseBudgets {
        max_bytes,
        max_nesting_depth: scale_budget(default.max_nesting_depth, max_bytes, default.max_bytes),
        max_tokens: scale_budget(default.max_tokens, max_bytes, default.max_bytes),
        max_statements: scale_budget(default.max_statements, max_bytes, default.max_bytes),
        max_literal_bytes: scale_budget(default.max_literal_bytes, max_bytes, default.max_bytes)
            .min(max_bytes),
    }
}

fn scale_budget(default_value: usize, max_bytes: usize, default_max_bytes: usize) -> usize {
    ((((default_value as u128) * (max_bytes as u128)) / (default_max_bytes as u128))
        .min(usize::MAX as u128) as usize)
        .max(1)
}
