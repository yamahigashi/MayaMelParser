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

use std::borrow::Cow;
use std::sync::Arc;
use std::{fs, io, ops::Range, path::Path};

use decode::{
    decode_owned_bytes_auto, decode_owned_bytes_with_encoding, decode_source_auto,
    decode_source_with_encoding,
};
pub use light::{
    LightCommandSurface, LightItem, LightItemSink, LightParse, LightParseOptions, LightProcSurface,
    LightScanReport, LightSourceFile, LightWord, SharedLightParse, SharedLightScanReport,
    parse_light_bytes, parse_light_bytes_with_encoding, parse_light_file,
    parse_light_file_with_encoding, parse_light_shared_source,
    parse_light_shared_source_with_options, parse_light_source, parse_light_source_with_options,
    scan_light_bytes_with_encoding_and_options_and_sink, scan_light_bytes_with_encoding_and_sink,
    scan_light_bytes_with_options_and_sink, scan_light_bytes_with_sink,
    scan_light_file_with_encoding_and_options_and_sink, scan_light_file_with_encoding_and_sink,
    scan_light_file_with_options_and_sink, scan_light_file_with_sink,
    scan_light_shared_source_with_options_and_sink, scan_light_shared_source_with_sink,
    scan_light_source_with_options_and_sink, scan_light_source_with_sink,
};
use parser::Parser;
use remap::{RangeMapper, remap_parse_ranges_with_mapper, remap_source_file_ranges};

use mel_syntax::{LexDiagnostic, SourceMap, SourceView, TextRange};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeDiagnostic {
    pub message: Cow<'static, str>,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: &'static str,
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
pub struct SharedParse {
    pub syntax: mel_ast::SourceFile,
    pub source_text: Arc<str>,
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

impl SharedParse {
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

impl From<SharedParse> for Parse {
    fn from(value: SharedParse) -> Self {
        Self {
            syntax: value.syntax,
            source_text: value.source_text.as_ref().to_owned(),
            source_map: value.source_map,
            source_encoding: value.source_encoding,
            decode_errors: value.decode_errors,
            lex_errors: value.lex_errors,
            errors: value.errors,
        }
    }
}

#[must_use]
pub fn parse_source(input: &str) -> Parse {
    parse_source_with_options(input, ParseOptions::default())
}

#[must_use]
pub fn parse_source_with_options(input: &str, options: ParseOptions) -> Parse {
    parse_owned_source(
        input.to_owned(),
        SourceMap::identity(input.len()),
        SourceEncoding::Utf8,
        Vec::new(),
        options,
    )
}

#[must_use]
pub fn parse_shared_source(input: Arc<str>) -> SharedParse {
    parse_shared_source_with_options(input, ParseOptions::default())
}

#[must_use]
pub fn parse_shared_source_with_options(input: Arc<str>, options: ParseOptions) -> SharedParse {
    let len = input.len();
    parse_shared_source_text(
        input,
        SourceMap::identity(len),
        SourceEncoding::Utf8,
        Vec::new(),
        options,
    )
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
    let mut parse = parse_borrowed_source(
        input,
        SourceMap::identity(input.len()),
        SourceEncoding::Utf8,
        Vec::new(),
        options,
    );
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
    parse_decoded_source(decode_source_auto(input), ParseOptions::default())
}

#[must_use]
pub fn parse_bytes_with_encoding(input: &[u8], encoding: SourceEncoding) -> Parse {
    parse_decoded_source(
        decode_source_with_encoding(input, encoding),
        ParseOptions::default(),
    )
}

pub fn parse_file(path: impl AsRef<Path>) -> io::Result<Parse> {
    let bytes = fs::read(path)?;
    Ok(parse_owned_bytes(bytes, ParseOptions::default()))
}

pub fn parse_file_with_encoding(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
) -> io::Result<Parse> {
    let bytes = fs::read(path)?;
    Ok(parse_owned_bytes_with_encoding(
        bytes,
        encoding,
        ParseOptions::default(),
    ))
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

struct SourceMapRangeMapper<'a> {
    source_map: &'a SourceMap,
}

impl RangeMapper for SourceMapRangeMapper<'_> {
    fn map_range(&self, range: TextRange) -> TextRange {
        self.source_map
            .source_range_from_display_range(usize::from(range.start())..usize::from(range.end()))
    }
}

fn parse_owned_source(
    source_text: String,
    source_map: SourceMap,
    source_encoding: SourceEncoding,
    decode_errors: Vec<DecodeDiagnostic>,
    options: ParseOptions,
) -> Parse {
    let mut parse = parse_borrowed_source(
        &source_text,
        source_map,
        source_encoding,
        decode_errors,
        options,
    );
    parse.source_text = source_text;
    parse
}

fn parse_shared_source_text(
    source_text: Arc<str>,
    source_map: SourceMap,
    source_encoding: SourceEncoding,
    decode_errors: Vec<DecodeDiagnostic>,
    options: ParseOptions,
) -> SharedParse {
    let parse = Parser::new(&source_text, options).parse();
    SharedParse {
        syntax: parse.syntax,
        source_text,
        source_map,
        source_encoding,
        decode_errors,
        lex_errors: parse.lex_errors,
        errors: parse.errors,
    }
}

fn parse_borrowed_source(
    input: &str,
    source_map: SourceMap,
    source_encoding: SourceEncoding,
    decode_errors: Vec<DecodeDiagnostic>,
    options: ParseOptions,
) -> Parse {
    let mut parse = Parser::new(input, options).parse();
    parse.source_map = source_map;
    parse.source_encoding = source_encoding;
    parse.decode_errors = decode_errors;
    parse
}

fn parse_decoded_source(decoded: decode::DecodedSource<'_>, options: ParseOptions) -> Parse {
    let source_map = decoded.offset_map.source_map();
    let mut parse = parse_owned_source(
        decoded.text.into_owned(),
        source_map.clone(),
        decoded.encoding,
        decoded.diagnostics,
        options,
    );
    remap_parse_ranges_with_mapper(
        &mut parse,
        &SourceMapRangeMapper {
            source_map: &source_map,
        },
    );
    parse
}

fn parse_owned_decoded_source(decoded: decode::DecodedOwnedSource, options: ParseOptions) -> Parse {
    let source_map = decoded.offset_map.source_map();
    let mut parse = parse_owned_source(
        decoded.text,
        source_map.clone(),
        decoded.encoding,
        decoded.diagnostics,
        options,
    );
    remap_parse_ranges_with_mapper(
        &mut parse,
        &SourceMapRangeMapper {
            source_map: &source_map,
        },
    );
    parse
}

fn parse_owned_bytes(input: Vec<u8>, options: ParseOptions) -> Parse {
    parse_owned_decoded_source(decode_owned_bytes_auto(input), options)
}

fn parse_owned_bytes_with_encoding(
    input: Vec<u8>,
    encoding: SourceEncoding,
    options: ParseOptions,
) -> Parse {
    parse_owned_decoded_source(decode_owned_bytes_with_encoding(input, encoding), options)
}
