#![forbid(unsafe_code)]
//! Minimal parser scaffold.
//!
//! This parser keeps the MEL surface intentionally small, but it now supports
//! byte-safe file inputs, a Pratt expression layer, command-style invocations,
//! indexing, and the first loop statements.

mod decode;
mod parser;
mod remap;

#[cfg(test)]
mod tests;

use std::{fs, io, ops::Range, path::Path};

use decode::{decode_source_auto, decode_source_with_encoding};
use parser::Parser;
use remap::remap_parse_ranges;

use mel_syntax::{LexDiagnostic, TextRange, range_end, range_start};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeDiagnostic {
    pub message: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceEncoding {
    Utf8,
    Cp932,
    Gbk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParseMode {
    #[default]
    Strict,
    AllowTrailingStmtWithoutSemi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParseOptions {
    pub mode: ParseMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    pub syntax: mel_ast::SourceFile,
    pub source_text: String,
    pub source_map: SourceMap,
    pub source_encoding: SourceEncoding,
    pub decode_errors: Vec<DecodeDiagnostic>,
    pub lex_errors: Vec<LexDiagnostic>,
    pub errors: Vec<ParseError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    source_to_display: Vec<u32>,
}

impl SourceMap {
    fn identity(len: usize) -> Self {
        let source_to_display = (0..=len)
            .map(|offset| u32::try_from(offset).unwrap_or(u32::MAX))
            .collect();
        Self { source_to_display }
    }

    fn from_offset_map(offset_map: &decode::OffsetMap) -> Self {
        Self {
            source_to_display: offset_map.source_to_decoded.clone(),
        }
    }

    #[must_use]
    pub fn display_offset(&self, offset: u32) -> usize {
        self.source_to_display
            .get(offset as usize)
            .copied()
            .or_else(|| self.source_to_display.last().copied())
            .unwrap_or(offset) as usize
    }

    #[must_use]
    pub fn display_range(&self, range: TextRange) -> Range<usize> {
        self.display_offset(range_start(range))..self.display_offset(range_end(range))
    }
}

#[must_use]
pub fn parse_source(input: &str) -> Parse {
    parse_source_with_options(input, ParseOptions::default())
}

#[must_use]
pub fn parse_source_with_options(input: &str, options: ParseOptions) -> Parse {
    let mut parse = Parser::new(input, options).parse();
    parse.source_text = input.to_owned();
    parse.source_map = SourceMap::identity(input.len());
    parse.source_encoding = SourceEncoding::Utf8;
    parse.decode_errors = Vec::new();
    parse
}

#[must_use]
pub fn parse_bytes(input: &[u8]) -> Parse {
    let decoded = decode_source_auto(input);
    let source_text = decoded.text.into_owned();
    let mut parse = parse_source(&source_text);
    parse.source_text = source_text;
    parse.source_map = SourceMap::from_offset_map(&decoded.offset_map);
    parse.source_encoding = decoded.encoding;
    remap_parse_ranges(&mut parse, &decoded.offset_map);
    parse.decode_errors = decoded.diagnostics;
    parse
}

#[must_use]
pub fn parse_bytes_with_encoding(input: &[u8], encoding: SourceEncoding) -> Parse {
    let decoded = decode_source_with_encoding(input, encoding);
    let source_text = decoded.text.into_owned();
    let mut parse = parse_source(&source_text);
    parse.source_text = source_text;
    parse.source_map = SourceMap::from_offset_map(&decoded.offset_map);
    parse.source_encoding = decoded.encoding;
    remap_parse_ranges(&mut parse, &decoded.offset_map);
    parse.decode_errors = decoded.diagnostics;
    parse
}

pub fn parse_file(path: impl AsRef<Path>) -> io::Result<Parse> {
    let bytes = fs::read(path)?;
    Ok(parse_bytes(&bytes))
}

pub fn parse_file_with_encoding(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
) -> io::Result<Parse> {
    let bytes = fs::read(path)?;
    Ok(parse_bytes_with_encoding(&bytes, encoding))
}
