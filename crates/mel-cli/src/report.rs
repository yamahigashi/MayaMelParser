use crate::{
    args::CliDiagnosticLevel,
    diagnostics::{
        DiagnosticCounts, append_compact_file_diagnostics, append_compact_parse_diagnostics,
        diagnostic_counts, filtered_light_diagnostics, filtered_parse_diagnostics,
        filtered_sema_diagnostics, parse_diagnostic_counts, render_file_diagnostics_into,
    },
};
use mel_maya::{MayaLightSpecializedCommand, MayaLightTopLevelItem, collect_top_level_facts_light};
use mel_parser::{LightParse, Parse};
use std::{
    collections::HashMap,
    fmt::Write as FmtWrite,
    fs, io,
    io::Write,
    path::{Path, PathBuf},
};

const TOP_RANK_LIMIT: usize = 10;

pub(crate) fn collect_source_files(root: &Path, lightweight: bool) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_source_files_recursive(root, &mut files, lightweight)?;
    files.sort();
    Ok(files)
}

pub(crate) fn print_parse_summary(label: &str, parse: &Parse) {
    let diagnostics = filtered_parse_diagnostics(parse, CliDiagnosticLevel::All);
    print!(
        "{}",
        format_parse_summary(label, parse, diagnostic_counts(&diagnostics))
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

pub(crate) fn write_single_file_output_with_style(
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

pub(crate) fn write_single_file_output(
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
