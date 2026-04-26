use crate::{
    args::{CliCommand, CliCorpusEngine, CliDiagnosticLevel, parse_cli_args, print_help},
    report::{
        CorpusSummary, LightCorpusSummary, LightSummarySink, ParseReportOptions,
        SelectiveCorpusSummary, SelectiveSummarySink, collect_source_files,
        format_light_corpus_summary, format_selective_corpus_summary,
        print_parse_summary_with_options, summarize_parse_file,
        write_light_scan_single_file_output, write_selective_single_file_output,
        write_single_file_output_with_style_and_options,
    },
};
use maya_mel::maya::{
    collect_selective_top_level_file_with_encoding_and_options_and_sink,
    collect_selective_top_level_file_with_light_options_and_sink,
};
use maya_mel::parser::{
    LightParseOptions, scan_light_file_with_encoding_and_options_and_sink,
    scan_light_file_with_options_and_sink,
};
use maya_mel::{
    ParseBudgets, ParseOptions, SourceEncoding, parse_file_with_encoding_and_options,
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

    if let Some(command) = args.command {
        return run_command(
            command,
            selected_encoding,
            diagnostic_level,
            budgets,
            args.expression,
        )
        .map_err(|error| RunError::Message(error.to_string()));
    }

    if let Some(path) = args.path {
        return print_path_output(
            &path,
            selected_encoding,
            args.lightweight,
            diagnostic_level,
            budgets,
            args.expression,
        )
        .map_err(|error| RunError::Message(error.to_string()));
    }

    if let Some(input) = args.inline_input {
        let parse =
            parse_source_with_options(&input, cli_parse_options(args.expression, true, budgets));
        print_parse_summary_with_options(
            "inline",
            &parse,
            cli_parse_report_options(args.expression, CliDiagnosticLevel::All),
        );
        return Ok(());
    }

    Ok(())
}

pub(crate) fn run_command(
    command: CliCommand,
    encoding: Option<SourceEncoding>,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
    expression: bool,
) -> io::Result<()> {
    match command {
        CliCommand::Parse(command) => {
            if let Some(input) = command.inline_input {
                let parse =
                    parse_source_with_options(&input, cli_parse_options(expression, true, budgets));
                print_parse_summary_with_options(
                    "inline",
                    &parse,
                    cli_parse_report_options(expression, CliDiagnosticLevel::All),
                );
                Ok(())
            } else if let Some(path) = command.path {
                print_single_file_parse(&path, encoding, diagnostic_level, budgets, expression)
            } else {
                print_help().map_err(io::Error::other)
            }
        }
        CliCommand::Scan(command) => {
            reject_expression_for_non_parse(expression, "scan")?;
            print_single_file_scan(&command.path, encoding, diagnostic_level, budgets)
        }
        CliCommand::Selective(command) => {
            reject_expression_for_non_parse(expression, "selective")?;
            print_single_file_selective(&command.path, encoding, diagnostic_level, budgets)
        }
        CliCommand::Corpus(command) => {
            reject_expression_for_non_parse(expression, "corpus")?;
            print_corpus_summary_with_engine(
                &command.root,
                encoding,
                command.engine,
                diagnostic_level,
                budgets,
            )
        }
    }
}

pub(crate) fn print_path_output(
    path: &Path,
    encoding: Option<SourceEncoding>,
    lightweight: bool,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
    expression: bool,
) -> io::Result<()> {
    let metadata = fs::metadata(path)?;

    if expression && lightweight {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--expression cannot be used with --lightweight",
        ));
    }

    if metadata.is_dir() {
        if expression {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--expression can only be used with full single-file or inline parse input",
            ));
        }
        print_corpus_summary(path, encoding, lightweight, diagnostic_level, budgets)
    } else if metadata.is_file() {
        print_single_file(
            path,
            encoding,
            lightweight,
            diagnostic_level,
            budgets,
            expression,
        )
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
    expression: bool,
) -> io::Result<()> {
    if lightweight {
        print_single_file_scan(path, encoding, diagnostic_level, budgets)?;
    } else {
        print_single_file_parse(path, encoding, diagnostic_level, budgets, expression)?;
    }
    Ok(())
}

fn print_single_file_parse(
    path: &Path,
    encoding: Option<SourceEncoding>,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
    expression: bool,
) -> io::Result<()> {
    let label = path.display().to_string();
    let fancy_diagnostics = io::stdout().is_terminal();
    let options = cli_parse_options(expression, false, budgets);
    let parse = if let Some(encoding) = encoding {
        parse_file_with_encoding_and_options(path, encoding, options)?
    } else {
        parse_file_with_options(path, options)?
    };
    write_single_file_output_with_style_and_options(
        io::stdout().lock(),
        &label,
        &parse,
        cli_parse_report_options(expression, diagnostic_level),
        fancy_diagnostics,
    )
}

fn print_single_file_scan(
    path: &Path,
    encoding: Option<SourceEncoding>,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    let label = path.display().to_string();
    let mut sink = LightSummarySink::default();
    let options = LightParseOptions {
        budgets,
        ..LightParseOptions::default()
    };
    let report = if let Some(encoding) = encoding {
        scan_light_file_with_encoding_and_options_and_sink(path, encoding, options, &mut sink)?
    } else {
        scan_light_file_with_options_and_sink(path, options, &mut sink)?
    };
    let summary = sink.finish(path, &report, diagnostic_level);
    write_light_scan_single_file_output(
        io::stdout().lock(),
        &label,
        &report,
        &summary,
        diagnostic_level,
    )
}

fn print_single_file_selective(
    path: &Path,
    encoding: Option<SourceEncoding>,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    let label = path.display().to_string();
    let mut sink = SelectiveSummarySink::default();
    let options = LightParseOptions {
        budgets,
        ..LightParseOptions::default()
    };
    let selective_options = maya_mel::maya::model::MayaSelectiveOptions::default();
    let selector = maya_mel::maya::model::DefaultMayaSelectiveSetAttrSelector;
    let report = if let Some(encoding) = encoding {
        collect_selective_top_level_file_with_encoding_and_options_and_sink(
            path,
            encoding,
            options,
            &selective_options,
            &selector,
            &mut sink,
        )?
    } else {
        collect_selective_top_level_file_with_light_options_and_sink(
            path,
            options,
            &selective_options,
            &selector,
            &mut sink,
        )?
    };
    let summary = sink.finish(path, &report, diagnostic_level);
    write_selective_single_file_output(
        io::stdout().lock(),
        &label,
        &report,
        &summary,
        diagnostic_level,
    )
}

fn print_corpus_summary(
    root: &Path,
    encoding: Option<SourceEncoding>,
    lightweight: bool,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    let engine = if lightweight {
        CliCorpusEngine::Scan
    } else {
        CliCorpusEngine::Full
    };
    print_corpus_summary_with_engine(root, encoding, engine, diagnostic_level, budgets)
}

fn print_corpus_summary_with_engine(
    root: &Path,
    encoding: Option<SourceEncoding>,
    engine: CliCorpusEngine,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    match engine {
        CliCorpusEngine::Full => {
            print_full_corpus_summary(root, encoding, diagnostic_level, budgets)
        }
        CliCorpusEngine::Scan => {
            print_scan_corpus_summary(root, encoding, diagnostic_level, budgets)
        }
        CliCorpusEngine::Selective => {
            print_selective_corpus_summary(root, encoding, diagnostic_level, budgets)
        }
    }
}

fn print_scan_corpus_summary(
    root: &Path,
    encoding: Option<SourceEncoding>,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    let files = collect_source_files(root, true)?;
    let mut summary = LightCorpusSummary::default();
    for path in files {
        summary.files += 1;
        let mut sink = LightSummarySink::default();
        let options = LightParseOptions {
            budgets,
            ..LightParseOptions::default()
        };
        let report = if let Some(encoding) = encoding {
            scan_light_file_with_encoding_and_options_and_sink(&path, encoding, options, &mut sink)
        } else {
            scan_light_file_with_options_and_sink(&path, options, &mut sink)
        };
        match report {
            Ok(report) => summary.record(sink.finish(&path, &report, diagnostic_level)),
            Err(error) => {
                summary.io_errors += 1;
                summary
                    .samples
                    .push(format!("io error: {} ({error})", path.display()));
            }
        }
    }

    println!("{}", format_light_corpus_summary(&summary));
    Ok(())
}

fn print_selective_corpus_summary(
    root: &Path,
    encoding: Option<SourceEncoding>,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    let files = collect_source_files(root, true)?;
    let mut summary = SelectiveCorpusSummary::default();
    let selective_options = maya_mel::maya::model::MayaSelectiveOptions::default();
    let selector = maya_mel::maya::model::DefaultMayaSelectiveSetAttrSelector;
    for path in files {
        summary.files += 1;
        let mut sink = SelectiveSummarySink::default();
        let options = LightParseOptions {
            budgets,
            ..LightParseOptions::default()
        };
        let report = if let Some(encoding) = encoding {
            collect_selective_top_level_file_with_encoding_and_options_and_sink(
                &path,
                encoding,
                options,
                &selective_options,
                &selector,
                &mut sink,
            )
        } else {
            collect_selective_top_level_file_with_light_options_and_sink(
                &path,
                options,
                &selective_options,
                &selector,
                &mut sink,
            )
        };
        match report {
            Ok(report) => summary.record(sink.finish(&path, &report, diagnostic_level)),
            Err(error) => {
                summary.io_errors += 1;
                summary
                    .samples
                    .push(format!("io error: {} ({error})", path.display()));
            }
        }
    }

    println!("{}", format_selective_corpus_summary(&summary));
    Ok(())
}

fn print_full_corpus_summary(
    root: &Path,
    encoding: Option<SourceEncoding>,
    diagnostic_level: CliDiagnosticLevel,
    budgets: ParseBudgets,
) -> io::Result<()> {
    let files = collect_source_files(root, false)?;
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

fn cli_parse_options(expression: bool, inline: bool, budgets: ParseBudgets) -> ParseOptions {
    let mut options = match (expression, inline) {
        (false, false) => ParseOptions::strict(),
        (false, true) => ParseOptions::inline(),
        (true, false) => ParseOptions::expression(),
        (true, true) => ParseOptions::inline_expression(),
    };
    options.budgets = budgets;
    options
}

fn cli_parse_report_options(
    expression: bool,
    diagnostic_level: CliDiagnosticLevel,
) -> ParseReportOptions {
    if expression {
        ParseReportOptions::expression(diagnostic_level)
    } else {
        ParseReportOptions::mel(diagnostic_level)
    }
}

fn reject_expression_for_non_parse(expression: bool, command: &str) -> io::Result<()> {
    if expression {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("--expression can only be used with full parse input, not {command}"),
        ));
    }
    Ok(())
}
