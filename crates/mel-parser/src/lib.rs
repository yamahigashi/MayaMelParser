#![forbid(unsafe_code)]
//! Minimal parser scaffold.
//!
//! This parser keeps the MEL surface intentionally small, but it now supports
//! byte-safe file inputs, a Pratt expression layer, command-style invocations,
//! indexing, and the first loop statements.

mod decode;
mod light;
mod parser;
mod remap;

#[cfg(test)]
mod tests;

use std::{fs, io, ops::Range, path::Path};

use decode::{decode_source_auto, decode_source_with_encoding};
pub use light::{
    LightCommandSurface, LightItem, LightParse, LightParseOptions, LightProcSurface, LightWord,
    parse_light_bytes, parse_light_bytes_with_encoding, parse_light_file,
    parse_light_file_with_encoding, parse_light_source, parse_light_source_with_options,
};
use parser::Parser;
use remap::{RangeMapper, remap_parse_ranges, remap_source_file_ranges};

use mel_syntax::{LexDiagnostic, SourceMap, SourceView, TextRange};

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

#[derive(Debug, Clone)]
pub struct ParseSlice<'a> {
    pub syntax: mel_ast::SourceFile,
    pub source: SourceView<'a>,
    pub lex_errors: Vec<LexDiagnostic>,
    pub errors: Vec<ParseError>,
}

impl ParseSlice<'_> {
    #[must_use]
    pub fn source_slice(&self, range: TextRange) -> &str {
        self.source.slice(range)
    }

    #[must_use]
    pub fn display_slice(&self, range: TextRange) -> &str {
        self.source.display_slice(range)
    }
}

impl Parse {
    #[must_use]
    pub fn source_view(&self) -> SourceView<'_> {
        SourceView::new(&self.source_text, &self.source_map)
    }

    #[must_use]
    pub fn source_range(&self, range: TextRange) -> Range<usize> {
        self.source_view().display_range(range)
    }

    #[must_use]
    pub fn source_slice(&self, range: TextRange) -> &str {
        self.source_view().slice(range)
    }

    #[must_use]
    pub fn display_slice(&self, range: TextRange) -> &str {
        self.source_view().display_slice(range)
    }

    #[must_use]
    pub fn string_literal_contents(&self, range: TextRange) -> Option<&str> {
        self.source_slice(range)
            .strip_prefix('"')?
            .strip_suffix('"')
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
pub fn parse_source_view_range(source: SourceView<'_>, range: TextRange) -> ParseSlice<'_> {
    parse_source_view_range_with_options(source, range, ParseOptions::default())
}

#[must_use]
pub fn parse_source_view_range_with_options(
    source: SourceView<'_>,
    range: TextRange,
    options: ParseOptions,
) -> ParseSlice<'_> {
    let display_range = source.display_range(range);
    let input = &source.text()[display_range.clone()];
    let mut parse = parse_source_with_options(input, options);
    let mapper = SourceViewRangeMapper {
        source,
        display_start: display_range.start,
    };
    remap_source_file_ranges(&mut parse.syntax, &mapper);
    for diagnostic in &mut parse.lex_errors {
        diagnostic.range = mapper.map_range(diagnostic.range);
    }
    for error in &mut parse.errors {
        error.range = mapper.map_range(error.range);
    }

    ParseSlice {
        syntax: parse.syntax,
        source,
        lex_errors: parse.lex_errors,
        errors: parse.errors,
    }
}

#[must_use]
pub fn parse_bytes(input: &[u8]) -> Parse {
    let decoded = decode_source_auto(input);
    let source_text = decoded.text.into_owned();
    let mut parse = parse_source(&source_text);
    parse.source_text = source_text;
    parse.source_map =
        SourceMap::from_source_to_display(decoded.offset_map.source_to_decoded.clone());
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
    parse.source_map =
        SourceMap::from_source_to_display(decoded.offset_map.source_to_decoded.clone());
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

struct SourceViewRangeMapper<'a> {
    source: SourceView<'a>,
    display_start: usize,
}

impl RangeMapper for SourceViewRangeMapper<'_> {
    fn map_range(&self, range: TextRange) -> TextRange {
        let start = self.display_start + usize::from(range.start());
        let end = self.display_start + usize::from(range.end());
        self.source.source_range_from_display_range(start..end)
    }
}
