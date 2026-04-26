#[cfg(test)]
use crate::diagnostics::append_compact_file_diagnostics;
#[cfg(test)]
use crate::diagnostics::filtered_light_diagnostics;
use crate::{
    args::CliDiagnosticLevel,
    diagnostics::{
        DiagnosticCounts, append_compact_light_scan_diagnostics, append_compact_parse_diagnostics,
        diagnostic_counts, filtered_parse_diagnostics, filtered_parse_diagnostics_with_sema,
        filtered_sema_diagnostics, light_scan_diagnostic_counts, parse_diagnostic_counts,
        render_file_diagnostics_into,
    },
};
use maya_mel::Parse;
#[cfg(test)]
use maya_mel::maya::collect_top_level_facts_light;
use maya_mel::maya::model::MayaSelectiveItem;
#[cfg(test)]
use maya_mel::maya::model::{MayaLightSpecializedCommand, MayaLightTopLevelItem};
#[cfg(test)]
use maya_mel::parser::LightParse;
use maya_mel::parser::{
    LightCommandSurface, LightItem, LightItemSink, LightScanReport, LightSourceView,
};
use std::{
    collections::HashMap,
    fmt::Write as FmtWrite,
    fs, io,
    io::Write,
    path::{Path, PathBuf},
};

const TOP_RANK_LIMIT: usize = 10;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ParseReportOptions {
    pub(crate) diagnostic_level: CliDiagnosticLevel,
    pub(crate) run_sema: bool,
    pub(crate) mode_label: Option<&'static str>,
}

impl ParseReportOptions {
    pub(crate) const fn mel(diagnostic_level: CliDiagnosticLevel) -> Self {
        Self {
            diagnostic_level,
            run_sema: true,
            mode_label: None,
        }
    }

    pub(crate) const fn expression(diagnostic_level: CliDiagnosticLevel) -> Self {
        Self {
            diagnostic_level,
            run_sema: false,
            mode_label: Some("expression"),
        }
    }
}

pub(crate) fn collect_source_files(root: &Path, lightweight: bool) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_source_files_recursive(root, &mut files, lightweight)?;
    files.sort();
    Ok(files)
}

pub(crate) fn print_parse_summary_with_options(
    label: &str,
    parse: &Parse,
    options: ParseReportOptions,
) {
    let diagnostics =
        filtered_parse_diagnostics_with_sema(parse, options.diagnostic_level, options.run_sema);
    print!(
        "{}",
        format_parse_summary_with_options(label, parse, diagnostic_counts(&diagnostics), options)
    );
}

#[cfg(test)]
pub(crate) fn format_single_file_output(
    label: &str,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
) -> io::Result<String> {
    format_single_file_output_with_style(label, parse, diagnostic_level, true)
}

#[cfg(test)]
pub(crate) fn format_single_file_output_with_style(
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

#[cfg(test)]
pub(crate) fn write_single_file_output_with_style(
    writer: impl Write,
    label: &str,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
    fancy_diagnostics: bool,
) -> io::Result<()> {
    write_single_file_output_with_style_and_options(
        writer,
        label,
        parse,
        ParseReportOptions::mel(diagnostic_level),
        fancy_diagnostics,
    )
}

pub(crate) fn write_single_file_output_with_style_and_options(
    mut writer: impl Write,
    label: &str,
    parse: &Parse,
    options: ParseReportOptions,
    fancy_diagnostics: bool,
) -> io::Result<()> {
    if fancy_diagnostics {
        let diagnostics =
            filtered_parse_diagnostics_with_sema(parse, options.diagnostic_level, options.run_sema);
        let mut output = format_parse_summary_with_options(
            label,
            parse,
            diagnostic_counts(&diagnostics),
            options,
        );
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
        write_single_file_output_with_options(writer, label, parse, options)
    }
}

#[cfg(test)]
pub(crate) fn write_single_file_output(
    writer: impl Write,
    label: &str,
    parse: &Parse,
    diagnostic_level: CliDiagnosticLevel,
) -> io::Result<()> {
    write_single_file_output_with_options(
        writer,
        label,
        parse,
        ParseReportOptions::mel(diagnostic_level),
    )
}

pub(crate) fn write_single_file_output_with_options(
    mut writer: impl Write,
    label: &str,
    parse: &Parse,
    options: ParseReportOptions,
) -> io::Result<()> {
    let sema_diagnostics = if options.run_sema {
        filtered_sema_diagnostics(parse, options.diagnostic_level)
    } else {
        Vec::new()
    };
    let counts = parse_diagnostic_counts(parse, options.diagnostic_level, &sema_diagnostics);
    let mut output = String::new();
    append_parse_summary_with_options(&mut output, label, parse, counts, options)
        .expect("summary append");
    append_compact_parse_diagnostics(
        &mut output,
        parse,
        options.diagnostic_level,
        &sema_diagnostics,
    );
    writer.write_all(output.as_bytes())
}

#[cfg(test)]
pub(crate) fn format_light_single_file_output(
    label: &str,
    parse: &LightParse,
    diagnostic_level: CliDiagnosticLevel,
) -> io::Result<String> {
    format_light_single_file_output_with_style(label, parse, diagnostic_level, true)
}

#[cfg(test)]
pub(crate) fn format_light_single_file_output_with_style(
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

#[cfg(test)]
pub(crate) fn write_light_single_file_output_with_style(
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

#[cfg(test)]
pub(crate) fn write_light_single_file_output(
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

pub(crate) fn write_light_scan_single_file_output(
    mut writer: impl Write,
    label: &str,
    report: &LightScanReport,
    summary: &LightFileSummary,
    diagnostic_level: CliDiagnosticLevel,
) -> io::Result<()> {
    let mut output = String::new();
    append_light_scan_summary(&mut output, label, report, summary).expect("light scan summary");
    append_compact_light_scan_diagnostics(&mut output, report, diagnostic_level);
    writer.write_all(output.as_bytes())
}

pub(crate) fn write_selective_single_file_output(
    mut writer: impl Write,
    label: &str,
    report: &LightScanReport,
    summary: &SelectiveFileSummary,
    diagnostic_level: CliDiagnosticLevel,
) -> io::Result<()> {
    let mut output = String::new();
    append_selective_summary(&mut output, label, report, summary).expect("selective summary");
    append_compact_light_scan_diagnostics(&mut output, report, diagnostic_level);
    writer.write_all(output.as_bytes())
}

pub(crate) fn summarize_parse_file(
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

#[cfg(test)]
pub(crate) fn light_file_summary(
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

#[derive(Debug, Default)]
pub(crate) struct LightSummarySink {
    items: usize,
    command_items: usize,
    proc_items: usize,
    other_items: usize,
    opaque_tail_commands: usize,
    specialized_set_attr: usize,
    set_attr_with_opaque_tail: usize,
}

impl LightSummarySink {
    pub(crate) fn finish(
        self,
        path: &Path,
        report: &LightScanReport,
        diagnostic_level: CliDiagnosticLevel,
    ) -> LightFileSummary {
        let counts = light_scan_diagnostic_counts(report, diagnostic_level);
        LightFileSummary {
            path: path.display().to_string(),
            decode_errors: counts.decode,
            light_parse_errors: counts.light,
            items: self.items,
            command_items: self.command_items,
            proc_items: self.proc_items,
            other_items: self.other_items,
            opaque_tail_commands: self.opaque_tail_commands,
            specialized_set_attr: self.specialized_set_attr,
            set_attr_with_opaque_tail: self.set_attr_with_opaque_tail,
        }
    }

    fn record_command(&mut self, source: LightSourceView<'_>, command: &LightCommandSurface) {
        self.command_items += 1;
        if command.opaque_tail.is_some() {
            self.opaque_tail_commands += 1;
        }
        if source.try_ascii_slice(command.head_range) == Some("setAttr") {
            self.specialized_set_attr += 1;
            if command.opaque_tail.is_some() {
                self.set_attr_with_opaque_tail += 1;
            }
        }
    }
}

impl LightItemSink for LightSummarySink {
    fn on_item(&mut self, source: LightSourceView<'_>, item: LightItem) {
        self.items += 1;
        match item {
            LightItem::Command(command) => self.record_command(source, &command),
            LightItem::Proc(_) => self.proc_items += 1,
            LightItem::Other { .. } => self.other_items += 1,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct SelectiveSummarySink {
    total_items: usize,
    requires: usize,
    files: usize,
    create_nodes: usize,
    set_attrs: usize,
    tracked_set_attrs: usize,
    set_attr_with_opaque_tail: usize,
    other_commands: usize,
}

impl SelectiveSummarySink {
    pub(crate) fn finish(
        self,
        path: &Path,
        report: &LightScanReport,
        diagnostic_level: CliDiagnosticLevel,
    ) -> SelectiveFileSummary {
        let counts = light_scan_diagnostic_counts(report, diagnostic_level);
        SelectiveFileSummary {
            path: path.display().to_string(),
            decode_errors: counts.decode,
            light_parse_errors: counts.light,
            total_items: self.total_items,
            requires: self.requires,
            files: self.files,
            create_nodes: self.create_nodes,
            set_attrs: self.set_attrs,
            tracked_set_attrs: self.tracked_set_attrs,
            set_attr_with_opaque_tail: self.set_attr_with_opaque_tail,
            other_commands: self.other_commands,
        }
    }
}

impl maya_mel::maya::model::MayaSelectiveItemSink for SelectiveSummarySink {
    fn on_item(&mut self, item: MayaSelectiveItem) {
        self.total_items += 1;
        match item {
            MayaSelectiveItem::Requires(_) => self.requires += 1,
            MayaSelectiveItem::File(_) => self.files += 1,
            MayaSelectiveItem::CreateNode(_) => self.create_nodes += 1,
            MayaSelectiveItem::SetAttr(set_attr) => {
                self.set_attrs += 1;
                if set_attr.tracked_attr.is_some() {
                    self.tracked_set_attrs += 1;
                }
                if set_attr.opaque_tail.is_some() {
                    self.set_attr_with_opaque_tail += 1;
                }
            }
            MayaSelectiveItem::OtherCommand { .. } => self.other_commands += 1,
        }
    }
}

pub(crate) fn format_light_corpus_summary(summary: &LightCorpusSummary) -> String {
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

pub(crate) fn format_selective_corpus_summary(summary: &SelectiveCorpusSummary) -> String {
    let mut output = format!(
        "corpus files: {}\nfiles with decode issues: {}\nfiles with light parse errors: {}\ntotal selective items: {}\ntotal requires: {}\ntotal file commands: {}\ntotal createNode: {}\ntotal setAttr: {}\ntotal tracked setAttr: {}\ntotal setAttr with opaque tail: {}\ntotal other commands: {}\nio errors: {}\n",
        summary.files,
        summary.files_with_decode_issues,
        summary.files_with_light_parse_errors,
        summary.total_items,
        summary.total_requires,
        summary.total_file_commands,
        summary.total_create_nodes,
        summary.total_set_attrs,
        summary.total_tracked_set_attrs,
        summary.total_set_attr_with_opaque_tail,
        summary.total_other_commands,
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

#[derive(Debug, Default)]
pub(crate) struct CorpusSummary {
    pub(crate) files: usize,
    pub(crate) files_with_decode_issues: usize,
    pub(crate) files_with_lex_errors: usize,
    pub(crate) files_with_parse_errors: usize,
    pub(crate) files_with_semantic_diagnostics: usize,
    pub(crate) io_errors: usize,
    pub(crate) samples: Vec<String>,
    parse_error_files: Vec<(String, usize)>,
    parse_error_message_counts: HashMap<String, usize>,
}

impl CorpusSummary {
    pub(crate) fn record(&mut self, file: FileSummary) {
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

    pub(crate) fn top_parse_error_files(&self) -> Vec<(String, usize)> {
        let mut ranked = self.parse_error_files.clone();
        ranked.sort_by(|lhs, rhs| rhs.1.cmp(&lhs.1).then_with(|| lhs.0.cmp(&rhs.0)));
        ranked.truncate(TOP_RANK_LIMIT);
        ranked
    }

    pub(crate) fn top_parse_error_messages(&self) -> Vec<(String, usize)> {
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
pub(crate) struct FileSummary {
    pub(crate) path: String,
    pub(crate) decode_errors: usize,
    pub(crate) lex_errors: usize,
    pub(crate) parse_errors: usize,
    pub(crate) parse_error_messages: Vec<String>,
    pub(crate) semantic_diagnostics: usize,
}

#[derive(Debug, Default)]
pub(crate) struct LightCorpusSummary {
    pub(crate) files: usize,
    pub(crate) files_with_decode_issues: usize,
    pub(crate) files_with_light_parse_errors: usize,
    pub(crate) io_errors: usize,
    pub(crate) total_items: usize,
    pub(crate) total_command_items: usize,
    pub(crate) total_proc_items: usize,
    pub(crate) total_opaque_tail_commands: usize,
    pub(crate) total_specialized_set_attr: usize,
    pub(crate) total_set_attr_with_opaque_tail: usize,
    pub(crate) samples: Vec<String>,
}

impl LightCorpusSummary {
    pub(crate) fn record(&mut self, file: LightFileSummary) {
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

#[derive(Debug, Default)]
pub(crate) struct SelectiveCorpusSummary {
    pub(crate) files: usize,
    pub(crate) files_with_decode_issues: usize,
    pub(crate) files_with_light_parse_errors: usize,
    pub(crate) io_errors: usize,
    pub(crate) total_items: usize,
    pub(crate) total_requires: usize,
    pub(crate) total_file_commands: usize,
    pub(crate) total_create_nodes: usize,
    pub(crate) total_set_attrs: usize,
    pub(crate) total_tracked_set_attrs: usize,
    pub(crate) total_set_attr_with_opaque_tail: usize,
    pub(crate) total_other_commands: usize,
    pub(crate) samples: Vec<String>,
}

impl SelectiveCorpusSummary {
    pub(crate) fn record(&mut self, file: SelectiveFileSummary) {
        if file.decode_errors > 0 {
            self.files_with_decode_issues += 1;
        }
        if file.light_parse_errors > 0 {
            self.files_with_light_parse_errors += 1;
        }
        self.total_items += file.total_items;
        self.total_requires += file.requires;
        self.total_file_commands += file.files;
        self.total_create_nodes += file.create_nodes;
        self.total_set_attrs += file.set_attrs;
        self.total_tracked_set_attrs += file.tracked_set_attrs;
        self.total_set_attr_with_opaque_tail += file.set_attr_with_opaque_tail;
        self.total_other_commands += file.other_commands;

        if self.samples.len() < 10 && (file.decode_errors > 0 || file.light_parse_errors > 0) {
            self.samples.push(format!(
                "{} decode={} light={} selective={}",
                file.path, file.decode_errors, file.light_parse_errors, file.total_items
            ));
        }
    }
}

#[derive(Debug)]
pub(crate) struct LightFileSummary {
    pub(crate) path: String,
    pub(crate) decode_errors: usize,
    pub(crate) light_parse_errors: usize,
    pub(crate) items: usize,
    pub(crate) command_items: usize,
    pub(crate) proc_items: usize,
    pub(crate) other_items: usize,
    pub(crate) opaque_tail_commands: usize,
    pub(crate) specialized_set_attr: usize,
    pub(crate) set_attr_with_opaque_tail: usize,
}

#[derive(Debug)]
pub(crate) struct SelectiveFileSummary {
    pub(crate) path: String,
    pub(crate) decode_errors: usize,
    pub(crate) light_parse_errors: usize,
    pub(crate) total_items: usize,
    pub(crate) requires: usize,
    pub(crate) files: usize,
    pub(crate) create_nodes: usize,
    pub(crate) set_attrs: usize,
    pub(crate) tracked_set_attrs: usize,
    pub(crate) set_attr_with_opaque_tail: usize,
    pub(crate) other_commands: usize,
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

fn format_parse_summary_with_options(
    label: &str,
    parse: &Parse,
    counts: DiagnosticCounts,
    options: ParseReportOptions,
) -> String {
    let mut output = String::new();
    append_parse_summary_with_options(&mut output, label, parse, counts, options)
        .expect("formatting parse summary should not fail");
    output
}

fn append_parse_summary_with_options(
    output: &mut String,
    label: &str,
    parse: &Parse,
    counts: DiagnosticCounts,
    options: ParseReportOptions,
) -> std::fmt::Result {
    if let Some(mode_label) = options.mode_label {
        writeln!(output, "source: {label}")?;
        writeln!(output, "mode: {mode_label}")?;
        writeln!(output, "encoding: {}", parse.source_encoding.label())?;
        writeln!(output, "items: {}", parse.syntax.items.len())?;
        writeln!(output, "decode diagnostics: {}", counts.decode)?;
        writeln!(output, "lexical diagnostics: {}", counts.lex)?;
        writeln!(output, "parse errors: {}", counts.parse)?;
        writeln!(output, "semantic diagnostics: {}", counts.sema)?;
        return Ok(());
    }

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

fn append_light_scan_summary(
    output: &mut String,
    label: &str,
    report: &LightScanReport,
    summary: &LightFileSummary,
) -> std::fmt::Result {
    write!(
        output,
        "source: {label}\nmode: scan\nencoding: {}\nitems: {}\ncommand items: {}\nproc items: {}\nother items: {}\nopaque-tail commands: {}\nlight specialized setAttr: {}\nsetAttr with opaque tail: {}\ndecode diagnostics: {}\nlight parse errors: {}\n",
        report.source_encoding.label(),
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

fn append_selective_summary(
    output: &mut String,
    label: &str,
    report: &LightScanReport,
    summary: &SelectiveFileSummary,
) -> std::fmt::Result {
    write!(
        output,
        "source: {label}\nmode: selective\nencoding: {}\nselective items: {}\nrequires: {}\nfile commands: {}\ncreateNode: {}\nsetAttr: {}\ntracked setAttr: {}\nsetAttr with opaque tail: {}\nother commands: {}\ndecode diagnostics: {}\nlight parse errors: {}\n",
        report.source_encoding.label(),
        summary.total_items,
        summary.requires,
        summary.files,
        summary.create_nodes,
        summary.set_attrs,
        summary.tracked_set_attrs,
        summary.set_attr_with_opaque_tail,
        summary.other_commands,
        summary.decode_errors,
        summary.light_parse_errors,
    )
}
