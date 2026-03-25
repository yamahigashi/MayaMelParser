#![forbid(unsafe_code)]

use ariadne::{Color, Config, Label, Report, ReportKind, Source};
use clap::{CommandFactory, Parser, ValueEnum};
use std::{
    collections::HashMap,
    fmt::Write as FmtWrite,
    fs, io,
    io::IsTerminal,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use mel_maya::{
    MayaCommandRegistry, MayaLightSpecializedCommand, MayaLightTopLevelItem,
    collect_top_level_facts_light,
};
use mel_parser::{
    LightParse, Parse, ParseMode, ParseOptions, SourceEncoding, parse_file,
    parse_file_with_encoding, parse_light_file, parse_light_file_with_encoding,
    parse_source_with_options,
};
use mel_sema::{
    DiagnosticFilter, DiagnosticLabel, DiagnosticSeverity, analyze_diagnostics_with_registry,
    analyze_diagnostics_with_registry_filtered,
};
use mel_syntax::{SourceMap, TextRange, range_end, range_start, text_range};

const TOP_RANK_LIMIT: usize = 10;
const DIAGNOSTIC_TAB_WIDTH: usize = 1;

#[derive(Debug, Parser)]
#[command(about = "Inspect MEL parse and diagnostic output", long_about = None)]
struct Args {
    #[arg(long, value_enum, default_value_t = CliEncoding::Auto)]
    encoding: CliEncoding,
    #[arg(long, value_enum, default_value_t = CliDiagnosticLevel::All)]
    diagnostic_level: CliDiagnosticLevel,
    #[arg(long, conflicts_with = "inline_input")]
    lightweight: bool,
    #[arg(value_name = "PATH", conflicts_with = "inline_input")]
    path: Option<PathBuf>,
    #[arg(long = "inline", value_name = "SOURCE", conflicts_with = "path")]
    inline_input: Option<String>,
}

impl Args {
    fn has_input(&self) -> bool {
        self.path.is_some() || self.inline_input.is_some()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum CliEncoding {
    #[default]
    #[value(name = "auto")]
    Auto,
    #[value(name = "utf8")]
    Utf8,
    #[value(name = "cp932")]
    Cp932,
    #[value(name = "gbk")]
    Gbk,
}

impl CliEncoding {
    fn into_source_encoding(self) -> Option<SourceEncoding> {
        match self {
            Self::Auto => None,
            Self::Utf8 => Some(SourceEncoding::Utf8),
            Self::Cp932 => Some(SourceEncoding::Cp932),
            Self::Gbk => Some(SourceEncoding::Gbk),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum CliDiagnosticLevel {
    #[default]
    #[value(name = "all")]
    All,
    #[value(name = "error")]
    Error,
    #[value(name = "none")]
    None,
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(RunError::Cli(error)) => error.exit(),
        Err(RunError::Message(error)) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}

#[derive(Debug)]
enum RunError {
    Cli(clap::Error),
    Message(String),
}

fn run() -> Result<(), RunError> {
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
            },
        );
        print_parse_summary("inline", &parse);
        return Ok(());
    }

    Ok(())
}

fn print_path_output(
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

fn parse_cli_args<I, T>(args: I) -> Result<Args, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Args::try_parse_from(args)
}

fn print_help() -> io::Result<()> {
    let mut command = Args::command();
    command.print_help()?;
    println!();
    Ok(())
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
                    let diagnostics = filtered_light_diagnostics(&parse, diagnostic_level);
                    let file_summary =
                        light_file_summary(&path, &parse, diagnostic_counts(&diagnostics));
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

fn collect_source_files(root: &Path, lightweight: bool) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_source_files_recursive(root, &mut files, lightweight)?;
    files.sort();
    Ok(files)
}

fn collect_source_files_recursive(
    root: &Path,
    files: &mut Vec<PathBuf>,
    lightweight: bool,
) -> io::Result<()> {
    let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_source_files_recursive(&path, files, lightweight)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "mel" || (lightweight && ext == "ma"))
        {
            files.push(path);
        }
    }

    Ok(())
}

fn print_parse_summary(label: &str, parse: &Parse) {
    let diagnostics = filtered_parse_diagnostics(parse, CliDiagnosticLevel::All);
    print!(
        "{}",
        format_parse_summary(label, parse, diagnostic_counts(&diagnostics))
    );
}

fn format_parse_summary(label: &str, parse: &Parse, counts: DiagnosticCounts) -> String {
    let mut output = String::new();
    append_parse_summary(&mut output, label, parse, counts)
        .expect("formatting parse summary should not fail");
    output
}

fn append_parse_summary(
    output: &mut String,
    label: &str,
    parse: &Parse,
    counts: DiagnosticCounts,
) -> std::fmt::Result {
    write!(
        output,
        "source: {label}\nencoding: {}\nitems: {}\ndecode diagnostics: {}\nlexical diagnostics: {}\nparse errors: {}\nsemantic diagnostics: {}\n",
        parse.source_encoding.label(),
        parse.syntax.items.len(),
        counts.decode,
        counts.lex,
        counts.parse,
        counts.sema
    )
}

#[cfg(test)]
fn format_single_file_output(
    label: &str,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
) -> io::Result<String> {
    format_single_file_output_with_style(label, parse, diagnostic_level, true)
}

#[cfg(test)]
fn format_single_file_output_with_style(
    label: &str,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
    fancy_diagnostics: bool,
) -> io::Result<String> {
    let mut output = Vec::new();
    write_single_file_output_with_style(
        &mut output,
        label,
        parse,
        diagnostic_level,
        fancy_diagnostics,
    )?;
    String::from_utf8(output).map_err(io::Error::other)
}

fn write_single_file_output_with_style(
    mut writer: impl Write,
    label: &str,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
    fancy_diagnostics: bool,
) -> io::Result<()> {
    if fancy_diagnostics {
        let diagnostics = filtered_parse_diagnostics(parse, diagnostic_level);
        let mut output = format_parse_summary(label, parse, diagnostic_counts(&diagnostics));
        writer.write_all(output.as_bytes())?;
        output.clear();
        render_file_diagnostics_into(
            &mut writer,
            label,
            parse.source_text.as_str(),
            &parse.source_map,
            &diagnostics,
            true,
        )
    } else {
        write_single_file_output(writer, label, parse, diagnostic_level)
    }
}

fn write_single_file_output(
    mut writer: impl Write,
    label: &str,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
) -> io::Result<()> {
    let sema_diagnostics = filtered_sema_diagnostics(parse, diagnostic_level);
    let counts = parse_diagnostic_counts(parse, diagnostic_level, &sema_diagnostics);
    let mut output = String::new();
    append_parse_summary(&mut output, label, parse, counts).expect("summary append");
    append_compact_parse_diagnostics(&mut output, parse, diagnostic_level, &sema_diagnostics);
    writer.write_all(output.as_bytes())
}

#[cfg(test)]
fn format_light_single_file_output(
    label: &str,
    parse: &LightParse,
    diagnostic_level: CliDiagnosticLevel,
) -> io::Result<String> {
    format_light_single_file_output_with_style(label, parse, diagnostic_level, true)
}

#[cfg(test)]
fn format_light_single_file_output_with_style(
    label: &str,
    parse: &LightParse,
    diagnostic_level: CliDiagnosticLevel,
    fancy_diagnostics: bool,
) -> io::Result<String> {
    let mut output = Vec::new();
    write_light_single_file_output_with_style(
        &mut output,
        label,
        parse,
        diagnostic_level,
        fancy_diagnostics,
    )?;
    String::from_utf8(output).map_err(io::Error::other)
}

fn write_light_single_file_output_with_style(
    mut writer: impl Write,
    label: &str,
    parse: &LightParse,
    diagnostic_level: CliDiagnosticLevel,
    fancy_diagnostics: bool,
) -> io::Result<()> {
    if fancy_diagnostics {
        let diagnostics = filtered_light_diagnostics(parse, diagnostic_level);
        let summary = light_file_summary(Path::new(label), parse, diagnostic_counts(&diagnostics));
        let mut output = String::new();
        append_light_summary(&mut output, label, parse, &summary).expect("light summary append");
        writer.write_all(output.as_bytes())?;
        output.clear();
        render_file_diagnostics_into(
            &mut writer,
            label,
            parse.source_text.as_str(),
            &parse.source_map,
            &diagnostics,
            true,
        )
    } else {
        write_light_single_file_output(writer, label, parse, diagnostic_level)
    }
}

fn write_light_single_file_output(
    mut writer: impl Write,
    label: &str,
    parse: &LightParse,
    diagnostic_level: CliDiagnosticLevel,
) -> io::Result<()> {
    let diagnostics = filtered_light_diagnostics(parse, diagnostic_level);
    let summary = light_file_summary(Path::new(label), parse, diagnostic_counts(&diagnostics));
    let mut output = String::new();
    append_light_summary(&mut output, label, parse, &summary).expect("light summary append");
    append_compact_file_diagnostics(
        &mut output,
        parse.source_text.as_str(),
        &parse.source_map,
        &diagnostics,
    );
    writer.write_all(output.as_bytes())
}

fn append_light_summary(
    output: &mut String,
    label: &str,
    parse: &LightParse,
    summary: &LightFileSummary,
) -> std::fmt::Result {
    write!(
        output,
        "source: {label}\nmode: lightweight\nencoding: {}\nitems: {}\ncommand items: {}\nproc items: {}\nother items: {}\nopaque-tail commands: {}\nlight specialized setAttr: {}\nsetAttr with opaque tail: {}\ndecode diagnostics: {}\nlight parse errors: {}\n",
        parse.source_encoding.label(),
        summary.items,
        summary.command_items,
        summary.proc_items,
        summary.other_items,
        summary.opaque_tail_commands,
        summary.specialized_set_attr,
        summary.set_attr_with_opaque_tail,
        summary.decode_errors,
        summary.light_parse_errors,
    )
}

#[derive(Debug, Clone)]
enum FileDiagnosticText<'a> {
    Borrowed(&'a str),
    Shared(Arc<str>),
}

impl FileDiagnosticText<'_> {
    fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(text) => text,
            Self::Shared(text) => text.as_ref(),
        }
    }
}

#[derive(Debug, Clone)]
struct FileDiagnostic<'a> {
    stage: &'static str,
    severity: DiagnosticSeverity,
    message: FileDiagnosticText<'a>,
    labels: Vec<FileDiagnosticLabel<'a>>,
}

#[derive(Clone, Debug)]
struct FileDiagnosticLabel<'a> {
    range: TextRange,
    message: Option<FileDiagnosticText<'a>>,
    is_primary: bool,
}

type IsolatedDiagnosticSpan<'a> = (std::ops::Range<usize>, &'a str, bool);

#[derive(Clone, Copy)]
struct BorrowedDiagnosticLabel<'a> {
    range: TextRange,
    message: &'a str,
    is_primary: bool,
}

struct CompactDiagnosticContext<'a> {
    source_text: &'a str,
    source_map: &'a SourceMap,
    line_starts: &'a [usize],
}

fn render_file_diagnostics_into(
    mut writer: impl Write,
    label: &str,
    source_text: &str,
    source_map: &mel_syntax::SourceMap,
    diagnostics: &[FileDiagnostic<'_>],
    fancy_diagnostics: bool,
) -> io::Result<()> {
    if diagnostics.is_empty() {
        return Ok(());
    }

    if !fancy_diagnostics {
        return writer.write_all(
            render_compact_file_diagnostics(source_text, source_map, diagnostics).as_bytes(),
        );
    }

    let (display_text, display_map) = normalize_diagnostic_source_text(source_text);
    let display_line_starts = compute_display_line_starts(display_text.as_str());
    let mut rendered = Vec::new();
    let mut display_labels = Vec::new();
    let mut isolated_input = Vec::new();
    let mut isolated_labels = Vec::new();
    let mut isolated_text = String::new();
    let mut report_message = String::new();
    for diagnostic in diagnostics {
        display_labels.clear();
        isolated_input.clear();
        isolated_labels.clear();
        isolated_text.clear();

        for label in &diagnostic.labels {
            let source_span = source_map.display_range(label.range);
            let range = display_map
                .display_range(text_range(source_span.start as u32, source_span.end as u32));
            let message = label
                .message
                .as_ref()
                .map(FileDiagnosticText::as_str)
                .unwrap_or("");
            let mapped = BorrowedDiagnosticLabel {
                range: text_range(range.start as u32, range.end as u32),
                message,
                is_primary: label.is_primary,
            };
            display_labels.push(mapped);
            isolated_input.push((
                range_start(mapped.range) as usize..range_end(mapped.range) as usize,
                mapped.message,
                mapped.is_primary,
            ));
        }

        isolate_diagnostic_source_lines_into(
            display_text.as_str(),
            &display_line_starts,
            &isolated_input,
            &mut isolated_text,
            &mut isolated_labels,
        );
        let primary_range = isolated_labels
            .iter()
            .find(|(_, _, is_primary)| *is_primary)
            .map(|(range, _, _)| range.clone())
            .unwrap_or_else(|| isolated_labels[0].0.clone());
        report_message.clear();
        write!(
            &mut report_message,
            "{}: {}",
            diagnostic.stage,
            diagnostic.message.as_str()
        )
        .expect("diagnostic message append");
        let mut report = Report::build(report_kind(diagnostic.severity), (label, primary_range))
            .with_config(Config::default().with_tab_width(DIAGNOSTIC_TAB_WIDTH))
            .with_message(std::mem::take(&mut report_message));
        for (range, message, is_primary) in &isolated_labels {
            let color = if *is_primary {
                stage_color(diagnostic.stage, diagnostic.severity)
            } else {
                Color::Cyan
            };
            let message = if message.is_empty() {
                diagnostic.message.as_str()
            } else {
                *message
            };
            report = report.with_label(
                Label::new((label, range.clone()))
                    .with_message(message)
                    .with_color(color),
            );
        }
        report
            .finish()
            .write((label, Source::from(isolated_text.as_str())), &mut rendered)
            .map_err(io::Error::other)?;
    }

    writer.write_all(&rendered)
}

fn render_compact_file_diagnostics(
    source_text: &str,
    source_map: &SourceMap,
    diagnostics: &[FileDiagnostic<'_>],
) -> String {
    let mut rendered = String::new();
    append_compact_file_diagnostics(&mut rendered, source_text, source_map, diagnostics);
    rendered
}

fn append_compact_file_diagnostics(
    output: &mut String,
    source_text: &str,
    source_map: &SourceMap,
    diagnostics: &[FileDiagnostic<'_>],
) {
    if diagnostics.is_empty() {
        return;
    }

    let line_starts = compute_normalized_line_starts(source_text);

    for diagnostic in diagnostics {
        let primary_range = diagnostic
            .labels
            .iter()
            .find(|label| label.is_primary)
            .or_else(|| diagnostic.labels.first())
            .map(|label| {
                let source_span = source_map.display_range(label.range);
                source_span.start
            });
        let severity = match diagnostic.severity {
            DiagnosticSeverity::Error => "Error",
            DiagnosticSeverity::Warning => "Warning",
        };
        write!(
            output,
            "{severity}: {}: {}",
            diagnostic.stage,
            diagnostic.message.as_str()
        )
        .expect("compact diagnostic append");
        if let Some(offset) = primary_range {
            let (line, column) = normalized_line_col_for_offset(source_text, &line_starts, offset);
            write!(output, " @ {}:{}", line + 1, column + 1)
                .expect("compact diagnostic location append");
        }
        output.push('\n');
    }
}

fn append_compact_parse_diagnostics(
    output: &mut String,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
    sema_diagnostics: &[mel_sema::Diagnostic],
) {
    if matches!(diagnostic_level, CliDiagnosticLevel::None) {
        return;
    }

    let line_starts = compute_normalized_line_starts(parse.source_text.as_str());
    let context = CompactDiagnosticContext {
        source_text: parse.source_text.as_str(),
        source_map: &parse.source_map,
        line_starts: &line_starts,
    };
    for diagnostic in &parse.decode_errors {
        write_compact_diagnostic_line(
            output,
            "decode",
            DiagnosticSeverity::Error,
            diagnostic.message.as_ref(),
            Some(diagnostic.range),
            &context,
        );
    }
    for diagnostic in &parse.lex_errors {
        write_compact_diagnostic_line(
            output,
            "lex",
            DiagnosticSeverity::Error,
            diagnostic.message,
            Some(diagnostic.range),
            &context,
        );
    }
    for diagnostic in &parse.errors {
        write_compact_diagnostic_line(
            output,
            "parse",
            DiagnosticSeverity::Error,
            diagnostic.message,
            Some(diagnostic.range),
            &context,
        );
    }
    for diagnostic in sema_diagnostics {
        let primary_range = diagnostic
            .labels
            .iter()
            .find(|label| label.is_primary)
            .or_else(|| diagnostic.labels.first())
            .map(|label| label.range);
        write_compact_diagnostic_line(
            output,
            "sema",
            diagnostic.severity,
            diagnostic.message.as_ref(),
            primary_range,
            &context,
        );
    }
}

fn write_compact_diagnostic_line(
    output: &mut String,
    stage: &str,
    severity: DiagnosticSeverity,
    message: &str,
    range: Option<TextRange>,
    context: &CompactDiagnosticContext<'_>,
) {
    let severity = match severity {
        DiagnosticSeverity::Error => "Error",
        DiagnosticSeverity::Warning => "Warning",
    };
    write!(output, "{severity}: {stage}: {message}").expect("compact diagnostic append");
    if let Some(range) = range {
        let source_span = context.source_map.display_range(range);
        let (line, column) = normalized_line_col_for_offset(
            context.source_text,
            context.line_starts,
            source_span.start,
        );
        write!(output, " @ {}:{}", line + 1, column + 1)
            .expect("compact diagnostic location append");
    }
    output.push('\n');
}

fn compute_normalized_line_starts(source_text: &str) -> Vec<usize> {
    let bytes = source_text.as_bytes();
    let mut starts = vec![0];
    let mut offset = 0usize;
    while offset < bytes.len() {
        match bytes[offset] {
            b'\r' if bytes.get(offset + 1) == Some(&b'\n') => {
                starts.push(offset + 2);
                offset += 2;
            }
            b'\n' | b'\r' => {
                starts.push(offset + 1);
                offset += 1;
            }
            _ => offset += 1,
        }
    }
    starts
}

fn normalized_line_col_for_offset(
    source_text: &str,
    line_starts: &[usize],
    offset: usize,
) -> (usize, usize) {
    let line = match line_starts.binary_search(&offset) {
        Ok(index) => index,
        Err(next_index) => next_index.saturating_sub(1),
    };
    let line_start = line_starts[line];
    let column = normalized_column_for_source_offset(source_text, line_start, offset);
    (line, column)
}

fn normalized_column_for_source_offset(source_text: &str, start: usize, offset: usize) -> usize {
    let bytes = source_text.as_bytes();
    let mut source_offset = start.min(bytes.len());
    let target = offset.min(bytes.len());
    let mut display_offset = 0usize;

    while source_offset < target {
        match bytes[source_offset] {
            b'\r' if bytes.get(source_offset + 1) == Some(&b'\n') => {
                if source_offset + 1 >= target {
                    break;
                }
                display_offset += 1;
                source_offset += 2;
            }
            _ => {
                display_offset += 1;
                source_offset += 1;
            }
        }
    }

    display_offset
}

fn normalize_diagnostic_source_text(source_text: &str) -> (String, SourceMap) {
    let bytes = source_text.as_bytes();
    let mut display = Vec::with_capacity(bytes.len());
    let mut source_to_display = vec![0u32; bytes.len() + 1];
    let mut source_offset = 0usize;
    let mut display_offset = 0u32;

    while source_offset < bytes.len() {
        source_to_display[source_offset] = display_offset;
        match bytes[source_offset] {
            b'\r' if bytes.get(source_offset + 1) == Some(&b'\n') => {
                source_to_display[source_offset + 1] = display_offset;
                display.push(b'\n');
                display_offset += 1;
                source_offset += 2;
            }
            b'\t' => {
                display.push(b' ');
                display_offset += 1;
                source_offset += 1;
            }
            b'\r' => {
                display.push(b'\n');
                display_offset += 1;
                source_offset += 1;
            }
            byte => {
                display.push(byte);
                display_offset += 1;
                source_offset += 1;
            }
        }
    }

    source_to_display[bytes.len()] = display_offset;
    (
        String::from_utf8(display).expect("normalized diagnostic source should remain utf-8"),
        SourceMap::from_source_to_display(source_to_display),
    )
}

fn compute_display_line_starts(source_text: &str) -> Vec<usize> {
    let bytes = source_text.as_bytes();
    let mut starts = vec![0];
    for (offset, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            starts.push(offset + 1);
        }
    }
    starts
}

fn isolate_diagnostic_source_lines_into<'a>(
    source_text: &str,
    line_starts: &[usize],
    spans: &[IsolatedDiagnosticSpan<'a>],
    isolated: &mut String,
    isolated_spans: &mut Vec<IsolatedDiagnosticSpan<'a>>,
) {
    let line_start = spans
        .iter()
        .map(|(span, _, _)| {
            source_text[..span.start]
                .rfind('\n')
                .map_or(0, |index| index + 1)
        })
        .min()
        .unwrap_or(0);
    let line_end = spans
        .iter()
        .map(|(span, _, _)| {
            source_text[span.end..]
                .find('\n')
                .map_or(source_text.len(), |index| span.end + index)
        })
        .max()
        .unwrap_or(source_text.len());
    let preceding_lines = match line_starts.binary_search(&line_start) {
        Ok(index) => index,
        Err(next_index) => next_index.saturating_sub(1),
    };
    isolated.clear();
    isolated.reserve(preceding_lines + (line_end - line_start));
    for _ in 0..preceding_lines {
        isolated.push('\n');
    }
    isolated.push_str(&source_text[line_start..line_end]);
    let prefix_len = preceding_lines;
    isolated_spans.clear();
    isolated_spans.extend(spans.iter().map(|(span, message, is_primary)| {
        (
            (prefix_len + span.start - line_start)..(prefix_len + span.end - line_start),
            *message,
            *is_primary,
        )
    }));
}

fn collect_diagnostics(parse: &Parse, filter: DiagnosticFilter) -> Vec<FileDiagnostic<'_>> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(parse.decode_errors.iter().map(|diagnostic| FileDiagnostic {
        stage: "decode",
        severity: DiagnosticSeverity::Error,
        message: FileDiagnosticText::Borrowed(diagnostic.message.as_ref()),
        labels: vec![FileDiagnosticLabel {
            range: diagnostic.range,
            message: None,
            is_primary: true,
        }],
    }));
    diagnostics.extend(parse.lex_errors.iter().map(|diagnostic| FileDiagnostic {
        stage: "lex",
        severity: DiagnosticSeverity::Error,
        message: FileDiagnosticText::Borrowed(diagnostic.message),
        labels: vec![FileDiagnosticLabel {
            range: diagnostic.range,
            message: None,
            is_primary: true,
        }],
    }));
    diagnostics.extend(parse.errors.iter().map(|diagnostic| FileDiagnostic {
        stage: "parse",
        severity: DiagnosticSeverity::Error,
        message: FileDiagnosticText::Borrowed(diagnostic.message),
        labels: vec![FileDiagnosticLabel {
            range: diagnostic.range,
            message: None,
            is_primary: true,
        }],
    }));
    diagnostics.extend(
        analyze_parse_diagnostics(parse, filter)
            .into_iter()
            .map(|diagnostic| FileDiagnostic {
                stage: "sema",
                severity: diagnostic.severity,
                message: FileDiagnosticText::Shared(diagnostic.message),
                labels: diagnostic
                    .labels
                    .into_iter()
                    .map(file_diagnostic_label)
                    .collect(),
            }),
    );
    diagnostics
}

fn filtered_sema_diagnostics(
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
) -> Vec<mel_sema::Diagnostic> {
    match diagnostic_level {
        CliDiagnosticLevel::All => analyze_parse_diagnostics(parse, DiagnosticFilter::All),
        CliDiagnosticLevel::Error => analyze_parse_diagnostics(parse, DiagnosticFilter::ErrorsOnly),
        CliDiagnosticLevel::None => Vec::new(),
    }
}

fn parse_diagnostic_counts(
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
    sema_diagnostics: &[mel_sema::Diagnostic],
) -> DiagnosticCounts {
    match diagnostic_level {
        CliDiagnosticLevel::None => DiagnosticCounts::default(),
        CliDiagnosticLevel::All | CliDiagnosticLevel::Error => DiagnosticCounts {
            decode: parse.decode_errors.len(),
            lex: parse.lex_errors.len(),
            parse: parse.errors.len(),
            sema: sema_diagnostics.len(),
            light: 0,
        },
    }
}

fn collect_light_diagnostics(parse: &LightParse) -> Vec<FileDiagnostic<'_>> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(parse.decode_errors.iter().map(|diagnostic| FileDiagnostic {
        stage: "decode",
        severity: DiagnosticSeverity::Error,
        message: FileDiagnosticText::Borrowed(diagnostic.message.as_ref()),
        labels: vec![FileDiagnosticLabel {
            range: diagnostic.range,
            message: None,
            is_primary: true,
        }],
    }));
    diagnostics.extend(parse.errors.iter().map(|diagnostic| FileDiagnostic {
        stage: "light",
        severity: DiagnosticSeverity::Error,
        message: FileDiagnosticText::Borrowed(diagnostic.message),
        labels: vec![FileDiagnosticLabel {
            range: diagnostic.range,
            message: None,
            is_primary: true,
        }],
    }));
    diagnostics
}

fn filtered_parse_diagnostics(
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
) -> Vec<FileDiagnostic<'_>> {
    match diagnostic_level {
        CliDiagnosticLevel::All => collect_diagnostics(parse, DiagnosticFilter::All),
        CliDiagnosticLevel::Error => filter_diagnostics(
            collect_diagnostics(parse, DiagnosticFilter::ErrorsOnly),
            diagnostic_level,
        ),
        CliDiagnosticLevel::None => Vec::new(),
    }
}

fn filtered_light_diagnostics(
    parse: &LightParse,
    diagnostic_level: CliDiagnosticLevel,
) -> Vec<FileDiagnostic<'_>> {
    match diagnostic_level {
        CliDiagnosticLevel::None => Vec::new(),
        _ => filter_diagnostics(collect_light_diagnostics(parse), diagnostic_level),
    }
}

fn filter_diagnostics(
    diagnostics: Vec<FileDiagnostic<'_>>,
    diagnostic_level: CliDiagnosticLevel,
) -> Vec<FileDiagnostic<'_>> {
    match diagnostic_level {
        CliDiagnosticLevel::All => diagnostics,
        CliDiagnosticLevel::Error => diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            .collect(),
        CliDiagnosticLevel::None => Vec::new(),
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct DiagnosticCounts {
    decode: usize,
    lex: usize,
    parse: usize,
    sema: usize,
    light: usize,
}

fn diagnostic_counts(diagnostics: &[FileDiagnostic<'_>]) -> DiagnosticCounts {
    let mut counts = DiagnosticCounts::default();
    for diagnostic in diagnostics {
        match diagnostic.stage {
            "decode" => counts.decode += 1,
            "lex" => counts.lex += 1,
            "parse" => counts.parse += 1,
            "sema" => counts.sema += 1,
            "light" => counts.light += 1,
            _ => {}
        }
    }
    counts
}

fn file_diagnostic_label(label: DiagnosticLabel) -> FileDiagnosticLabel<'static> {
    FileDiagnosticLabel {
        range: label.range,
        message: Some(FileDiagnosticText::Shared(label.message)),
        is_primary: label.is_primary,
    }
}

fn analyze_parse_diagnostics(parse: &Parse, filter: DiagnosticFilter) -> Vec<mel_sema::Diagnostic> {
    match filter {
        DiagnosticFilter::All => analyze_diagnostics_with_registry(
            &parse.syntax,
            parse.source_view(),
            &MayaCommandRegistry::new(),
        ),
        DiagnosticFilter::ErrorsOnly => analyze_diagnostics_with_registry_filtered(
            &parse.syntax,
            parse.source_view(),
            &MayaCommandRegistry::new(),
            DiagnosticFilter::ErrorsOnly,
        ),
    }
}

fn summarize_parse_file(
    path: &Path,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
) -> FileSummary {
    let diagnostics = filtered_parse_diagnostics(parse, diagnostic_level);
    let counts = diagnostic_counts(&diagnostics);
    FileSummary {
        path: path.display().to_string(),
        decode_errors: counts.decode,
        lex_errors: counts.lex,
        parse_errors: counts.parse,
        parse_error_messages: diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.stage == "parse")
            .map(|diagnostic| diagnostic.message.as_str().to_owned())
            .collect(),
        semantic_diagnostics: counts.sema,
    }
}

fn report_kind(severity: DiagnosticSeverity) -> ReportKind<'static> {
    match severity {
        DiagnosticSeverity::Error => ReportKind::Error,
        DiagnosticSeverity::Warning => ReportKind::Warning,
    }
}

fn stage_color(stage: &str, severity: DiagnosticSeverity) -> Color {
    if matches!(severity, DiagnosticSeverity::Warning) {
        return Color::Yellow;
    }

    match stage {
        "decode" => Color::Yellow,
        "lex" => Color::Blue,
        "parse" => Color::Red,
        "sema" => Color::Magenta,
        "light" => Color::Cyan,
        _ => Color::White,
    }
}

#[derive(Debug, Default)]
struct CorpusSummary {
    files: usize,
    files_with_decode_issues: usize,
    files_with_lex_errors: usize,
    files_with_parse_errors: usize,
    files_with_semantic_diagnostics: usize,
    io_errors: usize,
    samples: Vec<String>,
    parse_error_files: Vec<(String, usize)>,
    parse_error_message_counts: HashMap<String, usize>,
}

impl CorpusSummary {
    fn record(&mut self, file: FileSummary) {
        let path = file.path.clone();
        if file.decode_errors > 0 {
            self.files_with_decode_issues += 1;
        }
        if file.lex_errors > 0 {
            self.files_with_lex_errors += 1;
        }
        if file.parse_errors > 0 {
            self.files_with_parse_errors += 1;
            self.parse_error_files
                .push((path.clone(), file.parse_errors));
        }
        if file.semantic_diagnostics > 0 {
            self.files_with_semantic_diagnostics += 1;
        }

        for message in &file.parse_error_messages {
            *self
                .parse_error_message_counts
                .entry(message.clone())
                .or_insert(0) += 1;
        }

        if self.samples.len() < 10
            && (file.decode_errors > 0
                || file.lex_errors > 0
                || file.parse_errors > 0
                || file.semantic_diagnostics > 0)
        {
            self.samples.push(format!(
                "{} decode={} lex={} parse={} sema={}",
                path,
                file.decode_errors,
                file.lex_errors,
                file.parse_errors,
                file.semantic_diagnostics
            ));
        }
    }

    fn top_parse_error_files(&self) -> Vec<(String, usize)> {
        let mut ranked = self.parse_error_files.clone();
        ranked.sort_by(|lhs, rhs| rhs.1.cmp(&lhs.1).then_with(|| lhs.0.cmp(&rhs.0)));
        ranked.truncate(TOP_RANK_LIMIT);
        ranked
    }

    fn top_parse_error_messages(&self) -> Vec<(String, usize)> {
        let mut ranked: Vec<_> = self
            .parse_error_message_counts
            .iter()
            .map(|(message, count)| (message.clone(), *count))
            .collect();
        ranked.sort_by(|lhs, rhs| rhs.1.cmp(&lhs.1).then_with(|| lhs.0.cmp(&rhs.0)));
        ranked.truncate(TOP_RANK_LIMIT);
        ranked
    }
}

#[derive(Debug)]
struct FileSummary {
    path: String,
    decode_errors: usize,
    lex_errors: usize,
    parse_errors: usize,
    parse_error_messages: Vec<String>,
    semantic_diagnostics: usize,
}

#[derive(Debug, Default)]
struct LightCorpusSummary {
    files: usize,
    files_with_decode_issues: usize,
    files_with_light_parse_errors: usize,
    io_errors: usize,
    total_items: usize,
    total_command_items: usize,
    total_proc_items: usize,
    total_opaque_tail_commands: usize,
    total_specialized_set_attr: usize,
    total_set_attr_with_opaque_tail: usize,
    samples: Vec<String>,
}

impl LightCorpusSummary {
    fn record(&mut self, file: LightFileSummary) {
        if file.decode_errors > 0 {
            self.files_with_decode_issues += 1;
        }
        if file.light_parse_errors > 0 {
            self.files_with_light_parse_errors += 1;
        }
        self.total_items += file.items;
        self.total_command_items += file.command_items;
        self.total_proc_items += file.proc_items;
        self.total_opaque_tail_commands += file.opaque_tail_commands;
        self.total_specialized_set_attr += file.specialized_set_attr;
        self.total_set_attr_with_opaque_tail += file.set_attr_with_opaque_tail;

        if self.samples.len() < 10 && (file.decode_errors > 0 || file.light_parse_errors > 0) {
            self.samples.push(format!(
                "{} decode={} light={} commands={} opaque_tail={}",
                file.path,
                file.decode_errors,
                file.light_parse_errors,
                file.command_items,
                file.opaque_tail_commands
            ));
        }
    }
}

#[derive(Debug)]
struct LightFileSummary {
    path: String,
    decode_errors: usize,
    light_parse_errors: usize,
    items: usize,
    command_items: usize,
    proc_items: usize,
    other_items: usize,
    opaque_tail_commands: usize,
    specialized_set_attr: usize,
    set_attr_with_opaque_tail: usize,
}

fn light_file_summary(
    path: &Path,
    parse: &LightParse,
    counts: DiagnosticCounts,
) -> LightFileSummary {
    let facts = collect_top_level_facts_light(parse);
    let mut command_items = 0;
    let mut proc_items = 0;
    let mut other_items = 0;
    let mut opaque_tail_commands = 0;
    let mut specialized_set_attr = 0;
    let mut set_attr_with_opaque_tail = 0;

    for item in &facts.items {
        match item {
            MayaLightTopLevelItem::Command(command) => {
                command_items += 1;
                if command.opaque_tail.is_some() {
                    opaque_tail_commands += 1;
                }
                if let Some(MayaLightSpecializedCommand::SetAttr(set_attr)) =
                    command.specialized.as_ref()
                {
                    specialized_set_attr += 1;
                    if set_attr.opaque_tail.is_some() {
                        set_attr_with_opaque_tail += 1;
                    }
                }
            }
            MayaLightTopLevelItem::Proc { .. } => proc_items += 1,
            MayaLightTopLevelItem::Other { .. } => other_items += 1,
        }
    }

    LightFileSummary {
        path: path.display().to_string(),
        decode_errors: counts.decode,
        light_parse_errors: counts.light,
        items: facts.items.len(),
        command_items,
        proc_items,
        other_items,
        opaque_tail_commands,
        specialized_set_attr,
        set_attr_with_opaque_tail,
    }
}

fn format_light_corpus_summary(summary: &LightCorpusSummary) -> String {
    let mut output = format!(
        "corpus files: {}\nfiles with decode issues: {}\nfiles with light parse errors: {}\ntotal items: {}\ntotal command items: {}\ntotal proc items: {}\ntotal opaque-tail commands: {}\ntotal light-specialized setAttr: {}\ntotal setAttr with opaque tail: {}\nio errors: {}\n",
        summary.files,
        summary.files_with_decode_issues,
        summary.files_with_light_parse_errors,
        summary.total_items,
        summary.total_command_items,
        summary.total_proc_items,
        summary.total_opaque_tail_commands,
        summary.total_specialized_set_attr,
        summary.total_set_attr_with_opaque_tail,
        summary.io_errors,
    );

    if !summary.samples.is_empty() {
        output.push_str("sample issues:\n");
        for sample in summary.samples.iter().take(10) {
            output.push_str("  ");
            output.push_str(sample);
            output.push('\n');
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{
        Args, CliDiagnosticLevel, CorpusSummary, FileSummary, LightCorpusSummary, LightFileSummary,
        format_light_corpus_summary, format_light_single_file_output, format_single_file_output,
        format_single_file_output_with_style, parse_cli_args, print_path_output,
    };
    use clap::{CommandFactory, error::ErrorKind};
    use mel_parser::{
        LightParseOptions, ParseMode, ParseOptions, SourceEncoding, parse_bytes_with_encoding,
        parse_light_source_with_options, parse_source, parse_source_with_options,
    };
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn render_snapshot(label: &str, source: &str) -> String {
        format_single_file_output(label, &parse_source(source), CliDiagnosticLevel::All)
            .expect("snapshot should render")
    }

    #[test]
    fn normalize_diagnostic_source_text_collapses_crlf_offsets() {
        let (display, map) = super::normalize_diagnostic_source_text("a\t\r\nb\r\n");
        assert_eq!(display, "a \nb\n");
        assert_eq!(map.display_offset(0), 0);
        assert_eq!(map.display_offset(1), 1);
        assert_eq!(map.display_offset(2), 2);
        assert_eq!(map.display_offset(3), 2);
        assert_eq!(map.display_offset(4), 3);
        assert_eq!(map.display_offset(5), 4);
        assert_eq!(map.display_offset(6), 4);
        assert_eq!(map.display_offset(7), 5);
    }

    #[test]
    fn compute_display_line_starts_tracks_normalized_lines() {
        let starts = super::compute_display_line_starts("first\nsecond\nthird");
        assert_eq!(starts, vec![0, 6, 13]);
    }

    #[test]
    fn normalized_line_col_matches_compact_display_rules() {
        let source = "a\t\r\nbc\r\ndef\n";
        let starts = super::compute_normalized_line_starts(source);
        assert_eq!(starts, vec![0, 4, 8, 12]);
        assert_eq!(
            super::normalized_line_col_for_offset(source, &starts, 0),
            (0, 0)
        );
        assert_eq!(
            super::normalized_line_col_for_offset(source, &starts, 1),
            (0, 1)
        );
        assert_eq!(
            super::normalized_line_col_for_offset(source, &starts, 2),
            (0, 2)
        );
        assert_eq!(
            super::normalized_line_col_for_offset(source, &starts, 3),
            (0, 2)
        );
        assert_eq!(
            super::normalized_line_col_for_offset(source, &starts, 4),
            (1, 0)
        );
        assert_eq!(
            super::normalized_line_col_for_offset(source, &starts, 6),
            (1, 2)
        );
        assert_eq!(
            super::normalized_line_col_for_offset(source, &starts, 7),
            (1, 2)
        );
        assert_eq!(
            super::normalized_line_col_for_offset(source, &starts, 8),
            (2, 0)
        );
    }

    #[test]
    fn format_single_file_output_handles_gbk_source_without_panicking() {
        let parse = parse_bytes_with_encoding(b"print \"\xB0\xB4\xC5\xA5\";", SourceEncoding::Gbk);
        let output = format_single_file_output("gbk-fixture", &parse, CliDiagnosticLevel::All)
            .expect("gbk output should render");
        assert!(output.contains("encoding: gbk"));
    }

    #[test]
    fn format_light_single_file_output_handles_gbk_source_without_panicking() {
        let parse = mel_parser::parse_light_bytes_with_encoding(
            b"setAttr \".\xB0\xB4\" -type \"string\" \"\xC5\xA5\";",
            SourceEncoding::Gbk,
        );
        let output =
            format_light_single_file_output("gbk-fixture", &parse, CliDiagnosticLevel::All)
                .expect("light gbk output should render");
        assert!(output.contains("mode: lightweight"));
        assert!(output.contains("encoding: gbk"));
    }

    #[test]
    fn inline_mode_accepts_single_trailing_statement_without_semicolon() {
        let parse = parse_source_with_options(
            r#"print "hello""#,
            ParseOptions {
                mode: ParseMode::AllowTrailingStmtWithoutSemi,
            },
        );
        assert!(parse.errors.is_empty());
    }

    #[test]
    fn cli_accepts_positional_path() {
        let args = parse_cli_args(["mel-inspect", "private-corpus"]).expect("path should parse");
        assert_eq!(args.path, Some(PathBuf::from("private-corpus")));
    }

    #[test]
    fn cli_accepts_lightweight_flag() {
        let args =
            parse_cli_args(["mel-inspect", "--lightweight", "private-corpus"]).expect("light");
        assert!(args.lightweight);
    }

    #[test]
    fn cli_accepts_inline_flag() {
        let args = parse_cli_args(["mel-inspect", "--inline", r#"print "hello""#])
            .expect("inline should parse");
        assert_eq!(args.inline_input.as_deref(), Some(r#"print "hello""#));
    }

    #[test]
    fn cli_accepts_diagnostic_level_flag() {
        let args = parse_cli_args(["mel-inspect", "--diagnostic-level", "error", "fixture.mel"])
            .expect("diagnostic level should parse");
        assert_eq!(args.diagnostic_level, CliDiagnosticLevel::Error);
    }

    #[test]
    fn cli_rejects_removed_file_flag() {
        let error = parse_cli_args(["mel-inspect", "--file", "a.mel"])
            .expect_err("removed file flag should fail");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn cli_rejects_removed_directory_flag() {
        let error = parse_cli_args(["mel-inspect", "--directory", "private-corpus"])
            .expect_err("removed directory flag should fail");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn cli_rejects_removed_path_flag() {
        let error = parse_cli_args(["mel-inspect", "--path", "private-corpus"])
            .expect_err("removed path flag should fail");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn cli_rejects_conflicting_input_modes() {
        let error = parse_cli_args([
            "mel-inspect",
            "private-corpus",
            "--inline",
            r#"print "hello""#,
        ])
        .expect_err("conflicting modes should fail");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn cli_rejects_lightweight_with_inline() {
        let error = parse_cli_args(["mel-inspect", "--lightweight", "--inline", "print 1"])
            .expect_err("lightweight inline should fail");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn cli_rejects_invalid_encoding() {
        let error = parse_cli_args([
            "mel-inspect",
            "--encoding",
            "latin1",
            "--inline",
            "`ls -sl`;",
        ])
        .expect_err("invalid encoding should fail");
        assert_eq!(error.kind(), ErrorKind::InvalidValue);
    }

    #[test]
    fn help_mentions_directory_flag_and_encoding_values() {
        let mut help = Vec::new();
        let mut command = Args::command();
        command
            .write_long_help(&mut help)
            .expect("help should render");
        let help = String::from_utf8(help).expect("help should be utf8");
        assert!(help.contains("[PATH]"));
        assert!(help.contains("--lightweight"));
        assert!(help.contains("--inline <SOURCE>"));
        assert!(help.contains("--diagnostic-level <DIAGNOSTIC_LEVEL>"));
        assert!(help.contains("[possible values: auto, utf8, cp932, gbk]"));
    }

    #[test]
    fn error_diagnostic_level_hides_warnings_and_zeroes_summary_count() {
        let output = format_single_file_output(
            "warning-fixture",
            &parse_source("global proc foo() { string $name; if ($name == \"\") { } }\nfoo();\n"),
            CliDiagnosticLevel::Error,
        )
        .expect("filtered output");
        assert!(output.contains("semantic diagnostics: 0"));
        assert!(!output.contains("Warning:"));
    }

    #[test]
    fn none_diagnostic_level_hides_all_diagnostic_output() {
        let output = format_single_file_output(
            "parse-fixture",
            &parse_source("print(\n"),
            CliDiagnosticLevel::None,
        )
        .expect("filtered output");
        assert!(output.contains("decode diagnostics: 0"));
        assert!(output.contains("lexical diagnostics: 0"));
        assert!(output.contains("parse errors: 0"));
        assert!(output.contains("semantic diagnostics: 0"));
        assert!(!output.contains("Error:"));
        assert!(!output.contains("Warning:"));
    }

    #[test]
    fn error_diagnostic_level_keeps_semantic_error_count() {
        let output = format_single_file_output(
            "sema-fixture",
            &parse_source("addAttr;\n"),
            CliDiagnosticLevel::Error,
        )
        .expect("filtered output");
        assert!(output.contains("semantic diagnostics: 1"));
        assert!(output.contains("Error:"));
        assert!(output.contains("command \"addAttr\" expects"));
    }

    #[test]
    fn compact_output_uses_single_line_diagnostics_for_non_terminal_output() {
        let output = format_single_file_output_with_style(
            "sema-fixture",
            &parse_source("addAttr;\n"),
            CliDiagnosticLevel::Error,
            false,
        )
        .expect("compact output");
        assert!(output.contains("semantic diagnostics: 1"));
        assert!(output.contains("Error: sema: command \"addAttr\" expects"));
        assert!(output.contains("@ 1:1"));
        assert!(!output.contains("╭"));
    }

    #[test]
    fn compact_output_keeps_parse_error_locations() {
        let output = format_single_file_output_with_style(
            "parse-fixture",
            &parse_source("print(\n"),
            CliDiagnosticLevel::Error,
            false,
        )
        .expect("compact output");
        assert!(output.contains("parse errors: 3"));
        assert!(output.contains("Error: parse: expected expression as function argument"));
        assert!(output.contains("@ 2:1"));
    }

    #[test]
    fn write_single_file_output_matches_compact_formatter() {
        let parse = parse_source("addAttr;\n");
        let expected = format_single_file_output_with_style(
            "sema-fixture",
            &parse,
            CliDiagnosticLevel::Error,
            false,
        )
        .expect("compact output");
        let mut actual = Vec::new();
        super::write_single_file_output(
            &mut actual,
            "sema-fixture",
            &parse,
            CliDiagnosticLevel::Error,
        )
        .expect("write output");
        let actual = String::from_utf8(actual).expect("writer output should stay utf8");
        assert_eq!(actual, expected);
    }

    #[test]
    fn path_mode_rejects_non_file_non_directory() {
        let path = unique_test_path("socket");
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixListener;

            let _listener = UnixListener::bind(&path).expect("socket should bind");
            let error = print_path_output(&path, None, false, CliDiagnosticLevel::All)
                .expect_err("socket path should fail");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        }

        #[cfg(not(unix))]
        {
            fs::create_dir_all(path.parent().expect("temp dir should exist"))
                .expect("temp dir should exist");
            fs::write(&path, []).expect("temp file should exist");
            fs::remove_file(&path).expect("temp file should be removable");
        }

        cleanup_test_path(&path);
    }

    #[test]
    fn top_parse_error_files_are_sorted_by_count_then_path() {
        let mut summary = CorpusSummary::default();
        summary.record(FileSummary {
            path: "b.mel".to_owned(),
            decode_errors: 0,
            lex_errors: 0,
            parse_errors: 3,
            parse_error_messages: vec!["missing ;".to_owned()],
            semantic_diagnostics: 0,
        });
        summary.record(FileSummary {
            path: "a.mel".to_owned(),
            decode_errors: 0,
            lex_errors: 0,
            parse_errors: 3,
            parse_error_messages: vec!["missing ;".to_owned()],
            semantic_diagnostics: 0,
        });
        summary.record(FileSummary {
            path: "c.mel".to_owned(),
            decode_errors: 0,
            lex_errors: 0,
            parse_errors: 1,
            parse_error_messages: vec!["missing )".to_owned()],
            semantic_diagnostics: 0,
        });

        let ranked = summary.top_parse_error_files();
        assert_eq!(
            ranked,
            vec![
                ("a.mel".to_owned(), 3),
                ("b.mel".to_owned(), 3),
                ("c.mel".to_owned(), 1),
            ]
        );
    }

    #[test]
    fn top_parse_error_messages_are_aggregated_and_sorted() {
        let mut summary = CorpusSummary::default();
        summary.record(FileSummary {
            path: "a.mel".to_owned(),
            decode_errors: 0,
            lex_errors: 0,
            parse_errors: 2,
            parse_error_messages: vec!["missing ;".to_owned(), "missing )".to_owned()],
            semantic_diagnostics: 0,
        });
        summary.record(FileSummary {
            path: "b.mel".to_owned(),
            decode_errors: 0,
            lex_errors: 0,
            parse_errors: 2,
            parse_error_messages: vec!["missing ;".to_owned(), "missing ]".to_owned()],
            semantic_diagnostics: 0,
        });

        let ranked = summary.top_parse_error_messages();
        assert_eq!(
            ranked,
            vec![
                ("missing ;".to_owned(), 2),
                ("missing )".to_owned(), 1),
                ("missing ]".to_owned(), 1),
            ]
        );
    }

    #[test]
    fn light_output_reports_opaque_tail_counts() {
        let parse = parse_light_source_with_options(
            "setAttr \".pt\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n",
            LightParseOptions {
                max_prefix_words: 5,
                max_prefix_bytes: 32,
            },
        );
        let output =
            format_light_single_file_output("light-fixture", &parse, CliDiagnosticLevel::All)
                .expect("light output");
        assert!(output.contains("opaque-tail commands: 1"));
        assert!(output.contains("setAttr with opaque tail: 1"));
    }

    #[test]
    fn light_none_diagnostic_level_zeroes_rendered_counts() {
        let parse = parse_light_source_with_options(
            "setAttr \".tx\" -type;\n",
            LightParseOptions::default(),
        );
        let output =
            format_light_single_file_output("light-fixture", &parse, CliDiagnosticLevel::None)
                .expect("light output");
        assert!(output.contains("decode diagnostics: 0"));
        assert!(output.contains("light parse errors: 0"));
        assert!(!output.contains("Error:"));
    }

    #[test]
    fn collect_source_files_in_lightweight_mode_includes_ma_files() {
        let root = unique_test_path("light-corpus");
        fs::create_dir_all(&root).expect("temp dir");
        fs::write(root.join("a.mel"), "print 1;\n").expect("mel file");
        fs::write(root.join("b.ma"), "setAttr \".tx\" 1;\n").expect("ma file");
        fs::write(root.join("c.txt"), "ignore\n").expect("txt file");

        let mel_only = super::collect_source_files(&root, false).expect("mel files");
        let light_files = super::collect_source_files(&root, true).expect("light files");
        assert_eq!(mel_only.len(), 1);
        assert_eq!(light_files.len(), 2);

        cleanup_test_path(&root);
    }

    #[test]
    fn format_light_corpus_summary_reports_lightweight_counts() {
        let mut summary = LightCorpusSummary::default();
        summary.record(LightFileSummary {
            path: "a.ma".to_owned(),
            decode_errors: 1,
            light_parse_errors: 0,
            items: 10,
            command_items: 8,
            proc_items: 1,
            other_items: 1,
            opaque_tail_commands: 2,
            specialized_set_attr: 3,
            set_attr_with_opaque_tail: 2,
        });
        let output = format_light_corpus_summary(&summary);
        assert!(output.contains("files with light parse errors: 0"));
        assert!(output.contains("total opaque-tail commands: 2"));
        assert!(output.contains("total light-specialized setAttr: 3"));
    }

    #[test]
    fn snapshot_lexer_unterminated_string_fixture() {
        insta::assert_snapshot!(
            "lexer_unterminated_string",
            render_snapshot(
                "lexer/strings/unterminated-string.mel",
                include_str!("../../../tests/corpus/lexer/strings/unterminated-string.mel"),
            )
        );
    }

    #[test]
    fn snapshot_lexer_unknown_char_fixture() {
        insta::assert_snapshot!(
            "lexer_unknown_char",
            render_snapshot(
                "lexer/symbols/unknown-char.mel",
                include_str!("../../../tests/corpus/lexer/symbols/unknown-char.mel"),
            )
        );
    }

    #[test]
    fn snapshot_parser_missing_ternary_colon_fixture() {
        insta::assert_snapshot!(
            "parser_missing_ternary_colon",
            render_snapshot(
                "parser/expressions/missing-ternary-colon.mel",
                include_str!("../../../tests/corpus/parser/expressions/missing-ternary-colon.mel"),
            )
        );
    }

    #[test]
    fn snapshot_parser_missing_proc_param_name_fixture() {
        insta::assert_snapshot!(
            "parser_missing_proc_param_name",
            render_snapshot(
                "parser/proc/missing-proc-param-name.mel",
                include_str!("../../../tests/corpus/parser/proc/missing-proc-param-name.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_local_proc_forward_reference_fixture() {
        insta::assert_snapshot!(
            "sema_local_proc_forward_reference",
            render_snapshot(
                "sema/proc/local-forward-reference.mel",
                include_str!("../../../tests/corpus/sema/proc/local-forward-reference.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_local_proc_shell_unresolved_fixture() {
        insta::assert_snapshot!(
            "sema_local_proc_shell_unresolved",
            render_snapshot(
                "sema/proc/local-shell-unresolved.mel",
                include_str!("../../../tests/corpus/sema/proc/local-shell-unresolved.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_local_proc_shell_forward_reference_fixture() {
        insta::assert_snapshot!(
            "sema_local_proc_shell_forward_reference",
            render_snapshot(
                "sema/proc/local-shell-forward-reference.mel",
                include_str!("../../../tests/corpus/sema/proc/local-shell-forward-reference.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_typed_missing_value_return_fixture() {
        insta::assert_snapshot!(
            "sema_typed_missing_value_return",
            render_snapshot(
                "sema/proc/typed-missing-value-return.mel",
                include_str!("../../../tests/corpus/sema/proc/typed-missing-value-return.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_void_return_value_fixture() {
        insta::assert_snapshot!(
            "sema_void_return_value",
            render_snapshot(
                "sema/proc/void-return-value.mel",
                include_str!("../../../tests/corpus/sema/proc/void-return-value.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_typed_return_type_mismatch_fixture() {
        insta::assert_snapshot!(
            "sema_typed_return_type_mismatch",
            render_snapshot(
                "sema/proc/typed-return-type-mismatch.mel",
                include_str!("../../../tests/corpus/sema/proc/typed-return-type-mismatch.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_var_init_type_mismatch_fixture() {
        insta::assert_snapshot!(
            "sema_var_init_type_mismatch",
            render_snapshot(
                "sema/proc/var-init-type-mismatch.mel",
                include_str!("../../../tests/corpus/sema/proc/var-init-type-mismatch.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_typed_return_type_mismatch_via_call_fixture() {
        insta::assert_snapshot!(
            "sema_typed_return_type_mismatch_via_call",
            render_snapshot(
                "sema/proc/typed-return-type-mismatch-via-call.mel",
                include_str!(
                    "../../../tests/corpus/sema/proc/typed-return-type-mismatch-via-call.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_var_init_type_mismatch_via_call_fixture() {
        insta::assert_snapshot!(
            "sema_var_init_type_mismatch_via_call",
            render_snapshot(
                "sema/proc/var-init-type-mismatch-via-call.mel",
                include_str!("../../../tests/corpus/sema/proc/var-init-type-mismatch-via-call.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_read_before_write_and_shadowing_fixture() {
        insta::assert_snapshot!(
            "sema_read_before_write_and_shadowing",
            render_snapshot(
                "sema/lint/read-before-write-and-shadowing.mel",
                include_str!("../../../tests/corpus/sema/lint/read-before-write-and-shadowing.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_unresolved_variable_fixture() {
        insta::assert_snapshot!(
            "sema_unresolved_variable",
            render_snapshot(
                "sema/lint/unresolved-variable.mel",
                include_str!("../../../tests/corpus/sema/lint/unresolved-variable.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_delete_selection_omission_fixture() {
        insta::assert_snapshot!(
            "sema_delete_selection_omission",
            render_snapshot(
                "sema/command-schema/delete-selection-omission.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/delete-selection-omission.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_sets_selection_omission_fixture() {
        insta::assert_snapshot!(
            "sema_sets_selection_omission",
            render_snapshot(
                "sema/command-schema/sets-selection-omission.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/sets-selection-omission.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_poly_list_component_conversion_selection_omission_fixture() {
        insta::assert_snapshot!(
            "sema_poly_list_component_conversion_selection_omission",
            render_snapshot(
                "sema/command-schema/poly-list-component-conversion-selection-omission.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/poly-list-component-conversion-selection-omission.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_filter_expand_explicit_list_fixture() {
        insta::assert_snapshot!(
            "sema_filter_expand_explicit_list",
            render_snapshot(
                "sema/command-schema/filter-expand-explicit-list.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/filter-expand-explicit-list.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_eval_echo_single_script_fixture() {
        insta::assert_snapshot!(
            "sema_eval_echo_single_script",
            render_snapshot(
                "sema/command-schema/eval-echo-single-script.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/eval-echo-single-script.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_shading_node_single_type_fixture() {
        insta::assert_snapshot!(
            "sema_shading_node_single_type",
            render_snapshot(
                "sema/command-schema/shading-node-single-type.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/shading-node-single-type.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_poly_edit_uv_explicit_target_fixture() {
        insta::assert_snapshot!(
            "sema_poly_edit_uv_explicit_target",
            render_snapshot(
                "sema/command-schema/poly-edit-uv-explicit-target.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/poly-edit-uv-explicit-target.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_anim_layer_target_fixture() {
        insta::assert_snapshot!(
            "sema_anim_layer_target",
            render_snapshot(
                "sema/command-schema/anim-layer-target.mel",
                include_str!("../../../tests/corpus/sema/command-schema/anim-layer-target.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_reference_query_target_fixture() {
        insta::assert_snapshot!(
            "sema_reference_query_target",
            render_snapshot(
                "sema/command-schema/reference-query-target.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/reference-query-target.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_tree_view_query_item_fixture() {
        insta::assert_snapshot!(
            "sema_tree_view_query_item",
            render_snapshot(
                "sema/command-schema/tree-view-query-item.mel",
                include_str!("../../../tests/corpus/sema/command-schema/tree-view-query-item.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_attribute_exists_two_args_fixture() {
        insta::assert_snapshot!(
            "sema_attribute_exists_two_args",
            render_snapshot(
                "sema/command-schema/attribute-exists-two-args.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/attribute-exists-two-args.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_set_render_pass_type_target_fixture() {
        insta::assert_snapshot!(
            "sema_set_render_pass_type_target",
            render_snapshot(
                "sema/command-schema/set-render-pass-type-target.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/set-render-pass-type-target.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_namespace_info_current_fixture() {
        insta::assert_snapshot!(
            "sema_namespace_info_current",
            render_snapshot(
                "sema/command-schema/namespace-info-current.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/namespace-info-current.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_particle_query_target_fixture() {
        insta::assert_snapshot!(
            "sema_particle_query_target",
            render_snapshot(
                "sema/command-schema/particle-query-target.mel",
                include_str!("../../../tests/corpus/sema/command-schema/particle-query-target.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_list_transforms_single_arg_fixture() {
        insta::assert_snapshot!(
            "sema_list_transforms_single_arg",
            render_snapshot(
                "sema/command-schema/list-transforms-single-arg.mel",
                include_str!(
                    "../../../tests/corpus/sema/command-schema/list-transforms-single-arg.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_move_target_tail_fixture() {
        insta::assert_snapshot!(
            "sema_move_target_tail",
            render_snapshot(
                "sema/command-schema/move-target-tail.mel",
                include_str!("../../../tests/corpus/sema/command-schema/move-target-tail.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_for_in_binding_implicit_fixture() {
        insta::assert_snapshot!(
            "sema_for_in_binding_implicit",
            render_snapshot(
                "sema/lint/for-in-binding-implicit.mel",
                include_str!("../../../tests/corpus/sema/lint/for-in-binding-implicit.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_boolean_alias_return_fixture() {
        insta::assert_snapshot!(
            "sema_boolean_alias_return",
            render_snapshot(
                "sema/proc/boolean-alias-return.mel",
                include_str!("../../../tests/corpus/sema/proc/boolean-alias-return.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_var_init_comparison_int_result_fixture() {
        insta::assert_snapshot!(
            "sema_var_init_comparison_int_result",
            render_snapshot(
                "sema/proc/var-init-comparison-int-result.mel",
                include_str!("../../../tests/corpus/sema/proc/var-init-comparison-int-result.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_var_init_comparison_string_target_fixture() {
        insta::assert_snapshot!(
            "sema_var_init_comparison_string_target",
            render_snapshot(
                "sema/proc/var-init-comparison-string-target.mel",
                include_str!(
                    "../../../tests/corpus/sema/proc/var-init-comparison-string-target.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_var_assign_type_match_fixture() {
        insta::assert_snapshot!(
            "sema_var_assign_type_match",
            render_snapshot(
                "sema/proc/var-assign-type-match.mel",
                include_str!("../../../tests/corpus/sema/proc/var-assign-type-match.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_var_assign_type_mismatch_fixture() {
        insta::assert_snapshot!(
            "sema_var_assign_type_mismatch",
            render_snapshot(
                "sema/proc/var-assign-type-mismatch.mel",
                include_str!("../../../tests/corpus/sema/proc/var-assign-type-mismatch.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_scripted_panel_flag_mode_span_fixture() {
        insta::assert_snapshot!(
            "sema_scripted_panel_flag_mode_span",
            render_snapshot(
                "sema/lint/scripted-panel-flag-mode-span.mel",
                include_str!("../../../tests/corpus/sema/lint/scripted-panel-flag-mode-span.mel"),
            )
        );
    }

    #[test]
    fn snapshot_sema_scripted_panel_flag_mode_span_tabbed_fixture() {
        insta::assert_snapshot!(
            "sema_scripted_panel_flag_mode_span_tabbed",
            render_snapshot(
                "sema/lint/scripted-panel-flag-mode-span-tabbed.mel",
                include_str!(
                    "../../../tests/corpus/sema/lint/scripted-panel-flag-mode-span-tabbed.mel"
                ),
            )
        );
    }

    #[test]
    fn snapshot_sema_scripted_panel_flag_mode_span_tabbed_crlf_inline() {
        insta::assert_snapshot!(
            "sema_scripted_panel_flag_mode_span_tabbed_crlf_inline",
            render_snapshot(
                "inline-crlf-scripted-panel.mel",
                concat!(
                    "global string $gMainPane;\r\n",
                    "proc string test() {\r\n",
                    "\t\t\t$panelName = `scriptedPanel -menuBarVisible true -parent $gMainPane -l \"anyLabel\" -tearOff -type \"acPanelType\"`;\r\n",
                    "}\r\n",
                ),
            )
        );
    }

    #[test]
    fn diagnostics_keep_correct_columns_on_triple_digit_line_numbers() {
        let mut source = String::new();
        for _ in 0..99 {
            source.push('\n');
        }
        source.push_str(
            "\t\t\t$panelName = `scriptedPanel -menuBarVisible true -parent $gMainPane -l \"anyLabel\" -tearOff -type \"acPanelType\"`;\n",
        );

        let output = render_snapshot("inline-triple-digit-scripted-panel.mel", &source);
        assert!(output.contains("inline-triple-digit-scripted-panel.mel:100:61"));
        assert!(!output.contains("inline-triple-digit-scripted-panel.mel:100:69"));
    }

    fn unique_test_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        std::env::temp_dir().join(format!("mel-cli-{label}-{nanos}"))
    }

    fn cleanup_test_path(path: &PathBuf) {
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(path);
    }
}
