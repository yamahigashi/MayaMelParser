use clap::{CommandFactory, Parser, ValueEnum};
use maya_mel as mel_parser;
use mel_parser::SourceEncoding;
use std::{io, path::PathBuf};

#[derive(Debug, Parser)]
#[command(about = "Inspect MEL parse and diagnostic output", long_about = None)]
pub(crate) struct Args {
    #[arg(long, value_enum, default_value_t = CliEncoding::Auto)]
    pub(crate) encoding: CliEncoding,
    #[arg(long, value_enum, default_value_t = CliDiagnosticLevel::All)]
    pub(crate) diagnostic_level: CliDiagnosticLevel,
    #[arg(
        long,
        value_name = "MAX_BYTES",
        value_parser = parse_positive_usize,
        help = "Maximum source bytes to parse; other parser budgets scale proportionally from defaults"
    )]
    pub(crate) max_bytes: Option<usize>,
    #[arg(long, conflicts_with = "inline_input")]
    pub(crate) lightweight: bool,
    #[arg(value_name = "PATH", conflicts_with = "inline_input")]
    pub(crate) path: Option<PathBuf>,
    #[arg(long = "inline", value_name = "SOURCE", conflicts_with = "path")]
    pub(crate) inline_input: Option<String>,
}

impl Args {
    pub(crate) fn has_input(&self) -> bool {
        self.path.is_some() || self.inline_input.is_some()
    }
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
