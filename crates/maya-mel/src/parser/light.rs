use std::borrow::Cow;
use std::sync::Arc;
use std::{fs, io, ops::Range, path::Path};

use encoding_rs::{Encoding, GBK, SHIFT_JIS};
use mel_syntax::{SourceMap, SourceView, TextRange, text_range};

use crate::{
    DecodeDiagnostic, ParseBudgets, ParseError, SourceEncoding, budget_error,
    decode::{OffsetMap, decode_source_auto, decode_source_with_encoding},
    text_len_range,
};

const DEFAULT_MAX_PREFIX_WORDS: usize = 64;
const DEFAULT_MAX_PREFIX_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightParseOptions {
    pub max_prefix_words: usize,
    pub max_prefix_bytes: usize,
    pub budgets: ParseBudgets,
}

impl Default for LightParseOptions {
    fn default() -> Self {
        Self {
            max_prefix_words: DEFAULT_MAX_PREFIX_WORDS,
            max_prefix_bytes: DEFAULT_MAX_PREFIX_BYTES,
            budgets: ParseBudgets::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LightSourceFile {
    pub items: Vec<LightItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightItem {
    Command(LightCommandSurface),
    Proc(LightProcSurface),
    Other { span: TextRange },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightProcSurface {
    pub name_range: Option<TextRange>,
    pub is_global: bool,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightCommandSurface {
    pub head_range: TextRange,
    pub captured: bool,
    pub words: Vec<LightWord>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightWord {
    Flag { text: TextRange, range: TextRange },
    NumericLiteral { text: TextRange, range: TextRange },
    BareWord { text: TextRange, range: TextRange },
    QuotedString { text: TextRange, range: TextRange },
    Variable { range: TextRange },
    GroupedExpr { range: TextRange },
    BraceList { range: TextRange },
    VectorLiteral { range: TextRange },
    Capture { range: TextRange },
}

impl LightWord {
    #[must_use]
    pub const fn range(&self) -> TextRange {
        match self {
            Self::Flag { range, .. }
            | Self::NumericLiteral { range, .. }
            | Self::BareWord { range, .. }
            | Self::QuotedString { range, .. }
            | Self::Variable { range }
            | Self::GroupedExpr { range }
            | Self::BraceList { range }
            | Self::VectorLiteral { range }
            | Self::Capture { range } => *range,
        }
    }
}

pub trait LightItemSink {
    fn on_item(&mut self, source: LightSourceView<'_>, item: LightItem);
}

impl<F> LightItemSink for F
where
    F: for<'a> FnMut(LightSourceView<'a>, LightItem),
{
    fn on_item(&mut self, source: LightSourceView<'_>, item: LightItem) {
        self(source, item);
    }
}

#[derive(Clone, Copy)]
pub enum LightSourceView<'a> {
    Text(SourceView<'a>),
    Bytes {
        bytes: &'a [u8],
        encoding: SourceEncoding,
    },
}

impl<'a> LightSourceView<'a> {
    #[must_use]
    pub fn raw_slice(self, range: TextRange) -> &'a [u8] {
        let start = usize::from(range.start()).min(self.len());
        let end = usize::from(range.end()).min(self.len()).max(start);
        match self {
            Self::Text(source) => &source.text().as_bytes()[start..end],
            Self::Bytes { bytes, .. } => &bytes[start..end],
        }
    }

    #[must_use]
    pub fn try_ascii_slice(self, range: TextRange) -> Option<&'a str> {
        std::str::from_utf8(self.raw_slice(range))
            .ok()
            .filter(|text| text.is_ascii())
    }

    #[must_use]
    pub fn decode_slice_lossy_for_preview(self, range: TextRange) -> Cow<'a, str> {
        match self {
            Self::Text(source) => Cow::Borrowed(source.slice(range)),
            Self::Bytes { bytes, encoding } => {
                decode_bytes_lossy(slice_range(bytes, range), encoding)
            }
        }
    }

    #[must_use]
    pub fn decode_slice(self, range: TextRange) -> DecodedLightSlice<'a> {
        match self {
            Self::Text(source) => DecodedLightSlice {
                text: Cow::Borrowed(source.slice(range)),
                diagnostics: Vec::new(),
            },
            Self::Bytes { bytes, encoding } => {
                decode_bytes_with_diagnostics(slice_range(bytes, range), encoding, range)
            }
        }
    }

    #[must_use]
    pub fn len(self) -> usize {
        match self {
            Self::Text(source) => source.text().len(),
            Self::Bytes { bytes, .. } => bytes.len(),
        }
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedLightSlice<'a> {
    pub text: Cow<'a, str>,
    pub diagnostics: Vec<DecodeDiagnostic>,
}

fn slice_range(bytes: &[u8], range: TextRange) -> &[u8] {
    let start = usize::from(range.start()).min(bytes.len());
    let end = usize::from(range.end()).min(bytes.len()).max(start);
    &bytes[start..end]
}

fn decode_bytes_lossy(bytes: &[u8], encoding: SourceEncoding) -> Cow<'_, str> {
    if matches!(encoding, SourceEncoding::Utf8) {
        return String::from_utf8_lossy(bytes);
    }
    encoding_rs_encoding(encoding).decode(bytes).0
}

fn decode_bytes_with_diagnostics(
    bytes: &[u8],
    encoding: SourceEncoding,
    range: TextRange,
) -> DecodedLightSlice<'_> {
    if matches!(encoding, SourceEncoding::Utf8) {
        return match std::str::from_utf8(bytes) {
            Ok(text) => DecodedLightSlice {
                text: Cow::Borrowed(text),
                diagnostics: Vec::new(),
            },
            Err(_) => DecodedLightSlice {
                text: String::from_utf8_lossy(bytes),
                diagnostics: vec![DecodeDiagnostic {
                    message: "source slice is not valid UTF-8; decoded lossily".into(),
                    range,
                }],
            },
        };
    }

    let (text, _, had_errors) = encoding_rs_encoding(encoding).decode(bytes);
    DecodedLightSlice {
        text,
        diagnostics: had_errors
            .then(|| DecodeDiagnostic {
                message: format!(
                    "source slice is not valid {}; decoded with replacement",
                    encoding.label()
                )
                .into(),
                range,
            })
            .into_iter()
            .collect(),
    }
}

fn encoding_rs_encoding(encoding: SourceEncoding) -> &'static Encoding {
    match encoding {
        SourceEncoding::Utf8 => encoding_rs::UTF_8,
        SourceEncoding::Cp932 => SHIFT_JIS,
        SourceEncoding::Gbk => GBK,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightScanReport {
    pub source_text: String,
    pub source_map: SourceMap,
    pub source_encoding: SourceEncoding,
    pub decode_errors: Vec<DecodeDiagnostic>,
    pub errors: Vec<ParseError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedLightScanReport {
    pub source_text: Arc<str>,
    pub source_map: SourceMap,
    pub source_encoding: SourceEncoding,
    pub decode_errors: Vec<DecodeDiagnostic>,
    pub errors: Vec<ParseError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightScanSummary {
    pub source_encoding: SourceEncoding,
    pub decode_errors: Vec<DecodeDiagnostic>,
    pub errors: Vec<ParseError>,
}

impl LightScanReport {
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

impl SharedLightScanReport {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightParse {
    pub source: LightSourceFile,
    pub source_text: String,
    pub source_map: SourceMap,
    pub source_encoding: SourceEncoding,
    pub decode_errors: Vec<DecodeDiagnostic>,
    pub errors: Vec<ParseError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedLightParse {
    pub source: LightSourceFile,
    pub source_text: Arc<str>,
    pub source_map: SourceMap,
    pub source_encoding: SourceEncoding,
    pub decode_errors: Vec<DecodeDiagnostic>,
    pub errors: Vec<ParseError>,
}

impl LightParse {
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

impl SharedLightParse {
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

impl From<(LightSourceFile, LightScanReport)> for LightParse {
    fn from((source, report): (LightSourceFile, LightScanReport)) -> Self {
        Self {
            source,
            source_text: report.source_text,
            source_map: report.source_map,
            source_encoding: report.source_encoding,
            decode_errors: report.decode_errors,
            errors: report.errors,
        }
    }
}

impl From<(LightSourceFile, SharedLightScanReport)> for SharedLightParse {
    fn from((source, report): (LightSourceFile, SharedLightScanReport)) -> Self {
        Self {
            source,
            source_text: report.source_text,
            source_map: report.source_map,
            source_encoding: report.source_encoding,
            decode_errors: report.decode_errors,
            errors: report.errors,
        }
    }
}

impl From<SharedLightScanReport> for LightScanReport {
    fn from(value: SharedLightScanReport) -> Self {
        Self {
            source_text: value.source_text.as_ref().to_owned(),
            source_map: value.source_map,
            source_encoding: value.source_encoding,
            decode_errors: value.decode_errors,
            errors: value.errors,
        }
    }
}

impl From<SharedLightParse> for LightParse {
    fn from(value: SharedLightParse) -> Self {
        Self {
            source: value.source,
            source_text: value.source_text.as_ref().to_owned(),
            source_map: value.source_map,
            source_encoding: value.source_encoding,
            decode_errors: value.decode_errors,
            errors: value.errors,
        }
    }
}

#[must_use]
pub fn parse_light_source(input: &str) -> LightParse {
    parse_light_source_with_options(input, LightParseOptions::default())
}

#[must_use]
pub fn parse_light_source_with_options(input: &str, options: LightParseOptions) -> LightParse {
    let mut sink = CollectLightItems::default();
    let report = scan_light_source_with_options_and_sink(input, options, &mut sink);
    LightParse::from((sink.finish(), report))
}

#[must_use]
pub fn parse_light_shared_source(input: Arc<str>) -> SharedLightParse {
    parse_light_shared_source_with_options(input, LightParseOptions::default())
}

#[must_use]
pub fn parse_light_shared_source_with_options(
    input: Arc<str>,
    options: LightParseOptions,
) -> SharedLightParse {
    let mut sink = CollectLightItems::default();
    let report =
        scan_light_shared_source_with_options_and_sink(Arc::clone(&input), options, &mut sink);
    SharedLightParse::from((sink.finish(), report))
}

pub fn scan_light_source_with_sink(input: &str, sink: &mut impl LightItemSink) -> LightScanReport {
    scan_light_source_with_options_and_sink(input, LightParseOptions::default(), sink)
}

pub fn scan_light_shared_source_with_sink(
    input: Arc<str>,
    sink: &mut impl LightItemSink,
) -> SharedLightScanReport {
    scan_light_shared_source_with_options_and_sink(input, LightParseOptions::default(), sink)
}

pub fn scan_light_shared_source_with_options_and_sink(
    input: Arc<str>,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> SharedLightScanReport {
    let input_len = input.len();
    if let Some(error) = max_bytes_error_for_text(input_len, options.budgets) {
        return SharedLightScanReport {
            source_text: input,
            source_map: SourceMap::identity(input_len),
            source_encoding: SourceEncoding::Utf8,
            decode_errors: Vec::new(),
            errors: vec![error],
        };
    }
    let source_map = SourceMap::identity(input_len);
    let source_view = LightSourceView::Text(SourceView::new(&input, &source_map));
    let mut scanner = LightScanner::new(&input, options);
    scanner.scan_with_sink(source_view, sink, None);
    let errors = scanner.errors;
    SharedLightScanReport {
        source_text: input,
        source_map,
        source_encoding: SourceEncoding::Utf8,
        decode_errors: Vec::new(),
        errors,
    }
}

pub fn scan_light_source_with_options_and_sink(
    input: &str,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> LightScanReport {
    if let Some(error) = max_bytes_error_for_text(input.len(), options.budgets) {
        return LightScanReport {
            source_text: input.to_owned(),
            source_map: SourceMap::identity(input.len()),
            source_encoding: SourceEncoding::Utf8,
            decode_errors: Vec::new(),
            errors: vec![error],
        };
    }
    let source_map = SourceMap::identity(input.len());
    let source_view = LightSourceView::Text(SourceView::new(input, &source_map));
    let mut scanner = LightScanner::new(input, options);
    scanner.scan_with_sink(source_view, sink, None);
    LightScanReport {
        source_text: input.to_owned(),
        source_map,
        source_encoding: SourceEncoding::Utf8,
        decode_errors: Vec::new(),
        errors: scanner.errors,
    }
}

#[must_use]
pub fn parse_light_bytes(input: &[u8]) -> LightParse {
    let decoded = decode_source_auto(input);
    let mut sink = CollectLightItems::default();
    let report = build_light_scan(decoded, LightParseOptions::default(), &mut sink);
    LightParse::from((sink.finish(), report))
}

#[must_use]
pub fn parse_light_shared_bytes(input: &[u8]) -> SharedLightParse {
    let decoded = decode_source_auto(input);
    let mut sink = CollectLightItems::default();
    let report = build_shared_light_scan(decoded, LightParseOptions::default(), &mut sink);
    SharedLightParse::from((sink.finish(), report))
}

#[must_use]
pub fn parse_light_bytes_with_encoding(input: &[u8], encoding: SourceEncoding) -> LightParse {
    let decoded = decode_source_with_encoding(input, encoding);
    let mut sink = CollectLightItems::default();
    let report = build_light_scan(decoded, LightParseOptions::default(), &mut sink);
    LightParse::from((sink.finish(), report))
}

#[must_use]
pub fn parse_light_shared_bytes_with_encoding(
    input: &[u8],
    encoding: SourceEncoding,
) -> SharedLightParse {
    let decoded = decode_source_with_encoding(input, encoding);
    let mut sink = CollectLightItems::default();
    let report = build_shared_light_scan(decoded, LightParseOptions::default(), &mut sink);
    SharedLightParse::from((sink.finish(), report))
}

pub fn scan_light_bytes_with_sink(input: &[u8], sink: &mut impl LightItemSink) -> LightScanReport {
    scan_light_bytes_with_options_and_sink(input, LightParseOptions::default(), sink)
}

pub fn scan_light_shared_bytes_with_options_and_sink(
    input: &[u8],
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> SharedLightScanReport {
    if let Some(error) = max_bytes_error_for_bytes(input.len(), options.budgets) {
        return empty_shared_light_scan_report(error);
    }
    let summary =
        scan_light_bytes_native(input, detect_light_source_encoding(input), options, sink);
    shared_report_from_summary(summary, input.len())
}

pub fn scan_light_bytes_with_options_and_sink(
    input: &[u8],
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> LightScanReport {
    if let Some(error) = max_bytes_error_for_bytes(input.len(), options.budgets) {
        return empty_light_scan_report(error);
    }
    let summary =
        scan_light_bytes_native(input, detect_light_source_encoding(input), options, sink);
    report_from_summary(summary, input.len())
}

pub fn scan_light_bytes_with_options_and_sink_and_then<S, T>(
    input: &[u8],
    options: LightParseOptions,
    sink: &mut S,
    then: impl for<'a> FnOnce(&mut S, LightSourceView<'a>, LightScanSummary) -> T,
) -> T
where
    S: LightItemSink,
{
    if let Some(error) = max_bytes_error_for_bytes(input.len(), options.budgets) {
        let source_map = SourceMap::identity(0);
        return then(
            sink,
            LightSourceView::Text(SourceView::new("", &source_map)),
            LightScanSummary {
                source_encoding: SourceEncoding::Utf8,
                decode_errors: Vec::new(),
                errors: vec![error],
            },
        );
    }
    let encoding = detect_light_source_encoding(input);
    let summary = scan_light_bytes_native(input, encoding, options, sink);
    then(
        sink,
        LightSourceView::Bytes {
            bytes: input,
            encoding,
        },
        summary,
    )
}

pub fn scan_light_bytes_with_encoding_and_sink(
    input: &[u8],
    encoding: SourceEncoding,
    sink: &mut impl LightItemSink,
) -> LightScanReport {
    scan_light_bytes_with_encoding_and_options_and_sink(
        input,
        encoding,
        LightParseOptions::default(),
        sink,
    )
}

pub fn scan_light_shared_bytes_with_encoding_and_options_and_sink(
    input: &[u8],
    encoding: SourceEncoding,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> SharedLightScanReport {
    if let Some(error) = max_bytes_error_for_bytes(input.len(), options.budgets) {
        return empty_shared_light_scan_report(error);
    }
    let summary = scan_light_bytes_native(input, encoding, options, sink);
    shared_report_from_summary(summary, input.len())
}

pub fn scan_light_bytes_with_encoding_and_options_and_sink(
    input: &[u8],
    encoding: SourceEncoding,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> LightScanReport {
    if let Some(error) = max_bytes_error_for_bytes(input.len(), options.budgets) {
        return empty_light_scan_report(error);
    }
    let summary = scan_light_bytes_native(input, encoding, options, sink);
    report_from_summary(summary, input.len())
}

pub fn parse_light_file(path: impl AsRef<Path>) -> io::Result<LightParse> {
    parse_light_file_with_options(path, LightParseOptions::default())
}

pub fn parse_light_file_with_options(
    path: impl AsRef<Path>,
    options: LightParseOptions,
) -> io::Result<LightParse> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), options.budgets)? {
        return Ok(LightParse::from((
            LightSourceFile::default(),
            empty_light_scan_report(error),
        )));
    }
    let bytes = fs::read(path)?;
    let decoded = decode_source_auto(&bytes);
    let mut sink = CollectLightItems::default();
    let report = build_light_scan(decoded, options, &mut sink);
    Ok(LightParse::from((sink.finish(), report)))
}

pub fn parse_light_shared_file(path: impl AsRef<Path>) -> io::Result<SharedLightParse> {
    if let Some(error) =
        max_bytes_error_for_file(path.as_ref(), LightParseOptions::default().budgets)?
    {
        return Ok(SharedLightParse::from((
            LightSourceFile::default(),
            empty_shared_light_scan_report(error),
        )));
    }
    let bytes = fs::read(path)?;
    Ok(parse_light_shared_bytes(&bytes))
}

pub fn parse_light_file_with_encoding(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
) -> io::Result<LightParse> {
    parse_light_file_with_encoding_and_options(path, encoding, LightParseOptions::default())
}

pub fn parse_light_file_with_encoding_and_options(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
    options: LightParseOptions,
) -> io::Result<LightParse> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), options.budgets)? {
        return Ok(LightParse::from((
            LightSourceFile::default(),
            empty_light_scan_report(error),
        )));
    }
    let bytes = fs::read(path)?;
    let decoded = decode_source_with_encoding(&bytes, encoding);
    let mut sink = CollectLightItems::default();
    let report = build_light_scan(decoded, options, &mut sink);
    Ok(LightParse::from((sink.finish(), report)))
}

pub fn parse_light_shared_file_with_encoding(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
) -> io::Result<SharedLightParse> {
    if let Some(error) =
        max_bytes_error_for_file(path.as_ref(), LightParseOptions::default().budgets)?
    {
        return Ok(SharedLightParse::from((
            LightSourceFile::default(),
            empty_shared_light_scan_report(error),
        )));
    }
    let bytes = fs::read(path)?;
    Ok(parse_light_shared_bytes_with_encoding(&bytes, encoding))
}

pub fn scan_light_file_with_sink(
    path: impl AsRef<Path>,
    sink: &mut impl LightItemSink,
) -> io::Result<LightScanReport> {
    scan_light_file_with_options_and_sink(path, LightParseOptions::default(), sink)
}

pub fn scan_light_shared_file_with_options_and_sink(
    path: impl AsRef<Path>,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> io::Result<SharedLightScanReport> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), options.budgets)? {
        return Ok(empty_shared_light_scan_report(error));
    }
    let bytes = fs::read(path)?;
    Ok(scan_light_shared_bytes_with_options_and_sink(
        &bytes, options, sink,
    ))
}

pub fn scan_light_file_with_options_and_sink(
    path: impl AsRef<Path>,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> io::Result<LightScanReport> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), options.budgets)? {
        return Ok(empty_light_scan_report(error));
    }
    let bytes = fs::read(path)?;
    Ok(scan_light_bytes_with_options_and_sink(
        &bytes, options, sink,
    ))
}

pub fn scan_light_file_with_encoding_and_sink(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
    sink: &mut impl LightItemSink,
) -> io::Result<LightScanReport> {
    scan_light_file_with_encoding_and_options_and_sink(
        path,
        encoding,
        LightParseOptions::default(),
        sink,
    )
}

pub fn scan_light_shared_file_with_encoding_and_options_and_sink(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> io::Result<SharedLightScanReport> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), options.budgets)? {
        return Ok(empty_shared_light_scan_report(error));
    }
    let bytes = fs::read(path)?;
    Ok(scan_light_shared_bytes_with_encoding_and_options_and_sink(
        &bytes, encoding, options, sink,
    ))
}

pub fn scan_light_file_with_encoding_and_options_and_sink(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> io::Result<LightScanReport> {
    if let Some(error) = max_bytes_error_for_file(path.as_ref(), options.budgets)? {
        return Ok(empty_light_scan_report(error));
    }
    let bytes = fs::read(path)?;
    Ok(scan_light_bytes_with_encoding_and_options_and_sink(
        &bytes, encoding, options, sink,
    ))
}

fn build_light_scan(
    decoded: crate::decode::DecodedSource<'_>,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> LightScanReport {
    let source_text = decoded.text.into_owned();
    let source_map = decoded.offset_map.source_map();
    let source_view = LightSourceView::Text(SourceView::new(&source_text, &source_map));
    let mut scanner = LightScanner::new(&source_text, options);
    scanner.scan_with_sink(source_view, sink, Some(&decoded.offset_map));
    let errors = scanner
        .errors
        .into_iter()
        .map(|mut error| {
            error.range = decoded.offset_map.map_range(error.range);
            error
        })
        .collect();
    LightScanReport {
        source_text,
        source_map,
        source_encoding: decoded.encoding,
        decode_errors: decoded.diagnostics,
        errors,
    }
}

fn build_shared_light_scan(
    decoded: crate::decode::DecodedSource<'_>,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> SharedLightScanReport {
    let source_text: Arc<str> = Arc::from(decoded.text.into_owned());
    let source_map = decoded.offset_map.source_map();
    let source_view = LightSourceView::Text(SourceView::new(&source_text, &source_map));
    let mut scanner = LightScanner::new(&source_text, options);
    scanner.scan_with_sink(source_view, sink, Some(&decoded.offset_map));
    let errors = scanner
        .errors
        .into_iter()
        .map(|mut error| {
            error.range = decoded.offset_map.map_range(error.range);
            error
        })
        .collect();
    SharedLightScanReport {
        source_text,
        source_map,
        source_encoding: decoded.encoding,
        decode_errors: decoded.diagnostics,
        errors,
    }
}

fn scan_light_bytes_native(
    input: &[u8],
    encoding: SourceEncoding,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> LightScanSummary {
    let mut scanner = ByteLightScanner::new(input, encoding, options);
    scanner.scan_with_sink(sink);
    LightScanSummary {
        source_encoding: encoding,
        decode_errors: Vec::new(),
        errors: scanner.errors,
    }
}

fn report_from_summary(summary: LightScanSummary, source_len: usize) -> LightScanReport {
    LightScanReport {
        source_text: String::new(),
        source_map: SourceMap::identity(source_len),
        source_encoding: summary.source_encoding,
        decode_errors: summary.decode_errors,
        errors: summary.errors,
    }
}

fn shared_report_from_summary(
    summary: LightScanSummary,
    source_len: usize,
) -> SharedLightScanReport {
    SharedLightScanReport {
        source_text: Arc::from(""),
        source_map: SourceMap::identity(source_len),
        source_encoding: summary.source_encoding,
        decode_errors: summary.decode_errors,
        errors: summary.errors,
    }
}

fn detect_light_source_encoding(input: &[u8]) -> SourceEncoding {
    if ascii_header_declares_codeset_932(input) {
        return SourceEncoding::Cp932;
    }
    if std::str::from_utf8(input).is_ok() {
        SourceEncoding::Utf8
    } else {
        SourceEncoding::Cp932
    }
}

fn ascii_header_declares_codeset_932(input: &[u8]) -> bool {
    let limit = input.len().min(4096);
    input[..limit]
        .windows(b"Codeset: 932".len())
        .any(|window| window.eq_ignore_ascii_case(b"Codeset: 932"))
}

#[derive(Default)]
struct CollectLightItems {
    items: Vec<LightItem>,
}

impl CollectLightItems {
    fn finish(self) -> LightSourceFile {
        LightSourceFile { items: self.items }
    }
}

impl LightItemSink for CollectLightItems {
    fn on_item(&mut self, _: LightSourceView<'_>, item: LightItem) {
        self.items.push(item);
    }
}

fn remap_light_item(item: &mut LightItem, map: &OffsetMap) {
    match item {
        LightItem::Command(command) => {
            command.head_range = map.map_range(command.head_range);
            if let Some(opaque_tail) = &mut command.opaque_tail {
                *opaque_tail = map.map_range(*opaque_tail);
            }
            for word in &mut command.words {
                match word {
                    LightWord::Flag { text, range }
                    | LightWord::NumericLiteral { text, range }
                    | LightWord::BareWord { text, range }
                    | LightWord::QuotedString { text, range } => {
                        *text = map.map_range(*text);
                        *range = map.map_range(*range);
                    }
                    LightWord::Variable { range }
                    | LightWord::GroupedExpr { range }
                    | LightWord::BraceList { range }
                    | LightWord::VectorLiteral { range }
                    | LightWord::Capture { range } => {
                        *range = map.map_range(*range);
                    }
                }
            }
            command.span = map.map_range(command.span);
        }
        LightItem::Proc(proc_def) => {
            if let Some(name_range) = &mut proc_def.name_range {
                *name_range = map.map_range(*name_range);
            }
            proc_def.span = map.map_range(proc_def.span);
        }
        LightItem::Other { span } => *span = map.map_range(*span),
    }
}

struct LightScanner<'a> {
    text: &'a str,
    options: LightParseOptions,
    errors: Vec<ParseError>,
    reported_unterminated_block_comment: bool,
    reported_budget_error: bool,
    budget: LightBudgetTracker,
}

impl<'a> LightScanner<'a> {
    fn new(text: &'a str, options: LightParseOptions) -> Self {
        Self {
            text,
            options,
            errors: Vec::new(),
            reported_unterminated_block_comment: false,
            reported_budget_error: false,
            budget: LightBudgetTracker::new(options.budgets),
        }
    }

    fn scan_with_sink(
        &mut self,
        source: LightSourceView<'_>,
        sink: &mut impl LightItemSink,
        remap: Option<&OffsetMap>,
    ) {
        let mut cursor = self.skip_trivia(0);

        while cursor < self.text.len() && !self.is_halted() {
            let (mut item, next_cursor) = if self.is_proc_start(cursor) {
                self.scan_proc_item(cursor)
            } else {
                self.scan_statement_item(cursor)
            };
            if self.is_halted() {
                break;
            }
            if !self.record_statement(start_range(&item)) {
                break;
            }
            if let Some(map) = remap {
                remap_light_item(&mut item, map);
            }
            sink.on_item(source, item);
            cursor = self.skip_trivia(next_cursor);
        }
    }

    fn scan_proc_item(&mut self, start: usize) -> (LightItem, usize) {
        let mut cursor = start;
        let mut is_global = false;
        if let Some(after_global) = self.consume_keyword(cursor, "global") {
            is_global = true;
            cursor = self.skip_trivia(after_global);
        }
        let after_proc = self.consume_keyword(cursor, "proc").unwrap_or(cursor);
        cursor = self.skip_trivia(after_proc);

        let first_word = self.scan_simple_word(cursor);
        let mut name_range = None;
        let mut body_scan_start = cursor;
        if let Some((first_start, first_end)) = first_word {
            let after_first = self.skip_trivia(first_end);
            body_scan_start = after_first;
            if self.peek_byte(after_first) == Some(b'(') {
                name_range = Some(text_range(first_start as u32, first_end as u32));
            } else if let Some((name_start, name_end)) = self.scan_simple_word(after_first) {
                name_range = Some(text_range(name_start as u32, name_end as u32));
                body_scan_start = self.skip_trivia(name_end);
            }
        }

        let end = self.scan_until_matching_body_end(start, body_scan_start);
        (
            LightItem::Proc(LightProcSurface {
                name_range,
                is_global,
                span: text_range(start as u32, end as u32),
            }),
            end,
        )
    }

    fn scan_statement_item(&mut self, start: usize) -> (LightItem, usize) {
        let Some((head_start, head_end)) = self.scan_simple_word(start) else {
            let end = self.scan_statement_tail(start);
            return (
                LightItem::Other {
                    span: text_range(start as u32, end as u32),
                },
                end,
            );
        };
        let head_range = text_range(head_start as u32, head_end as u32);
        let head_is_non_command = is_non_command_head(&self.text[head_start..head_end]);
        let after_head = self.skip_trivia(head_end);
        if self.peek_byte(after_head) == Some(b'(') || head_is_non_command {
            let end = self.scan_statement_tail(after_head);
            return (
                LightItem::Other {
                    span: text_range(start as u32, end as u32),
                },
                end,
            );
        }

        let (end, words, opaque_tail) =
            self.scan_command_statement_tail(start, head_end, after_head);

        (
            LightItem::Command(LightCommandSurface {
                head_range,
                captured: false,
                words,
                opaque_tail,
                span: text_range(start as u32, end as u32),
            }),
            end,
        )
    }

    fn scan_command_statement_tail(
        &mut self,
        start: usize,
        head_end: usize,
        after_head: usize,
    ) -> (usize, Vec<LightWord>, Option<TextRange>) {
        let mut words = Vec::with_capacity(self.options.max_prefix_words.min(8));
        let mut cursor = after_head;
        loop {
            cursor = self.skip_trivia(cursor);
            if cursor >= self.text.len() {
                return (self.text.len(), words, None);
            }
            if self.byte_at(cursor) == Some(b';') {
                let _ = self.record_token(cursor, cursor + 1);
                return (cursor + 1, words, None);
            }

            let consumed_bytes = cursor.saturating_sub(head_end);
            if words.len() >= self.options.max_prefix_words
                || consumed_bytes >= self.options.max_prefix_bytes
            {
                let end = self.scan_statement_tail(cursor);
                let body_end = self.statement_body_end(start, end);
                let opaque_tail =
                    (cursor < body_end).then(|| text_range(cursor as u32, body_end as u32));
                return (end, words, opaque_tail);
            }

            let Some((word, next_cursor)) = self.scan_word(cursor, self.text.len()) else {
                if self.is_halted() {
                    return (self.text.len(), words, None);
                }
                let end = self.scan_statement_tail(cursor);
                let body_end = self.statement_body_end(start, end);
                let opaque_tail =
                    (cursor < body_end).then(|| text_range(cursor as u32, body_end as u32));
                return (end, words, opaque_tail);
            };
            words.push(word);
            cursor = next_cursor;
        }
    }

    fn scan_statement_tail(&mut self, start: usize) -> usize {
        let mut cursor = start;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut in_string = false;
        let mut in_backquote = false;

        while cursor < self.text.len() && !self.is_halted() {
            if in_string {
                cursor = self.advance_string_body(cursor);
                in_string = false;
                continue;
            }
            if in_backquote {
                cursor = self.advance_backquote_body(cursor);
                in_backquote = false;
                continue;
            }
            if self.starts_with(cursor, "//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, "/*") {
                cursor = self.skip_block_comment(cursor);
                continue;
            }

            match self.byte_at(cursor) {
                Some(b'"') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.text.len();
                    }
                    in_string = true;
                    cursor += 1;
                }
                Some(b'`') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.text.len();
                    }
                    in_backquote = true;
                    cursor += 1;
                }
                Some(b'(') => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.text.len();
                    }
                    paren_depth += 1;
                    cursor += 1;
                }
                Some(b')') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.text.len();
                    }
                    if paren_depth > 0 {
                        self.exit_nesting();
                    }
                    paren_depth = paren_depth.saturating_sub(1);
                    cursor += 1;
                }
                Some(b'[') => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.text.len();
                    }
                    bracket_depth += 1;
                    cursor += 1;
                }
                Some(b']') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.text.len();
                    }
                    if bracket_depth > 0 {
                        self.exit_nesting();
                    }
                    bracket_depth = bracket_depth.saturating_sub(1);
                    cursor += 1;
                }
                Some(b'{') => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.text.len();
                    }
                    brace_depth += 1;
                    cursor += 1;
                }
                Some(b'}') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.text.len();
                    }
                    if brace_depth > 0 {
                        self.exit_nesting();
                    }
                    brace_depth = brace_depth.saturating_sub(1);
                    cursor += 1;
                }
                Some(b';')
                    if paren_depth == 0
                        && bracket_depth == 0
                        && brace_depth == 0
                        && !in_string
                        && !in_backquote =>
                {
                    let _ = self.record_token(cursor, cursor + 1);
                    return cursor + 1;
                }
                Some(ch) if (ch as char).is_whitespace() => cursor = self.next_offset(cursor),
                Some(_) => {
                    let end = self.scan_simple_word_until(cursor, self.text.len());
                    if end <= cursor {
                        if !self.record_token(cursor, self.next_offset(cursor)) {
                            return self.text.len();
                        }
                        cursor = self.next_offset(cursor);
                    } else {
                        if !self.record_token(cursor, end) {
                            return self.text.len();
                        }
                        cursor = end;
                    }
                }
                None => break,
            }
        }

        self.text.len()
    }

    fn statement_body_end(&self, start: usize, end: usize) -> usize {
        let mut body_end = end;
        if body_end > start && self.byte_at(body_end - 1) == Some(b';') {
            body_end -= 1;
        }
        while body_end > start {
            let prev = self.prev_offset(body_end);
            let segment = &self.text[prev..body_end];
            if segment.chars().all(char::is_whitespace) {
                body_end = prev;
                continue;
            }
            break;
        }
        body_end
    }

    fn scan_word(&mut self, start: usize, body_end: usize) -> Option<(LightWord, usize)> {
        if start >= body_end {
            return None;
        }
        if self.byte_at(start) == Some(b'"') {
            let end = self.scan_quoted_string(start);
            let range = text_range(start as u32, end as u32);
            if !self.check_literal(range) {
                return None;
            }
            return Some((LightWord::QuotedString { text: range, range }, end));
        }
        if self.byte_at(start) == Some(b'`') {
            let end = self.scan_backquote(start);
            let range = text_range(start as u32, end as u32);
            return Some((LightWord::Capture { range }, end));
        }
        if self.byte_at(start) == Some(b'{') {
            let end = self.scan_balanced(start, b'{', b'}');
            let range = text_range(start as u32, end as u32);
            if !self.check_literal(range) {
                return None;
            }
            return Some((LightWord::BraceList { range }, end));
        }
        if self.starts_with(start, "<<") {
            let end = self.scan_vector_literal(start);
            let range = text_range(start as u32, end as u32);
            if !self.check_literal(range) {
                return None;
            }
            return Some((LightWord::VectorLiteral { range }, end));
        }
        if self.byte_at(start) == Some(b'(') {
            let end = self.scan_balanced(start, b'(', b')');
            let range = text_range(start as u32, end as u32);
            if !self.check_literal(range) {
                return None;
            }
            return Some((LightWord::GroupedExpr { range }, end));
        }

        let end = self.scan_simple_word_until(start, body_end);
        if end <= start {
            return None;
        }
        if !self.record_token(start, end) {
            return None;
        }
        let range = text_range(start as u32, end as u32);
        let text = &self.text[start..end];
        let word = if text.starts_with('$') {
            LightWord::Variable { range }
        } else if text.starts_with('-') && text.len() > 1 {
            LightWord::Flag { text: range, range }
        } else if looks_numeric_like(text) {
            LightWord::NumericLiteral { text: range, range }
        } else {
            LightWord::BareWord { text: range, range }
        };
        Some((word, end))
    }

    fn scan_quoted_string(&mut self, start: usize) -> usize {
        let mut cursor = start + 1;
        while cursor < self.text.len() {
            match self.byte_at(cursor) {
                Some(b'\\') => {
                    cursor = self.next_offset(cursor + 1);
                }
                Some(b'"') => {
                    let end = cursor + 1;
                    let _ = self.record_token(start, end);
                    return end;
                }
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        if self.is_halted() {
            return self.text.len();
        }
        let _ = self.record_token(start, self.text.len());
        self.errors.push(ParseError {
            message: "unterminated string literal in lightweight surface parse",
            range: text_range(start as u32, self.text.len() as u32),
        });
        self.text.len()
    }

    fn scan_backquote(&mut self, start: usize) -> usize {
        let mut cursor = start + 1;
        while cursor < self.text.len() {
            match self.byte_at(cursor) {
                Some(b'\\') => {
                    cursor = self.next_offset(cursor + 1);
                }
                Some(b'`') => {
                    let end = cursor + 1;
                    let _ = self.record_token(start, end);
                    return end;
                }
                Some(b'"') => cursor = self.scan_quoted_string(cursor),
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        if self.is_halted() {
            return self.text.len();
        }
        let _ = self.record_token(start, self.text.len());
        self.errors.push(ParseError {
            message: "unterminated backquote capture in lightweight surface parse",
            range: text_range(start as u32, self.text.len() as u32),
        });
        self.text.len()
    }

    fn scan_balanced(&mut self, start: usize, open: u8, close: u8) -> usize {
        let mut cursor = start;
        let mut depth = 0usize;
        while cursor < self.text.len() && !self.is_halted() {
            if self.starts_with(cursor, "//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, "/*") {
                cursor = self.skip_block_comment(cursor);
                continue;
            }
            match self.byte_at(cursor) {
                Some(b'"') => cursor = self.scan_quoted_string(cursor),
                Some(b'`') => cursor = self.scan_backquote(cursor),
                Some(ch) if ch == open => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.text.len();
                    }
                    depth += 1;
                    cursor += 1;
                }
                Some(ch) if ch == close => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.text.len();
                    }
                    if depth > 0 {
                        self.exit_nesting();
                    }
                    depth = depth.saturating_sub(1);
                    cursor += 1;
                    if depth == 0 {
                        return cursor;
                    }
                }
                Some(b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.text.len();
                    }
                    cursor += 1;
                }
                Some(ch) if (ch as char).is_whitespace() => cursor = self.next_offset(cursor),
                Some(_) => {
                    let end = self.scan_simple_word_until(cursor, self.text.len());
                    if end <= cursor {
                        if !self.record_token(cursor, self.next_offset(cursor)) {
                            return self.text.len();
                        }
                        cursor = self.next_offset(cursor);
                    } else {
                        if !self.record_token(cursor, end) {
                            return self.text.len();
                        }
                        cursor = end;
                    }
                }
                None => break,
            }
        }
        if self.is_halted() {
            return self.text.len();
        }
        self.errors.push(ParseError {
            message: "unterminated grouped surface in lightweight parse",
            range: text_range(start as u32, self.text.len() as u32),
        });
        self.text.len()
    }

    fn scan_vector_literal(&mut self, start: usize) -> usize {
        let mut cursor = start + 2;
        if !self.record_token(start, start + 2) || !self.enter_nesting(start, start + 2) {
            return self.text.len();
        }
        while cursor < self.text.len() && !self.is_halted() {
            if self.starts_with(cursor, ">>") {
                let _ = self.record_token(cursor, cursor + 2);
                self.exit_nesting();
                return cursor + 2;
            }
            if self.byte_at(cursor) == Some(b'"') {
                cursor = self.scan_quoted_string(cursor);
                continue;
            }
            if self
                .byte_at(cursor)
                .is_some_and(|ch| (ch as char).is_whitespace())
            {
                cursor = self.next_offset(cursor);
                continue;
            }
            let end = self.scan_simple_word_until(cursor, self.text.len());
            if end <= cursor {
                let next = self.next_offset(cursor);
                if !self.record_token(cursor, next) {
                    return self.text.len();
                }
                cursor = next;
            } else {
                if !self.record_token(cursor, end) {
                    return self.text.len();
                }
                cursor = end;
            }
        }
        if self.is_halted() {
            return self.text.len();
        }
        self.errors.push(ParseError {
            message: "unterminated vector literal in lightweight parse",
            range: text_range(start as u32, self.text.len() as u32),
        });
        self.text.len()
    }

    fn scan_until_matching_body_end(&mut self, start: usize, cursor: usize) -> usize {
        let mut cursor = cursor;
        let mut depth = 0usize;
        let mut saw_body = false;
        while cursor < self.text.len() && !self.is_halted() {
            if self.starts_with(cursor, "//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, "/*") {
                cursor = self.skip_block_comment(cursor);
                continue;
            }
            match self.byte_at(cursor) {
                Some(b'"') => cursor = self.scan_quoted_string(cursor),
                Some(b'`') => cursor = self.scan_backquote(cursor),
                Some(b'{') => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.text.len();
                    }
                    saw_body = true;
                    depth += 1;
                    cursor += 1;
                }
                Some(b'}') if saw_body => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.text.len();
                    }
                    if depth > 0 {
                        self.exit_nesting();
                    }
                    depth = depth.saturating_sub(1);
                    cursor += 1;
                    if depth == 0 {
                        return cursor;
                    }
                }
                Some(b'(' | b')' | b'[' | b']' | b',' | b';') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.text.len();
                    }
                    cursor += 1;
                }
                Some(ch) if (ch as char).is_whitespace() => cursor = self.next_offset(cursor),
                Some(_) => {
                    let end = self.scan_simple_word_until(cursor, self.text.len());
                    if end <= cursor {
                        if !self.record_token(cursor, self.next_offset(cursor)) {
                            return self.text.len();
                        }
                        cursor = self.next_offset(cursor);
                    } else {
                        if !self.record_token(cursor, end) {
                            return self.text.len();
                        }
                        cursor = end;
                    }
                }
                None => break,
            }
        }
        if self.is_halted() {
            return self.text.len();
        }
        self.errors.push(ParseError {
            message: "unterminated proc body in lightweight surface parse",
            range: text_range(start as u32, self.text.len() as u32),
        });
        self.text.len()
    }

    fn scan_simple_word(&mut self, start: usize) -> Option<(usize, usize)> {
        let start = self.skip_trivia(start);
        let end = self.scan_simple_word_until(start, self.text.len());
        if end > start && !self.record_token(start, end) {
            return None;
        }
        (end > start).then_some((start, end))
    }

    fn scan_simple_word_until(&self, start: usize, body_end: usize) -> usize {
        let mut cursor = start;
        while cursor < body_end {
            if self.starts_with(cursor, "//") || self.starts_with(cursor, "/*") {
                break;
            }
            match self.byte_at(cursor) {
                Some(b';' | b'(' | b')' | b'{' | b'}' | b'[' | b']' | b'`' | b'"') | None => break,
                Some(ch) if (ch as char).is_whitespace() => break,
                Some(_) => cursor = self.next_offset(cursor),
            }
        }
        cursor
    }

    fn skip_trivia(&mut self, start: usize) -> usize {
        let mut cursor = start;
        while cursor < self.text.len() {
            if self.starts_with(cursor, "//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, "/*") {
                cursor = self.skip_block_comment(cursor);
                continue;
            }
            let Some(ch) = self.text[cursor..].chars().next() else {
                break;
            };
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
                continue;
            }
            break;
        }
        cursor
    }

    fn skip_trivia_peek(&self, start: usize) -> usize {
        let mut cursor = start;
        while cursor < self.text.len() {
            if self.starts_with(cursor, "//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, "/*") {
                let Some(after_comment) = self.skip_block_comment_peek(cursor) else {
                    return self.text.len();
                };
                cursor = after_comment;
                continue;
            }
            let Some(ch) = self.text[cursor..].chars().next() else {
                break;
            };
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
                continue;
            }
            break;
        }
        cursor
    }

    fn skip_line_comment(&self, start: usize) -> usize {
        let mut cursor = start + 2;
        while cursor < self.text.len() {
            match self.byte_at(cursor) {
                Some(b'\n') => return cursor + 1,
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        self.text.len()
    }

    fn skip_block_comment(&mut self, start: usize) -> usize {
        let mut cursor = start + 2;
        while cursor < self.text.len() {
            if self.starts_with(cursor, "*/") {
                return cursor + 2;
            }
            cursor = self.next_offset(cursor);
        }
        if !self.reported_unterminated_block_comment {
            self.errors.push(ParseError {
                message: "unterminated block comment",
                range: text_range(start as u32, self.text.len() as u32),
            });
            self.reported_unterminated_block_comment = true;
        }
        self.text.len()
    }

    fn skip_block_comment_peek(&self, start: usize) -> Option<usize> {
        let mut cursor = start + 2;
        while cursor < self.text.len() {
            if self.starts_with(cursor, "*/") {
                return Some(cursor + 2);
            }
            cursor = self.next_offset(cursor);
        }
        None
    }

    fn advance_string_body(&self, start: usize) -> usize {
        let mut cursor = start;
        while cursor < self.text.len() {
            match self.byte_at(cursor) {
                Some(b'\\') => cursor = self.next_offset(cursor + 1),
                Some(b'"') => return cursor + 1,
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        self.text.len()
    }

    fn advance_backquote_body(&self, start: usize) -> usize {
        let mut cursor = start;
        while cursor < self.text.len() {
            match self.byte_at(cursor) {
                Some(b'\\') => cursor = self.next_offset(cursor + 1),
                Some(b'`') => return cursor + 1,
                Some(b'"') => cursor = self.advance_string_body(cursor + 1),
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        self.text.len()
    }

    fn is_proc_start(&mut self, start: usize) -> bool {
        if self.peek_keyword_end(start, "proc").is_some() {
            return true;
        }
        let Some(after_global) = self.peek_keyword_end(start, "global") else {
            return false;
        };
        let after_global = self.skip_trivia_peek(after_global);
        self.peek_keyword_end(after_global, "proc").is_some()
    }

    fn peek_keyword_end(&self, start: usize, keyword: &str) -> Option<usize> {
        let cursor = self.skip_trivia_peek(start);
        if !self.text[cursor..].starts_with(keyword) {
            return None;
        }
        let end = cursor + keyword.len();
        let next = self.text[end..].chars().next();
        if next.is_some_and(is_word_continue) {
            return None;
        }
        Some(end)
    }

    fn consume_keyword(&mut self, start: usize, keyword: &str) -> Option<usize> {
        let cursor = self.skip_trivia(start);
        if !self.text[cursor..].starts_with(keyword) {
            return None;
        }
        let end = cursor + keyword.len();
        let next = self.text[end..].chars().next();
        if next.is_some_and(is_word_continue) {
            return None;
        }
        if !self.record_token(cursor, end) {
            return None;
        }
        Some(end)
    }

    fn starts_with(&self, start: usize, needle: &str) -> bool {
        self.text[start..].starts_with(needle)
    }

    fn byte_at(&self, offset: usize) -> Option<u8> {
        self.text.as_bytes().get(offset).copied()
    }

    fn peek_byte(&self, offset: usize) -> Option<u8> {
        self.byte_at(offset)
    }

    fn next_offset(&self, offset: usize) -> usize {
        self.text[offset..]
            .chars()
            .next()
            .map_or(self.text.len(), |ch| offset + ch.len_utf8())
    }

    fn prev_offset(&self, offset: usize) -> usize {
        let mut index = offset.saturating_sub(1);
        while !self.text.is_char_boundary(index) {
            index = index.saturating_sub(1);
        }
        index
    }

    fn is_halted(&self) -> bool {
        self.budget.halted
    }

    fn halt(&mut self, error: ParseError) {
        if self.reported_budget_error {
            return;
        }
        self.reported_budget_error = true;
        self.budget.halted = true;
        self.errors.push(error);
    }

    fn record_token(&mut self, start: usize, end: usize) -> bool {
        let range = text_range(start as u32, end as u32);
        if !self.budget.record_token() {
            self.halt(budget_error("max_tokens", range));
            return false;
        }
        true
    }

    fn record_statement(&mut self, range: TextRange) -> bool {
        if !self.budget.record_statement() {
            self.halt(budget_error("max_statements", range));
            return false;
        }
        true
    }

    fn enter_nesting(&mut self, start: usize, end: usize) -> bool {
        let range = text_range(start as u32, end as u32);
        if !self.budget.enter_nesting() {
            self.halt(budget_error("max_nesting_depth", range));
            return false;
        }
        true
    }

    fn exit_nesting(&mut self) {
        self.budget.exit_nesting();
    }

    fn check_literal(&mut self, range: TextRange) -> bool {
        if !self.budget.check_literal(usize::from(range.len())) {
            self.halt(budget_error("max_literal_bytes", range));
            return false;
        }
        true
    }
}

struct ByteLightScanner<'a> {
    bytes: &'a [u8],
    encoding: SourceEncoding,
    options: LightParseOptions,
    errors: Vec<ParseError>,
    reported_unterminated_block_comment: bool,
    reported_budget_error: bool,
    budget: LightBudgetTracker,
}

impl<'a> ByteLightScanner<'a> {
    fn new(bytes: &'a [u8], encoding: SourceEncoding, options: LightParseOptions) -> Self {
        Self {
            bytes,
            encoding,
            options,
            errors: Vec::new(),
            reported_unterminated_block_comment: false,
            reported_budget_error: false,
            budget: LightBudgetTracker::new(options.budgets),
        }
    }

    fn scan_with_sink(&mut self, sink: &mut impl LightItemSink) {
        let source = LightSourceView::Bytes {
            bytes: self.bytes,
            encoding: self.encoding,
        };
        let mut cursor = self.skip_trivia(0);
        while cursor < self.bytes.len() && !self.is_halted() {
            let (item, next_cursor) = if self.is_proc_start(cursor) {
                self.scan_proc_item(cursor)
            } else {
                self.scan_statement_item(cursor)
            };
            if self.is_halted() {
                break;
            }
            if !self.record_statement(start_range(&item)) {
                break;
            }
            sink.on_item(source, item);
            cursor = self.skip_trivia(next_cursor);
        }
    }

    fn scan_proc_item(&mut self, start: usize) -> (LightItem, usize) {
        let mut cursor = start;
        let mut is_global = false;
        if let Some(after_global) = self.consume_keyword(cursor, b"global") {
            is_global = true;
            cursor = self.skip_trivia(after_global);
        }
        let after_proc = self.consume_keyword(cursor, b"proc").unwrap_or(cursor);
        cursor = self.skip_trivia(after_proc);
        let first_word = self.scan_simple_word(cursor);
        let mut name_range = None;
        let mut body_scan_start = cursor;
        if let Some((first_start, first_end)) = first_word {
            let after_first = self.skip_trivia(first_end);
            body_scan_start = after_first;
            if self.peek_byte(after_first) == Some(b'(') {
                name_range = Some(text_range(first_start as u32, first_end as u32));
            } else if let Some((name_start, name_end)) = self.scan_simple_word(after_first) {
                name_range = Some(text_range(name_start as u32, name_end as u32));
                body_scan_start = self.skip_trivia(name_end);
            }
        }
        let end = self.scan_until_matching_body_end(start, body_scan_start);
        (
            LightItem::Proc(LightProcSurface {
                name_range,
                is_global,
                span: text_range(start as u32, end as u32),
            }),
            end,
        )
    }

    fn scan_statement_item(&mut self, start: usize) -> (LightItem, usize) {
        let Some((head_start, head_end)) = self.scan_simple_word(start) else {
            let end = self.scan_statement_tail(start);
            return (
                LightItem::Other {
                    span: text_range(start as u32, end as u32),
                },
                end,
            );
        };
        let head_range = text_range(head_start as u32, head_end as u32);
        let head_is_non_command = is_non_command_head_bytes(&self.bytes[head_start..head_end]);
        let after_head = self.skip_trivia(head_end);
        if self.peek_byte(after_head) == Some(b'(') || head_is_non_command {
            let end = self.scan_statement_tail(after_head);
            return (
                LightItem::Other {
                    span: text_range(start as u32, end as u32),
                },
                end,
            );
        }
        let (end, words, opaque_tail) =
            self.scan_command_statement_tail(start, head_end, after_head);
        (
            LightItem::Command(LightCommandSurface {
                head_range,
                captured: false,
                words,
                opaque_tail,
                span: text_range(start as u32, end as u32),
            }),
            end,
        )
    }

    fn scan_command_statement_tail(
        &mut self,
        start: usize,
        head_end: usize,
        after_head: usize,
    ) -> (usize, Vec<LightWord>, Option<TextRange>) {
        let mut words = Vec::with_capacity(self.options.max_prefix_words.min(8));
        let mut cursor = after_head;
        loop {
            cursor = self.skip_trivia(cursor);
            if cursor >= self.bytes.len() {
                return (self.bytes.len(), words, None);
            }
            if self.byte_at(cursor) == Some(b';') {
                let _ = self.record_token(cursor, cursor + 1);
                return (cursor + 1, words, None);
            }
            if words.len() >= self.options.max_prefix_words
                || cursor.saturating_sub(head_end) >= self.options.max_prefix_bytes
            {
                let end = self.scan_statement_tail(cursor);
                let body_end = self.statement_body_end(start, end);
                let opaque_tail =
                    (cursor < body_end).then(|| text_range(cursor as u32, body_end as u32));
                return (end, words, opaque_tail);
            }
            let Some((word, next_cursor)) = self.scan_word(cursor, self.bytes.len()) else {
                if self.is_halted() {
                    return (self.bytes.len(), words, None);
                }
                let end = self.scan_statement_tail(cursor);
                let body_end = self.statement_body_end(start, end);
                let opaque_tail =
                    (cursor < body_end).then(|| text_range(cursor as u32, body_end as u32));
                return (end, words, opaque_tail);
            };
            words.push(word);
            cursor = next_cursor;
        }
    }

    fn scan_statement_tail(&mut self, start: usize) -> usize {
        let mut cursor = start;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        while cursor < self.bytes.len() && !self.is_halted() {
            if self.starts_with(cursor, b"//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, b"/*") {
                cursor = self.skip_block_comment(cursor);
                continue;
            }
            match self.byte_at(cursor) {
                Some(b'"') => cursor = self.scan_quoted_string(cursor),
                Some(b'`') => cursor = self.scan_backquote(cursor),
                Some(b'(') => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.bytes.len();
                    }
                    paren_depth += 1;
                    cursor += 1;
                }
                Some(b')') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.bytes.len();
                    }
                    if paren_depth > 0 {
                        self.exit_nesting();
                    }
                    paren_depth = paren_depth.saturating_sub(1);
                    cursor += 1;
                }
                Some(b'[') => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.bytes.len();
                    }
                    bracket_depth += 1;
                    cursor += 1;
                }
                Some(b']') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.bytes.len();
                    }
                    if bracket_depth > 0 {
                        self.exit_nesting();
                    }
                    bracket_depth = bracket_depth.saturating_sub(1);
                    cursor += 1;
                }
                Some(b'{') => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.bytes.len();
                    }
                    brace_depth += 1;
                    cursor += 1;
                }
                Some(b'}') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.bytes.len();
                    }
                    if brace_depth > 0 {
                        self.exit_nesting();
                    }
                    brace_depth = brace_depth.saturating_sub(1);
                    cursor += 1;
                }
                Some(b';') if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    let _ = self.record_token(cursor, cursor + 1);
                    return cursor + 1;
                }
                Some(ch) if is_ascii_ws(ch) => cursor += 1,
                Some(_) => {
                    let end = self.scan_simple_word_until(cursor, self.bytes.len());
                    if end <= cursor {
                        let next = self.next_offset(cursor);
                        if !self.record_token(cursor, next) {
                            return self.bytes.len();
                        }
                        cursor = next;
                    } else {
                        if !self.record_token(cursor, end) {
                            return self.bytes.len();
                        }
                        cursor = end;
                    }
                }
                None => break,
            }
        }
        self.bytes.len()
    }

    fn statement_body_end(&self, start: usize, end: usize) -> usize {
        let mut body_end = end;
        if body_end > start && self.byte_at(body_end - 1) == Some(b';') {
            body_end -= 1;
        }
        while body_end > start && self.byte_at(body_end - 1).is_some_and(is_ascii_ws) {
            body_end -= 1;
        }
        body_end
    }

    fn scan_word(&mut self, start: usize, body_end: usize) -> Option<(LightWord, usize)> {
        if start >= body_end {
            return None;
        }
        if self.byte_at(start) == Some(b'"') {
            let end = self.scan_quoted_string(start);
            let range = text_range(start as u32, end as u32);
            if !self.check_literal(range) {
                return None;
            }
            return Some((LightWord::QuotedString { text: range, range }, end));
        }
        if self.byte_at(start) == Some(b'`') {
            let end = self.scan_backquote(start);
            let range = text_range(start as u32, end as u32);
            return Some((LightWord::Capture { range }, end));
        }
        if self.byte_at(start) == Some(b'{') {
            let end = self.scan_balanced(start, b'{', b'}');
            let range = text_range(start as u32, end as u32);
            if !self.check_literal(range) {
                return None;
            }
            return Some((LightWord::BraceList { range }, end));
        }
        if self.starts_with(start, b"<<") {
            let end = self.scan_vector_literal(start);
            let range = text_range(start as u32, end as u32);
            if !self.check_literal(range) {
                return None;
            }
            return Some((LightWord::VectorLiteral { range }, end));
        }
        if self.byte_at(start) == Some(b'(') {
            let end = self.scan_balanced(start, b'(', b')');
            let range = text_range(start as u32, end as u32);
            if !self.check_literal(range) {
                return None;
            }
            return Some((LightWord::GroupedExpr { range }, end));
        }
        let end = self.scan_simple_word_until(start, body_end);
        if end <= start {
            return None;
        }
        if !self.record_token(start, end) {
            return None;
        }
        let range = text_range(start as u32, end as u32);
        let text = &self.bytes[start..end];
        let word = if text.starts_with(b"$") {
            LightWord::Variable { range }
        } else if text.starts_with(b"-") && text.len() > 1 {
            LightWord::Flag { text: range, range }
        } else if looks_numeric_like_bytes(text) {
            LightWord::NumericLiteral { text: range, range }
        } else {
            LightWord::BareWord { text: range, range }
        };
        Some((word, end))
    }

    fn scan_quoted_string(&mut self, start: usize) -> usize {
        let mut cursor = start + 1;
        while cursor < self.bytes.len() {
            match self.byte_at(cursor) {
                Some(b'\\') => cursor = self.next_offset(cursor + 1),
                Some(b'"') => {
                    let end = cursor + 1;
                    let _ = self.record_token(start, end);
                    return end;
                }
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        if self.is_halted() {
            return self.bytes.len();
        }
        let _ = self.record_token(start, self.bytes.len());
        self.errors.push(ParseError {
            message: "unterminated string literal in lightweight surface parse",
            range: text_range(start as u32, self.bytes.len() as u32),
        });
        self.bytes.len()
    }

    fn scan_backquote(&mut self, start: usize) -> usize {
        let mut cursor = start + 1;
        while cursor < self.bytes.len() {
            match self.byte_at(cursor) {
                Some(b'\\') => cursor = self.next_offset(cursor + 1),
                Some(b'`') => {
                    let end = cursor + 1;
                    let _ = self.record_token(start, end);
                    return end;
                }
                Some(b'"') => cursor = self.scan_quoted_string(cursor),
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        if self.is_halted() {
            return self.bytes.len();
        }
        let _ = self.record_token(start, self.bytes.len());
        self.errors.push(ParseError {
            message: "unterminated backquote capture in lightweight surface parse",
            range: text_range(start as u32, self.bytes.len() as u32),
        });
        self.bytes.len()
    }

    fn scan_balanced(&mut self, start: usize, open: u8, close: u8) -> usize {
        let mut cursor = start;
        let mut depth = 0usize;
        while cursor < self.bytes.len() && !self.is_halted() {
            if self.starts_with(cursor, b"//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, b"/*") {
                cursor = self.skip_block_comment(cursor);
                continue;
            }
            match self.byte_at(cursor) {
                Some(b'"') => cursor = self.scan_quoted_string(cursor),
                Some(b'`') => cursor = self.scan_backquote(cursor),
                Some(ch) if ch == open => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.bytes.len();
                    }
                    depth += 1;
                    cursor += 1;
                }
                Some(ch) if ch == close => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.bytes.len();
                    }
                    if depth > 0 {
                        self.exit_nesting();
                    }
                    depth = depth.saturating_sub(1);
                    cursor += 1;
                    if depth == 0 {
                        return cursor;
                    }
                }
                Some(b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.bytes.len();
                    }
                    cursor += 1;
                }
                Some(ch) if is_ascii_ws(ch) => cursor += 1,
                Some(_) => {
                    let end = self.scan_simple_word_until(cursor, self.bytes.len());
                    if end <= cursor {
                        let next = self.next_offset(cursor);
                        if !self.record_token(cursor, next) {
                            return self.bytes.len();
                        }
                        cursor = next;
                    } else {
                        if !self.record_token(cursor, end) {
                            return self.bytes.len();
                        }
                        cursor = end;
                    }
                }
                None => break,
            }
        }
        if self.is_halted() {
            return self.bytes.len();
        }
        self.errors.push(ParseError {
            message: "unterminated grouped surface in lightweight parse",
            range: text_range(start as u32, self.bytes.len() as u32),
        });
        self.bytes.len()
    }

    fn scan_vector_literal(&mut self, start: usize) -> usize {
        let mut cursor = start + 2;
        if !self.record_token(start, start + 2) || !self.enter_nesting(start, start + 2) {
            return self.bytes.len();
        }
        while cursor < self.bytes.len() && !self.is_halted() {
            if self.starts_with(cursor, b">>") {
                let _ = self.record_token(cursor, cursor + 2);
                self.exit_nesting();
                return cursor + 2;
            }
            if self.byte_at(cursor) == Some(b'"') {
                cursor = self.scan_quoted_string(cursor);
                continue;
            }
            if self.byte_at(cursor).is_some_and(is_ascii_ws) {
                cursor += 1;
                continue;
            }
            let end = self.scan_simple_word_until(cursor, self.bytes.len());
            if end <= cursor {
                let next = self.next_offset(cursor);
                if !self.record_token(cursor, next) {
                    return self.bytes.len();
                }
                cursor = next;
            } else {
                if !self.record_token(cursor, end) {
                    return self.bytes.len();
                }
                cursor = end;
            }
        }
        if self.is_halted() {
            return self.bytes.len();
        }
        self.errors.push(ParseError {
            message: "unterminated vector literal in lightweight parse",
            range: text_range(start as u32, self.bytes.len() as u32),
        });
        self.bytes.len()
    }

    fn scan_until_matching_body_end(&mut self, start: usize, cursor: usize) -> usize {
        let mut cursor = cursor;
        let mut depth = 0usize;
        let mut saw_body = false;
        while cursor < self.bytes.len() && !self.is_halted() {
            if self.starts_with(cursor, b"//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, b"/*") {
                cursor = self.skip_block_comment(cursor);
                continue;
            }
            match self.byte_at(cursor) {
                Some(b'"') => cursor = self.scan_quoted_string(cursor),
                Some(b'`') => cursor = self.scan_backquote(cursor),
                Some(b'{') => {
                    if !self.record_token(cursor, cursor + 1)
                        || !self.enter_nesting(cursor, cursor + 1)
                    {
                        return self.bytes.len();
                    }
                    saw_body = true;
                    depth += 1;
                    cursor += 1;
                }
                Some(b'}') if saw_body => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.bytes.len();
                    }
                    if depth > 0 {
                        self.exit_nesting();
                    }
                    depth = depth.saturating_sub(1);
                    cursor += 1;
                    if depth == 0 {
                        return cursor;
                    }
                }
                Some(b'(' | b')' | b'[' | b']' | b',' | b';') => {
                    if !self.record_token(cursor, cursor + 1) {
                        return self.bytes.len();
                    }
                    cursor += 1;
                }
                Some(ch) if is_ascii_ws(ch) => cursor += 1,
                Some(_) => {
                    let end = self.scan_simple_word_until(cursor, self.bytes.len());
                    if end <= cursor {
                        let next = self.next_offset(cursor);
                        if !self.record_token(cursor, next) {
                            return self.bytes.len();
                        }
                        cursor = next;
                    } else {
                        if !self.record_token(cursor, end) {
                            return self.bytes.len();
                        }
                        cursor = end;
                    }
                }
                None => break,
            }
        }
        if self.is_halted() {
            return self.bytes.len();
        }
        self.errors.push(ParseError {
            message: "unterminated proc body in lightweight surface parse",
            range: text_range(start as u32, self.bytes.len() as u32),
        });
        self.bytes.len()
    }

    fn scan_simple_word(&mut self, start: usize) -> Option<(usize, usize)> {
        let start = self.skip_trivia(start);
        let end = self.scan_simple_word_until(start, self.bytes.len());
        if end > start && !self.record_token(start, end) {
            return None;
        }
        (end > start).then_some((start, end))
    }

    fn scan_simple_word_until(&self, start: usize, body_end: usize) -> usize {
        let mut cursor = start;
        while cursor < body_end {
            if self.starts_with(cursor, b"//") || self.starts_with(cursor, b"/*") {
                break;
            }
            match self.byte_at(cursor) {
                Some(b';' | b'(' | b')' | b'{' | b'}' | b'[' | b']' | b'`' | b'"') | None => break,
                Some(ch) if is_ascii_ws(ch) => break,
                Some(_) => cursor = self.next_offset(cursor),
            }
        }
        cursor
    }

    fn skip_trivia(&mut self, start: usize) -> usize {
        let mut cursor = start;
        while cursor < self.bytes.len() {
            if self.starts_with(cursor, b"//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, b"/*") {
                cursor = self.skip_block_comment(cursor);
                continue;
            }
            if self.byte_at(cursor).is_some_and(is_ascii_ws) {
                cursor += 1;
                continue;
            }
            break;
        }
        cursor
    }

    fn skip_trivia_peek(&self, start: usize) -> usize {
        let mut cursor = start;
        while cursor < self.bytes.len() {
            if self.starts_with(cursor, b"//") {
                cursor = self.skip_line_comment(cursor);
                continue;
            }
            if self.starts_with(cursor, b"/*") {
                let Some(after_comment) = self.skip_block_comment_peek(cursor) else {
                    return self.bytes.len();
                };
                cursor = after_comment;
                continue;
            }
            if self.byte_at(cursor).is_some_and(is_ascii_ws) {
                cursor += 1;
                continue;
            }
            break;
        }
        cursor
    }

    fn skip_line_comment(&self, start: usize) -> usize {
        let mut cursor = start + 2;
        while cursor < self.bytes.len() {
            match self.byte_at(cursor) {
                Some(b'\n') => return cursor + 1,
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        self.bytes.len()
    }

    fn skip_block_comment(&mut self, start: usize) -> usize {
        let mut cursor = start + 2;
        while cursor < self.bytes.len() {
            if self.starts_with(cursor, b"*/") {
                return cursor + 2;
            }
            cursor = self.next_offset(cursor);
        }
        if !self.reported_unterminated_block_comment {
            self.errors.push(ParseError {
                message: "unterminated block comment",
                range: text_range(start as u32, self.bytes.len() as u32),
            });
            self.reported_unterminated_block_comment = true;
        }
        self.bytes.len()
    }

    fn skip_block_comment_peek(&self, start: usize) -> Option<usize> {
        let mut cursor = start + 2;
        while cursor < self.bytes.len() {
            if self.starts_with(cursor, b"*/") {
                return Some(cursor + 2);
            }
            cursor = self.next_offset(cursor);
        }
        None
    }

    fn is_proc_start(&self, start: usize) -> bool {
        if self.peek_keyword_end(start, b"proc").is_some() {
            return true;
        }
        let Some(after_global) = self.peek_keyword_end(start, b"global") else {
            return false;
        };
        let after_global = self.skip_trivia_peek(after_global);
        self.peek_keyword_end(after_global, b"proc").is_some()
    }

    fn peek_keyword_end(&self, start: usize, keyword: &[u8]) -> Option<usize> {
        let cursor = self.skip_trivia_peek(start);
        if !self.bytes.get(cursor..)?.starts_with(keyword) {
            return None;
        }
        let end = cursor + keyword.len();
        if self.byte_at(end).is_some_and(is_word_continue_byte) {
            return None;
        }
        Some(end)
    }

    fn consume_keyword(&mut self, start: usize, keyword: &[u8]) -> Option<usize> {
        let cursor = self.skip_trivia(start);
        if !self.bytes.get(cursor..)?.starts_with(keyword) {
            return None;
        }
        let end = cursor + keyword.len();
        if self.byte_at(end).is_some_and(is_word_continue_byte) {
            return None;
        }
        if !self.record_token(cursor, end) {
            return None;
        }
        Some(end)
    }

    fn starts_with(&self, start: usize, needle: &[u8]) -> bool {
        self.bytes
            .get(start..)
            .is_some_and(|bytes| bytes.starts_with(needle))
    }

    fn byte_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(offset).copied()
    }

    fn peek_byte(&self, offset: usize) -> Option<u8> {
        self.byte_at(offset)
    }

    fn next_offset(&self, offset: usize) -> usize {
        if offset >= self.bytes.len() {
            return self.bytes.len();
        }
        let first = self.bytes[offset];
        if first < 0x80 {
            return offset + 1;
        }
        let len = match self.encoding {
            SourceEncoding::Utf8 => utf8_sequence_len(first, &self.bytes[offset..]),
            SourceEncoding::Cp932 => multibyte_len_cp932(first, self.byte_at(offset + 1)),
            SourceEncoding::Gbk => multibyte_len_gbk(first, self.byte_at(offset + 1)),
        };
        offset.saturating_add(len).min(self.bytes.len())
    }

    fn is_halted(&self) -> bool {
        self.budget.halted
    }

    fn halt(&mut self, error: ParseError) {
        if self.reported_budget_error {
            return;
        }
        self.reported_budget_error = true;
        self.budget.halted = true;
        self.errors.push(error);
    }

    fn record_token(&mut self, start: usize, end: usize) -> bool {
        let range = text_range(start as u32, end as u32);
        if !self.budget.record_token() {
            self.halt(budget_error("max_tokens", range));
            return false;
        }
        true
    }

    fn record_statement(&mut self, range: TextRange) -> bool {
        if !self.budget.record_statement() {
            self.halt(budget_error("max_statements", range));
            return false;
        }
        true
    }

    fn enter_nesting(&mut self, start: usize, end: usize) -> bool {
        let range = text_range(start as u32, end as u32);
        if !self.budget.enter_nesting() {
            self.halt(budget_error("max_nesting_depth", range));
            return false;
        }
        true
    }

    fn exit_nesting(&mut self) {
        self.budget.exit_nesting();
    }

    fn check_literal(&mut self, range: TextRange) -> bool {
        if !self.budget.check_literal(usize::from(range.len())) {
            self.halt(budget_error("max_literal_bytes", range));
            return false;
        }
        true
    }
}

#[derive(Debug, Clone, Copy)]
struct LightBudgetTracker {
    max_nesting_depth: usize,
    max_literal_bytes: usize,
    remaining_tokens: usize,
    remaining_statements: usize,
    remaining_nesting: usize,
    halted: bool,
}

impl LightBudgetTracker {
    fn new(budgets: ParseBudgets) -> Self {
        Self {
            max_nesting_depth: budgets.max_nesting_depth,
            max_literal_bytes: budgets.max_literal_bytes,
            remaining_tokens: budgets.max_tokens,
            remaining_statements: budgets.max_statements,
            remaining_nesting: budgets.max_nesting_depth,
            halted: false,
        }
    }

    fn record_token(&mut self) -> bool {
        if self.remaining_tokens == 0 {
            self.halted = true;
            return false;
        }
        self.remaining_tokens -= 1;
        true
    }

    fn record_statement(&mut self) -> bool {
        if self.remaining_statements == 0 {
            self.halted = true;
            return false;
        }
        self.remaining_statements -= 1;
        true
    }

    fn enter_nesting(&mut self) -> bool {
        if self.remaining_nesting == 0 {
            self.halted = true;
            return false;
        }
        self.remaining_nesting -= 1;
        true
    }

    fn exit_nesting(&mut self) {
        if self.remaining_nesting < self.max_nesting_depth {
            self.remaining_nesting += 1;
        }
    }

    fn check_literal(&mut self, len: usize) -> bool {
        if len > self.max_literal_bytes {
            self.halted = true;
            return false;
        }
        true
    }
}

fn start_range(item: &LightItem) -> TextRange {
    match item {
        LightItem::Command(command) => command.span,
        LightItem::Proc(proc_def) => proc_def.span,
        LightItem::Other { span } => *span,
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

fn empty_light_scan_report(error: ParseError) -> LightScanReport {
    LightScanReport {
        source_text: String::new(),
        source_map: SourceMap::identity(0),
        source_encoding: SourceEncoding::Utf8,
        decode_errors: Vec::new(),
        errors: vec![error],
    }
}

fn empty_shared_light_scan_report(error: ParseError) -> SharedLightScanReport {
    SharedLightScanReport {
        source_text: Arc::from(""),
        source_map: SourceMap::identity(0),
        source_encoding: SourceEncoding::Utf8,
        decode_errors: Vec::new(),
        errors: vec![error],
    }
}

fn is_word_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$')
}

fn is_word_continue_byte(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$')
}

fn is_ascii_ws(ch: u8) -> bool {
    matches!(ch, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

fn is_non_command_head(head: &str) -> bool {
    matches!(
        head,
        "global"
            | "proc"
            | "if"
            | "while"
            | "do"
            | "for"
            | "switch"
            | "return"
            | "break"
            | "continue"
            | "int"
            | "float"
            | "string"
            | "vector"
            | "matrix"
    )
}

fn is_non_command_head_bytes(head: &[u8]) -> bool {
    matches!(
        head,
        b"global"
            | b"proc"
            | b"if"
            | b"while"
            | b"do"
            | b"for"
            | b"switch"
            | b"return"
            | b"break"
            | b"continue"
            | b"int"
            | b"float"
            | b"string"
            | b"vector"
            | b"matrix"
    )
}

fn looks_numeric_like(text: &str) -> bool {
    let trimmed = text.strip_prefix(['+', '-']).unwrap_or(text);
    if trimmed.is_empty() {
        return false;
    }
    trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        || (trimmed.starts_with('.')
            && trimmed[1..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit()))
}

fn looks_numeric_like_bytes(text: &[u8]) -> bool {
    let trimmed = text
        .strip_prefix(b"+")
        .or_else(|| text.strip_prefix(b"-"))
        .unwrap_or(text);
    if trimmed.is_empty() {
        return false;
    }
    trimmed[0].is_ascii_digit()
        || (trimmed.starts_with(b".") && trimmed.get(1).is_some_and(|ch| ch.is_ascii_digit()))
}

fn utf8_sequence_len(first: u8, bytes: &[u8]) -> usize {
    let len = if first & 0b1110_0000 == 0b1100_0000 {
        2
    } else if first & 0b1111_0000 == 0b1110_0000 {
        3
    } else if first & 0b1111_1000 == 0b1111_0000 {
        4
    } else {
        1
    };
    if bytes.len() >= len
        && bytes[1..len]
            .iter()
            .all(|byte| byte & 0b1100_0000 == 0b1000_0000)
    {
        len
    } else {
        1
    }
}

fn multibyte_len_cp932(first: u8, second: Option<u8>) -> usize {
    let is_lead = matches!(first, 0x81..=0x9F | 0xE0..=0xFC);
    let is_trail = second.is_some_and(|byte| matches!(byte, 0x40..=0x7E | 0x80..=0xFC));
    if is_lead && is_trail { 2 } else { 1 }
}

fn multibyte_len_gbk(first: u8, second: Option<u8>) -> usize {
    let is_lead = matches!(first, 0x81..=0xFE);
    let is_trail = second.is_some_and(|byte| matches!(byte, 0x40..=0x7E | 0x80..=0xFE));
    if is_lead && is_trail { 2 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::parse_light_source;
    use mel_syntax::text_range;

    #[test]
    fn unterminated_block_comment_reports_light_parse_error() {
        let parse = parse_light_source("createNode file -n \"f\";\n/* hidden tail");

        assert_eq!(parse.source.items.len(), 1);
        assert_eq!(parse.errors.len(), 1);
        assert_eq!(parse.errors[0].message, "unterminated block comment");
        assert_eq!(parse.errors[0].range, text_range(24, 38));
    }
}
