use crate::args::CliDiagnosticLevel;
use ariadne::{Color, Config, Label, Report, ReportKind, Source};
#[cfg(test)]
use maya_mel::parser::LightParse;
use maya_mel::parser::LightScanReport;
use maya_mel::syntax::{SourceMap, TextRange, range_end, range_start, text_range};
use maya_mel::{
    Diagnostic, DiagnosticFilter, DiagnosticLabel, DiagnosticSeverity, MayaCommandRegistry, Parse,
    analyze_diagnostics_with_registry, analyze_diagnostics_with_registry_filtered,
};
use std::{fmt::Write as FmtWrite, io, io::Write, sync::Arc};

const DIAGNOSTIC_TAB_WIDTH: usize = 1;

#[derive(Debug, Clone)]
pub(crate) enum FileDiagnosticText<'a> {
    Borrowed(&'a str),
    Shared(Arc<str>),
}

impl FileDiagnosticText<'_> {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(text) => text,
            Self::Shared(text) => text.as_ref(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FileDiagnostic<'a> {
    pub(crate) stage: &'static str,
    pub(crate) severity: DiagnosticSeverity,
    pub(crate) message: FileDiagnosticText<'a>,
    pub(crate) labels: Vec<FileDiagnosticLabel<'a>>,
}

#[derive(Clone, Debug)]
pub(crate) struct FileDiagnosticLabel<'a> {
    pub(crate) range: TextRange,
    pub(crate) message: Option<FileDiagnosticText<'a>>,
    pub(crate) is_primary: bool,
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

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct DiagnosticCounts {
    pub(crate) decode: usize,
    pub(crate) lex: usize,
    pub(crate) parse: usize,
    pub(crate) sema: usize,
    pub(crate) light: usize,
}

pub(crate) fn render_file_diagnostics_into(
    mut writer: impl Write,
    label: &str,
    source_text: &str,
    source_map: &SourceMap,
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

pub(crate) fn append_compact_file_diagnostics(
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

pub(crate) fn append_compact_parse_diagnostics(
    output: &mut String,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
    sema_diagnostics: &[Diagnostic],
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

pub(crate) fn compute_normalized_line_starts(source_text: &str) -> Vec<usize> {
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

pub(crate) fn normalized_line_col_for_offset(
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

pub(crate) fn normalize_diagnostic_source_text(source_text: &str) -> (String, SourceMap) {
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

pub(crate) fn compute_display_line_starts(source_text: &str) -> Vec<usize> {
    let bytes = source_text.as_bytes();
    let mut starts = vec![0];
    for (offset, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            starts.push(offset + 1);
        }
    }
    starts
}

pub(crate) fn collect_diagnostics_with_sema(
    parse: &Parse,
    filter: DiagnosticFilter,
    run_sema: bool,
) -> Vec<FileDiagnostic<'_>> {
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
    if run_sema {
        diagnostics.extend(analyze_parse_diagnostics(parse, filter).into_iter().map(
            |diagnostic| {
                FileDiagnostic {
                    stage: "sema",
                    severity: diagnostic.severity,
                    message: FileDiagnosticText::Shared(diagnostic.message),
                    labels: diagnostic
                        .labels
                        .into_iter()
                        .map(file_diagnostic_label)
                        .collect(),
                }
            },
        ));
    }
    diagnostics
}

pub(crate) fn filtered_sema_diagnostics(
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
) -> Vec<Diagnostic> {
    match diagnostic_level {
        CliDiagnosticLevel::All => analyze_parse_diagnostics(parse, DiagnosticFilter::All),
        CliDiagnosticLevel::Error => analyze_parse_diagnostics(parse, DiagnosticFilter::ErrorsOnly),
        CliDiagnosticLevel::None => Vec::new(),
    }
}

pub(crate) fn parse_diagnostic_counts(
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
    sema_diagnostics: &[Diagnostic],
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

#[cfg(test)]
pub(crate) fn collect_light_diagnostics(parse: &LightParse) -> Vec<FileDiagnostic<'_>> {
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

pub(crate) fn filtered_parse_diagnostics(
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
) -> Vec<FileDiagnostic<'_>> {
    filtered_parse_diagnostics_with_sema(parse, diagnostic_level, true)
}

pub(crate) fn filtered_parse_diagnostics_with_sema(
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
    run_sema: bool,
) -> Vec<FileDiagnostic<'_>> {
    match diagnostic_level {
        CliDiagnosticLevel::All => {
            collect_diagnostics_with_sema(parse, DiagnosticFilter::All, run_sema)
        }
        CliDiagnosticLevel::Error => filter_diagnostics(
            collect_diagnostics_with_sema(parse, DiagnosticFilter::ErrorsOnly, run_sema),
            diagnostic_level,
        ),
        CliDiagnosticLevel::None => Vec::new(),
    }
}

#[cfg(test)]
pub(crate) fn filtered_light_diagnostics(
    parse: &LightParse,
    diagnostic_level: CliDiagnosticLevel,
) -> Vec<FileDiagnostic<'_>> {
    match diagnostic_level {
        CliDiagnosticLevel::None => Vec::new(),
        _ => filter_diagnostics(collect_light_diagnostics(parse), diagnostic_level),
    }
}

pub(crate) fn diagnostic_counts(diagnostics: &[FileDiagnostic<'_>]) -> DiagnosticCounts {
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

pub(crate) fn light_scan_diagnostic_counts(
    report: &LightScanReport,
    diagnostic_level: CliDiagnosticLevel,
) -> DiagnosticCounts {
    match diagnostic_level {
        CliDiagnosticLevel::None => DiagnosticCounts::default(),
        CliDiagnosticLevel::All | CliDiagnosticLevel::Error => DiagnosticCounts {
            decode: report.decode_errors.len(),
            light: report.errors.len(),
            ..DiagnosticCounts::default()
        },
    }
}

pub(crate) fn append_compact_light_scan_diagnostics(
    output: &mut String,
    report: &LightScanReport,
    diagnostic_level: CliDiagnosticLevel,
) {
    if matches!(diagnostic_level, CliDiagnosticLevel::None) {
        return;
    }

    for diagnostic in &report.decode_errors {
        writeln!(
            output,
            "Error: decode: {} @ byte {}",
            diagnostic.message,
            range_start(diagnostic.range)
        )
        .expect("light scan decode diagnostic append");
    }
    for diagnostic in &report.errors {
        writeln!(
            output,
            "Error: light: {} @ byte {}",
            diagnostic.message,
            range_start(diagnostic.range)
        )
        .expect("light scan diagnostic append");
    }
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

fn file_diagnostic_label(label: DiagnosticLabel) -> FileDiagnosticLabel<'static> {
    FileDiagnosticLabel {
        range: label.range,
        message: Some(FileDiagnosticText::Shared(label.message)),
        is_primary: label.is_primary,
    }
}

fn analyze_parse_diagnostics(parse: &Parse, filter: DiagnosticFilter) -> Vec<Diagnostic> {
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
