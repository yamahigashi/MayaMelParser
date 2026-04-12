#![forbid(unsafe_code)]
//! Full and lightweight MEL parsing entry points.
//!
//! Most users should start with [`parse_source`] or [`parse_file`]. Those APIs
//! return a typed AST plus lexical, decoding, and parse diagnostics.
//!
//! Use the lightweight parse APIs such as [`parse_light_source`] when you need
//! command-oriented scans over large sources without materializing the full AST.

pub(crate) mod decode;
mod engine;
pub(crate) mod light;
pub(crate) mod remap;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

use std::borrow::Cow;
use std::sync::Arc;
use std::{fs, io, ops::Range, path::Path};

use self::decode::{
    decode_owned_bytes_auto, decode_owned_bytes_with_encoding, decode_source_auto,
    decode_source_with_encoding,
};
use self::engine::Parser;
pub use self::light::{
    LightCommandSurface, LightItem, LightItemSink, LightParse, LightParseOptions, LightProcSurface,
    LightScanReport, LightSourceFile, LightWord, SharedLightParse, SharedLightScanReport,
    parse_light_bytes, parse_light_bytes_with_encoding, parse_light_file,
    parse_light_file_with_encoding, parse_light_shared_bytes,
    parse_light_shared_bytes_with_encoding, parse_light_shared_file,
    parse_light_shared_file_with_encoding, parse_light_shared_source,
    parse_light_shared_source_with_options, parse_light_source, parse_light_source_with_options,
    scan_light_bytes_with_encoding_and_options_and_sink, scan_light_bytes_with_encoding_and_sink,
    scan_light_bytes_with_options_and_sink, scan_light_bytes_with_sink,
    scan_light_file_with_encoding_and_options_and_sink, scan_light_file_with_encoding_and_sink,
    scan_light_file_with_options_and_sink, scan_light_file_with_sink,
    scan_light_shared_bytes_with_encoding_and_options_and_sink,
    scan_light_shared_bytes_with_options_and_sink,
    scan_light_shared_file_with_encoding_and_options_and_sink,
    scan_light_shared_file_with_options_and_sink, scan_light_shared_source_with_options_and_sink,
    scan_light_shared_source_with_sink, scan_light_source_with_options_and_sink,
    scan_light_source_with_sink,
};
use self::remap::{
    RangeMapper, remap_parse_ranges_with_mapper, remap_shared_parse_ranges_with_mapper,
    remap_source_file_ranges,
};

use mel_syntax::{LexDiagnostic, SourceMap, SourceView, TextRange, text_range};

const DEFAULT_MAX_BYTES: usize = 256 * 1024 * 1024;
const DEFAULT_MAX_NESTING_DEPTH: usize = 512;
const DEFAULT_MAX_TOKENS: usize = 8_000_000;
const DEFAULT_MAX_STATEMENTS: usize = 1_000_000;
const DEFAULT_MAX_LITERAL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
/// A diagnostic emitted while decoding non-UTF-8 source into display text.
pub struct DecodeDiagnostic {
    pub message: Cow<'static, str>,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A parse error emitted after lexing succeeds.
pub struct ParseError {
    pub message: &'static str,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Supported source encodings for byte-oriented parse entry points.
pub enum SourceEncoding {
    Utf8,
    Cp932,
    Gbk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
/// Parser behavior toggles for strict and snippet-oriented entry points.
pub enum ParseMode {
    #[default]
    Strict,
    AllowTrailingStmtWithoutSemi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Resource budgets enforced by full and lightweight parse entry points.
///
/// [`Default::default`] enables conservative hard limits intended to protect
/// parser callers from hostile or unexpectedly large input. Use
/// [`Self::unlimited`] only for trusted local workloads such as benchmarks.
pub struct ParseBudgets {
    pub max_bytes: usize,
    pub max_nesting_depth: usize,
    pub max_tokens: usize,
    pub max_statements: usize,
    pub max_literal_bytes: usize,
}

impl Default for ParseBudgets {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_nesting_depth: DEFAULT_MAX_NESTING_DEPTH,
            max_tokens: DEFAULT_MAX_TOKENS,
            max_statements: DEFAULT_MAX_STATEMENTS,
            max_literal_bytes: DEFAULT_MAX_LITERAL_BYTES,
        }
    }
}

impl ParseBudgets {
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_bytes: usize::MAX,
            max_nesting_depth: usize::MAX,
            max_tokens: usize::MAX,
            max_statements: usize::MAX,
            max_literal_bytes: usize::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
/// Options shared by the full parse entry points.
///
/// Most callers can use [`Default::default`]. Set [`ParseMode`] explicitly when
/// parsing snippet-like source that may omit the final semicolon.
pub struct ParseOptions {
    pub mode: ParseMode,
    pub budgets: ParseBudgets,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Owned full-parse result for source text plus all parser-side diagnostics.
///
/// This is the main result type returned by [`parse_source`], [`parse_bytes`],
/// and [`parse_file`]. The typed MEL syntax tree is stored in [`Self::syntax`],
/// while the original decoded text and source mapping remain available for
/// diagnostics and source slicing.
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
/// Shared full-parse result backed by `Arc<str>`.
///
/// Use this when the decoded source text needs to be shared across multiple
/// downstream consumers without cloning the full string.
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
/// Parse result for a source subrange produced from [`parse_source_view_range`].
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
/// Parse a UTF-8 source string into a full AST.
///
/// ```rust
/// use maya_mel::parse_source;
///
/// let parse = parse_source("global proc hello() {}");
/// assert!(parse.errors.is_empty());
/// assert_eq!(parse.syntax.items.len(), 1);
/// ```
pub fn parse_source(input: &str) -> Parse {
    parse_source_with_options(input, ParseOptions::default())
}

#[must_use]
/// Parse a UTF-8 source string with explicit [`ParseOptions`].
///
/// ```rust
/// use maya_mel::{ParseMode, ParseOptions, parse_source_with_options};
///
/// let parse = parse_source_with_options(
///     "print \"hello\"",
///     ParseOptions {
///         mode: ParseMode::AllowTrailingStmtWithoutSemi,
///         ..ParseOptions::default()
///     },
/// );
///
/// assert!(parse.errors.is_empty());
/// ```
pub fn parse_source_with_options(input: &str, options: ParseOptions) -> Parse {
    if let Some(error) = max_bytes_error_for_text(input.len(), options.budgets) {
        return parse_budget_failure_for_source(input.to_owned(), error);
    }
    parse_owned_source(
        input.to_owned(),
        SourceMap::identity(input.len()),
        SourceEncoding::Utf8,
        Vec::new(),
        options,
    )
}

#[must_use]
/// Parse shared UTF-8 source text into a full AST.
pub fn parse_shared_source(input: Arc<str>) -> SharedParse {
    parse_shared_source_with_options(input, ParseOptions::default())
}

#[must_use]
/// Parse shared UTF-8 source text with explicit [`ParseOptions`].
pub fn parse_shared_source_with_options(input: Arc<str>, options: ParseOptions) -> SharedParse {
    if let Some(error) = max_bytes_error_for_text(input.len(), options.budgets) {
        return shared_parse_budget_failure_for_source(input, error);
    }
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
/// Parse a range from an existing [`SourceView`] using default [`ParseOptions`].
pub fn parse_source_view_range(source: SourceView<'_>, range: TextRange) -> ParseSlice<'_> {
    parse_source_view_range_with_options(source, range, ParseOptions::default())
}

#[must_use]
/// Parse a range from an existing [`SourceView`] with explicit [`ParseOptions`].
pub fn parse_source_view_range_with_options(
    source: SourceView<'_>,
    range: TextRange,
    options: ParseOptions,
) -> ParseSlice<'_> {
    let display_range = source.display_range(range);
    let input = &source.text()[display_range.clone()];
    if let Some(error) = max_bytes_error_for_text(input.len(), options.budgets) {
        return ParseSlice {
            syntax: mel_ast::SourceFile { items: Vec::new() },
            source,
            lex_errors: Vec::new(),
            errors: vec![error],
        };
    }
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
/// Decode and parse bytes using automatic encoding detection.
pub fn parse_bytes(input: &[u8]) -> Parse {
    if let Some(error) = max_bytes_error_for_bytes(input.len(), ParseBudgets::default()) {
        return parse_budget_failure_empty(error);
    }
    parse_decoded_source(decode_source_auto(input), ParseOptions::default())
}

#[must_use]
/// Decode and parse bytes into a shared parse using automatic encoding detection.
pub fn parse_shared_bytes(input: &[u8]) -> SharedParse {
    if let Some(error) = max_bytes_error_for_bytes(input.len(), ParseBudgets::default()) {
        return shared_parse_budget_failure_empty(error);
    }
    parse_shared_decoded_source(decode_source_auto(input), ParseOptions::default())
}

#[must_use]
/// Decode and parse bytes with an explicit source encoding.
pub fn parse_bytes_with_encoding(input: &[u8], encoding: SourceEncoding) -> Parse {
    if let Some(error) = max_bytes_error_for_bytes(input.len(), ParseBudgets::default()) {
        return parse_budget_failure_empty(error);
    }
    parse_decoded_source(
        decode_source_with_encoding(input, encoding),
        ParseOptions::default(),
    )
}

#[must_use]
/// Decode and parse bytes into a shared parse with an explicit source encoding.
pub fn parse_shared_bytes_with_encoding(input: &[u8], encoding: SourceEncoding) -> SharedParse {
    if let Some(error) = max_bytes_error_for_bytes(input.len(), ParseBudgets::default()) {
        return shared_parse_budget_failure_empty(error);
    }
    parse_shared_decoded_source(
        decode_source_with_encoding(input, encoding),
        ParseOptions::default(),
    )
}

/// Read, decode, and parse a file using automatic encoding detection.
pub fn parse_shared_file(path: impl AsRef<Path>) -> io::Result<SharedParse> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), ParseBudgets::default())? {
        return Ok(shared_parse_budget_failure_empty(error));
    }
    let bytes = fs::read(path)?;
    Ok(parse_shared_bytes(&bytes))
}

/// Read, decode, and parse a file into a shared parse with an explicit encoding.
pub fn parse_shared_file_with_encoding(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
) -> io::Result<SharedParse> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), ParseBudgets::default())? {
        return Ok(shared_parse_budget_failure_empty(error));
    }
    let bytes = fs::read(path)?;
    Ok(parse_shared_bytes_with_encoding(&bytes, encoding))
}

/// Read, decode, and parse a file using automatic encoding detection.
pub fn parse_file(path: impl AsRef<Path>) -> io::Result<Parse> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), ParseBudgets::default())? {
        return Ok(parse_budget_failure_empty(error));
    }
    let bytes = fs::read(path)?;
    Ok(parse_owned_bytes(bytes, ParseOptions::default()))
}

/// Read, decode, and parse a file with an explicit encoding.
pub fn parse_file_with_encoding(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
) -> io::Result<Parse> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), ParseBudgets::default())? {
        return Ok(parse_budget_failure_empty(error));
    }
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

fn parse_shared_decoded_source(
    decoded: decode::DecodedSource<'_>,
    options: ParseOptions,
) -> SharedParse {
    let source_map = decoded.offset_map.source_map();
    let mut parse = parse_shared_source_text(
        Arc::from(decoded.text.into_owned()),
        source_map.clone(),
        decoded.encoding,
        decoded.diagnostics,
        options,
    );
    remap_shared_parse_ranges_with_mapper(
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
    if let Some(error) = max_bytes_error_for_bytes(input.len(), options.budgets) {
        return parse_budget_failure_empty(error);
    }
    parse_owned_decoded_source(decode_owned_bytes_auto(input), options)
}

fn parse_owned_bytes_with_encoding(
    input: Vec<u8>,
    encoding: SourceEncoding,
    options: ParseOptions,
) -> Parse {
    if let Some(error) = max_bytes_error_for_bytes(input.len(), options.budgets) {
        return parse_budget_failure_empty(error);
    }
    parse_owned_decoded_source(decode_owned_bytes_with_encoding(input, encoding), options)
}

pub(crate) fn budget_error_message(limit: &'static str) -> &'static str {
    match limit {
        "max_bytes" => "source exceeds parse budget: max_bytes",
        "max_tokens" => "source exceeds parse budget: max_tokens",
        "max_statements" => "source exceeds parse budget: max_statements",
        "max_nesting_depth" => "source exceeds parse budget: max_nesting_depth",
        "max_literal_bytes" => "source exceeds parse budget: max_literal_bytes",
        _ => "source exceeds parse budget",
    }
}

pub(crate) fn budget_error(limit: &'static str, range: TextRange) -> ParseError {
    ParseError {
        message: budget_error_message(limit),
        range,
    }
}

fn max_bytes_error_for_text(len: usize, budgets: ParseBudgets) -> Option<ParseError> {
    (len > budgets.max_bytes).then(|| budget_error("max_bytes", text_len_range(len)))
}

fn max_bytes_error_for_bytes(len: usize, budgets: ParseBudgets) -> Option<ParseError> {
    (len > budgets.max_bytes).then(|| budget_error("max_bytes", text_range(0, 0)))
}

fn max_bytes_error_for_file(path: &Path, budgets: ParseBudgets) -> io::Result<Option<ParseError>> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() > budgets.max_bytes as u64 => {
            Ok(Some(budget_error("max_bytes", text_range(0, 0))))
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(error),
        Err(_) => Ok(None),
    }
}

fn parse_budget_failure_for_source(source_text: String, error: ParseError) -> Parse {
    Parse {
        syntax: mel_ast::SourceFile { items: Vec::new() },
        source_map: SourceMap::identity(source_text.len()),
        source_text,
        source_encoding: SourceEncoding::Utf8,
        decode_errors: Vec::new(),
        lex_errors: Vec::new(),
        errors: vec![error],
    }
}

fn shared_parse_budget_failure_for_source(source_text: Arc<str>, error: ParseError) -> SharedParse {
    SharedParse {
        syntax: mel_ast::SourceFile { items: Vec::new() },
        source_map: SourceMap::identity(source_text.len()),
        source_text,
        source_encoding: SourceEncoding::Utf8,
        decode_errors: Vec::new(),
        lex_errors: Vec::new(),
        errors: vec![error],
    }
}

fn parse_budget_failure_empty(error: ParseError) -> Parse {
    Parse {
        syntax: mel_ast::SourceFile { items: Vec::new() },
        source_text: String::new(),
        source_map: SourceMap::identity(0),
        source_encoding: SourceEncoding::Utf8,
        decode_errors: Vec::new(),
        lex_errors: Vec::new(),
        errors: vec![error],
    }
}

fn shared_parse_budget_failure_empty(error: ParseError) -> SharedParse {
    SharedParse {
        syntax: mel_ast::SourceFile { items: Vec::new() },
        source_text: Arc::from(""),
        source_map: SourceMap::identity(0),
        source_encoding: SourceEncoding::Utf8,
        decode_errors: Vec::new(),
        lex_errors: Vec::new(),
        errors: vec![error],
    }
}

pub(crate) fn text_len_range(len: usize) -> TextRange {
    text_range(0, len.min(u32::MAX as usize) as u32)
}
