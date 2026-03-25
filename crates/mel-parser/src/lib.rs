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
use std::{fs, io, ops::Range, path::Path};

use decode::{decode_source_auto, decode_source_with_encoding};
pub use light::{
    LightCommandSurface, LightItem, LightItemSink, LightParse, LightParseOptions, LightProcSurface,
    LightScanReport, LightWord, parse_light_bytes, parse_light_bytes_with_encoding,
    parse_light_file, parse_light_file_with_encoding, parse_light_source,
    parse_light_source_with_options, scan_light_bytes_with_encoding_and_options_and_sink,
    scan_light_bytes_with_encoding_and_sink, scan_light_bytes_with_options_and_sink,
    scan_light_bytes_with_sink, scan_light_file_with_encoding_and_options_and_sink,
    scan_light_file_with_encoding_and_sink, scan_light_file_with_options_and_sink,
    scan_light_file_with_sink, scan_light_source_with_options_and_sink,
    scan_light_source_with_sink,
};
use parser::Parser;
use remap::{RangeMapper, remap_parse_ranges, remap_source_file_ranges};

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
    parse_owned_source(
        input.to_owned(),
        SourceMap::identity(input.len()),
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

fn parse_owned_source(
    source_text: String,
    source_map: SourceMap,
    source_encoding: SourceEncoding,
    decode_errors: Vec<DecodeDiagnostic>,
    options: ParseOptions,
) -> Parse {
    let mut parse = Parser::new(&source_text, options).parse();
    parse.source_text = source_text;
    parse.source_map = source_map;
    parse.source_encoding = source_encoding;
    parse.decode_errors = decode_errors;
    parse
}

fn parse_decoded_source(decoded: decode::DecodedSource<'_>, options: ParseOptions) -> Parse {
    let mut parse = parse_owned_source(
        decoded.text.into_owned(),
        decoded.offset_map.source_map(),
        decoded.encoding,
        decoded.diagnostics,
        options,
    );
    remap_parse_ranges(&mut parse, &decoded.offset_map);
    parse
}

fn parse_owned_bytes(input: Vec<u8>, options: ParseOptions) -> Parse {
    match String::from_utf8(input) {
        Ok(source_text) => {
            let source_len = source_text.len();
            parse_owned_source(
                source_text,
                SourceMap::identity(source_len),
                SourceEncoding::Utf8,
                Vec::new(),
                options,
            )
        }
        Err(error) => parse_decoded_source(decode_source_auto(error.as_bytes()), options),
    }
}

fn parse_owned_bytes_with_encoding(
    input: Vec<u8>,
    encoding: SourceEncoding,
    options: ParseOptions,
) -> Parse {
    if matches!(encoding, SourceEncoding::Utf8) {
        return parse_owned_bytes(input, options);
    }

    parse_decoded_source(decode_source_with_encoding(&input, encoding), options)
}
