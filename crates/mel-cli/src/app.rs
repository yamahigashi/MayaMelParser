use crate::{
    args::{CliDiagnosticLevel, parse_cli_args, print_help},
    report::{
        CorpusSummary, LightCorpusSummary, collect_source_files, format_light_corpus_summary,
        light_file_summary, print_parse_summary, summarize_parse_file,
        write_light_single_file_output_with_style, write_single_file_output_with_style,
    },
};
use maya_mel as mel_parser;
use mel_parser::{
    ParseMode, ParseOptions, SourceEncoding, parse_file, parse_file_with_encoding,
    parse_light_file, parse_light_file_with_encoding, parse_source_with_options,
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

    if let Some(path) = args.path {
        return print_path_output(&path, selected_encoding, args.lightweight, diagnostic_level)
            .map_err(|error| RunError::Message(error.to_string()));
    }

    if let Some(input) = args.inline_input {
        let parse = parse_source_with_options(
            &input,
            ParseOptions {
                mode: ParseMode::AllowTrailingStmtWithoutSemi,
                ..ParseOptions::default()
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
) -> io::Result<()> {
    let metadata = fs::metadata(path)?;

    if metadata.is_dir() {
        print_corpus_summary(path, encoding, lightweight, diagnostic_level)
    } else if metadata.is_file() {
        print_single_file(path, encoding, lightweight, diagnostic_level)
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
) -> io::Result<()> {
    let label = path.display().to_string();
    let fancy_diagnostics = io::stdout().is_terminal();
    if lightweight {
        let parse = if let Some(encoding) = encoding {
            parse_light_file_with_encoding(path, encoding)?
        } else {
            parse_light_file(path)?
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
            parse_file_with_encoding(path, encoding)?
        } else {
            parse_file(path)?
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
) -> io::Result<()> {
    let files = collect_source_files(root, lightweight)?;
    if lightweight {
        let mut summary = LightCorpusSummary::default();
        for path in files {
            summary.files += 1;

            match encoding
                .map(|encoding| parse_light_file_with_encoding(&path, encoding))
                .unwrap_or_else(|| parse_light_file(&path))
            {
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
            .map(|encoding| parse_file_with_encoding(&path, encoding))
            .unwrap_or_else(|| parse_file(&path))
        {
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
