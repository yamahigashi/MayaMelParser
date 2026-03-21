#![forbid(unsafe_code)]

use ariadne::{Color, Label, Report, ReportKind, Source};
use clap::{CommandFactory, Parser, ValueEnum};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use mel_parser::{Parse, SourceEncoding, parse_file, parse_file_with_encoding, parse_source};
use mel_sema::{DiagnosticSeverity, analyze};
use mel_syntax::TextRange;

const TOP_RANK_LIMIT: usize = 10;

#[derive(Debug, Parser)]
#[command(about = "Inspect MEL parse and diagnostic output", long_about = None)]
struct Args {
    #[arg(long, value_enum, default_value_t = CliEncoding::Auto)]
    encoding: CliEncoding,
    #[arg(long, value_name = "PATH", conflicts_with_all = ["directory", "inline_input"])]
    file: Option<PathBuf>,
    #[arg(
        long = "directory",
        visible_alias = "dir",
        value_name = "PATH",
        conflicts_with_all = ["file", "inline_input"]
    )]
    directory: Option<PathBuf>,
    #[arg(value_name = "INPUT", conflicts_with_all = ["file", "directory"])]
    inline_input: Option<String>,
}

impl Args {
    fn has_input(&self) -> bool {
        self.file.is_some() || self.directory.is_some() || self.inline_input.is_some()
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

    if let Some(path) = args.file {
        return print_single_file(&path, selected_encoding)
            .map_err(|error| RunError::Message(error.to_string()));
    }

    if let Some(path) = args.directory {
        return print_corpus_summary(&path, selected_encoding)
            .map_err(|error| RunError::Message(error.to_string()));
    }

    if let Some(input) = args.inline_input {
        print_parse_summary("inline", &parse_source(&input));
        return Ok(());
    }

    Ok(())
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

fn print_single_file(path: &Path, encoding: Option<SourceEncoding>) -> io::Result<()> {
    let parse = if let Some(encoding) = encoding {
        parse_file_with_encoding(path, encoding)?
    } else {
        parse_file(path)?
    };
    let label = path.display().to_string();
    print!("{}", format_single_file_output(&label, &parse)?);
    Ok(())
}

fn print_corpus_summary(root: &Path, encoding: Option<SourceEncoding>) -> io::Result<()> {
    let files = collect_mel_files(root)?;
    let mut summary = CorpusSummary::default();

    for path in files {
        summary.files += 1;

        match encoding
            .map(|encoding| parse_file_with_encoding(&path, encoding))
            .unwrap_or_else(|| parse_file(&path))
        {
            Ok(parse) => {
                let analysis = analyze(&parse.syntax);
                let file_summary = FileSummary {
                    path: path.display().to_string(),
                    decode_errors: parse.decode_errors.len(),
                    lex_errors: parse.lex_errors.len(),
                    parse_errors: parse.errors.len(),
                    parse_error_messages: parse
                        .errors
                        .iter()
                        .map(|error| error.message.clone())
                        .collect(),
                    semantic_diagnostics: analysis.diagnostics.len(),
                };
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

fn collect_mel_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_mel_files_recursive(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_mel_files_recursive(root: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_mel_files_recursive(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "mel") {
            files.push(path);
        }
    }

    Ok(())
}

fn print_parse_summary(label: &str, parse: &Parse) {
    print!("{}", format_parse_summary(label, parse));
}

fn format_parse_summary(label: &str, parse: &Parse) -> String {
    let analysis = analyze(&parse.syntax);
    format!(
        "source: {label}\nencoding: {}\nitems: {}\ndecode diagnostics: {}\nlexical diagnostics: {}\nparse errors: {}\nsemantic diagnostics: {}\n",
        parse.source_encoding.label(),
        parse.syntax.items.len(),
        parse.decode_errors.len(),
        parse.lex_errors.len(),
        parse.errors.len(),
        analysis.diagnostics.len()
    )
}

fn format_single_file_output(label: &str, parse: &Parse) -> io::Result<String> {
    let mut output = format_parse_summary(label, parse);
    output.push_str(&render_diagnostics(label, parse)?);
    Ok(output)
}

#[derive(Debug, Clone)]
struct FileDiagnostic {
    stage: &'static str,
    severity: DiagnosticSeverity,
    message: String,
    range: TextRange,
}

fn render_diagnostics(label: &str, parse: &Parse) -> io::Result<String> {
    let diagnostics = collect_diagnostics(parse);
    if diagnostics.is_empty() {
        return Ok(String::new());
    }

    let mut rendered = Vec::new();
    for diagnostic in diagnostics {
        let span = parse.source_map.display_range(diagnostic.range);
        Report::build(report_kind(diagnostic.severity), (label, span.clone()))
            .with_message(format!("{}: {}", diagnostic.stage, diagnostic.message))
            .with_label(
                Label::new((label, span))
                    .with_message(diagnostic.message)
                    .with_color(stage_color(diagnostic.stage, diagnostic.severity)),
            )
            .finish()
            .write(
                (label, Source::from(parse.source_text.as_str())),
                &mut rendered,
            )
            .map_err(io::Error::other)?;
    }

    String::from_utf8(rendered).map_err(io::Error::other)
}

fn collect_diagnostics(parse: &Parse) -> Vec<FileDiagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(parse.decode_errors.iter().map(|diagnostic| FileDiagnostic {
        stage: "decode",
        severity: DiagnosticSeverity::Error,
        message: diagnostic.message.clone(),
        range: diagnostic.range,
    }));
    diagnostics.extend(parse.lex_errors.iter().map(|diagnostic| FileDiagnostic {
        stage: "lex",
        severity: DiagnosticSeverity::Error,
        message: diagnostic.message.clone(),
        range: diagnostic.range,
    }));
    diagnostics.extend(parse.errors.iter().map(|diagnostic| FileDiagnostic {
        stage: "parse",
        severity: DiagnosticSeverity::Error,
        message: diagnostic.message.clone(),
        range: diagnostic.range,
    }));
    diagnostics.extend(
        analyze(&parse.syntax)
            .diagnostics
            .into_iter()
            .map(|diagnostic| FileDiagnostic {
                stage: "sema",
                severity: diagnostic.severity,
                message: diagnostic.message,
                range: diagnostic.range,
            }),
    );
    diagnostics
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

#[cfg(test)]
mod tests {
    use super::{Args, CorpusSummary, FileSummary, format_single_file_output, parse_cli_args};
    use clap::{CommandFactory, error::ErrorKind};
    use mel_parser::parse_source;
    use std::path::PathBuf;

    fn render_snapshot(label: &str, source: &str) -> String {
        format_single_file_output(label, &parse_source(source)).expect("snapshot should render")
    }

    #[test]
    fn cli_accepts_directory_flag() {
        let args = parse_cli_args(["mel-cli", "--directory", "tests/private-corpus"])
            .expect("directory flag should parse");
        assert_eq!(args.directory, Some(PathBuf::from("tests/private-corpus")));
    }

    #[test]
    fn cli_accepts_dir_alias() {
        let args = parse_cli_args(["mel-cli", "--dir", "tests/private-corpus"])
            .expect("dir alias should parse");
        assert_eq!(args.directory, Some(PathBuf::from("tests/private-corpus")));
    }

    #[test]
    fn cli_rejects_removed_corpus_dir_flag() {
        let error = parse_cli_args(["mel-cli", "--corpus-dir", "tests/private-corpus"])
            .expect_err("removed flag should fail");
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn cli_rejects_conflicting_modes() {
        let error = parse_cli_args([
            "mel-cli",
            "--file",
            "a.mel",
            "--directory",
            "tests/private-corpus",
        ])
        .expect_err("conflicting modes should fail");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn cli_rejects_invalid_encoding() {
        let error = parse_cli_args(["mel-cli", "--encoding", "latin1", "`ls -sl`;"])
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
        assert!(help.contains("--directory <PATH>"));
        assert!(help.contains("[aliases: --dir]"));
        assert!(help.contains("[possible values: auto, utf8, cp932, gbk]"));
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
}
