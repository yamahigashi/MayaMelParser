use clap::{Args as ClapArgs, CommandFactory, Parser, Subcommand, ValueEnum};
use maya_mel as mel_parser;
use mel_parser::SourceEncoding;
use std::{io, path::PathBuf};

#[derive(Debug, Parser)]
#[command(about = "Inspect MEL parse and diagnostic output", long_about = None)]
pub(crate) struct Args {
    #[arg(long, value_enum, default_value_t = CliEncoding::Auto, global = true)]
    pub(crate) encoding: CliEncoding,
    #[arg(
        long,
        value_enum,
        default_value_t = CliDiagnosticLevel::All,
        global = true
    )]
    pub(crate) diagnostic_level: CliDiagnosticLevel,
    #[arg(
        long,
        value_name = "MAX_BYTES",
        value_parser = parse_positive_usize,
        help = "Maximum source bytes to parse; other parser budgets scale proportionally from defaults",
        global = true
    )]
    pub(crate) max_bytes: Option<usize>,
    #[arg(long, global = true)]
    pub(crate) expression: bool,
    #[arg(long, conflicts_with = "inline_input")]
    pub(crate) lightweight: bool,
    #[arg(value_name = "PATH", conflicts_with = "inline_input")]
    pub(crate) path: Option<PathBuf>,
    #[arg(long = "inline", value_name = "SOURCE", conflicts_with = "path")]
    pub(crate) inline_input: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Option<CliCommand>,
}

impl Args {
    pub(crate) fn has_input(&self) -> bool {
        self.path.is_some() || self.inline_input.is_some() || self.command.is_some()
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum CliCommand {
    /// Materialize a full MEL parse and source-backed diagnostics.
    Parse(CliParseCommand),
    /// Run byte-native light scan and print summary-only output.
    Scan(CliPathCommand),
    /// Run Maya .ma selective extraction summary.
    Selective(CliPathCommand),
    /// Aggregate a directory with an explicit execution engine.
    Corpus(CliCorpusCommand),
}

#[derive(Debug, ClapArgs)]
pub(crate) struct CliParseCommand {
    #[arg(value_name = "PATH", conflicts_with = "inline_input")]
    pub(crate) path: Option<PathBuf>,
    #[arg(long = "inline", value_name = "SOURCE", conflicts_with = "path")]
    pub(crate) inline_input: Option<String>,
}

#[derive(Debug, ClapArgs)]
pub(crate) struct CliPathCommand {
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,
}

#[derive(Debug, ClapArgs)]
pub(crate) struct CliCorpusCommand {
    #[arg(value_name = "DIR")]
    pub(crate) root: PathBuf,
    #[arg(long, value_enum, default_value_t = CliCorpusEngine::Full)]
    pub(crate) engine: CliCorpusEngine,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum CliEncoding {
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
    pub(crate) fn into_source_encoding(self) -> Option<SourceEncoding> {
        match self {
            Self::Auto => None,
            Self::Utf8 => Some(SourceEncoding::Utf8),
            Self::Cp932 => Some(SourceEncoding::Cp932),
            Self::Gbk => Some(SourceEncoding::Gbk),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum CliDiagnosticLevel {
    #[default]
    #[value(name = "all")]
    All,
    #[value(name = "error")]
    Error,
    #[value(name = "none")]
    None,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum CliCorpusEngine {
    #[default]
    #[value(name = "full")]
    Full,
    #[value(name = "scan")]
    Scan,
    #[value(name = "selective")]
    Selective,
}

pub(crate) fn parse_cli_args<I, T>(args: I) -> Result<Args, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Args::try_parse_from(args)
}

pub(crate) fn print_help() -> io::Result<()> {
    let mut command = Args::command();
    command.print_help()?;
    println!();
    Ok(())
}

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid integer value: {value}"))?;
    if parsed == 0 {
        return Err("max bytes must be at least 1".to_owned());
    }
    Ok(parsed)
}
