use std::{fs, io, ops::Range, path::Path};

use mel_syntax::{SourceMap, SourceView, TextRange, text_range};

use crate::{
    DecodeDiagnostic, ParseError, SourceEncoding,
    decode::{OffsetMap, decode_source_auto, decode_source_with_encoding},
};

const DEFAULT_MAX_PREFIX_WORDS: usize = 64;
const DEFAULT_MAX_PREFIX_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightParseOptions {
    pub max_prefix_words: usize,
    pub max_prefix_bytes: usize,
}

impl Default for LightParseOptions {
    fn default() -> Self {
        Self {
            max_prefix_words: DEFAULT_MAX_PREFIX_WORDS,
            max_prefix_bytes: DEFAULT_MAX_PREFIX_BYTES,
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
    fn on_item(&mut self, source: SourceView<'_>, item: LightItem);
}

impl<F> LightItemSink for F
where
    F: for<'a> FnMut(SourceView<'a>, LightItem),
{
    fn on_item(&mut self, source: SourceView<'_>, item: LightItem) {
        self(source, item);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightParse {
    pub source: LightSourceFile,
    pub source_text: String,
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

pub fn scan_light_source_with_sink(input: &str, sink: &mut impl LightItemSink) -> LightScanReport {
    scan_light_source_with_options_and_sink(input, LightParseOptions::default(), sink)
}

pub fn scan_light_source_with_options_and_sink(
    input: &str,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> LightScanReport {
    let source_map = SourceMap::identity(input.len());
    let source_view = SourceView::new(input, &source_map);
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
    let mut sink = CollectLightItems::default();
    let report = scan_light_bytes_with_sink(input, &mut sink);
    LightParse::from((sink.finish(), report))
}

#[must_use]
pub fn parse_light_bytes_with_encoding(input: &[u8], encoding: SourceEncoding) -> LightParse {
    let mut sink = CollectLightItems::default();
    let report = scan_light_bytes_with_encoding_and_sink(input, encoding, &mut sink);
    LightParse::from((sink.finish(), report))
}

pub fn scan_light_bytes_with_sink(input: &[u8], sink: &mut impl LightItemSink) -> LightScanReport {
    scan_light_bytes_with_options_and_sink(input, LightParseOptions::default(), sink)
}

pub fn scan_light_bytes_with_options_and_sink(
    input: &[u8],
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> LightScanReport {
    let decoded = decode_source_auto(input);
    build_light_scan(decoded, options, sink)
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

pub fn scan_light_bytes_with_encoding_and_options_and_sink(
    input: &[u8],
    encoding: SourceEncoding,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> LightScanReport {
    let decoded = decode_source_with_encoding(input, encoding);
    build_light_scan(decoded, options, sink)
}

pub fn parse_light_file(path: impl AsRef<Path>) -> io::Result<LightParse> {
    let bytes = fs::read(path)?;
    Ok(parse_light_bytes(&bytes))
}

pub fn parse_light_file_with_encoding(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
) -> io::Result<LightParse> {
    let bytes = fs::read(path)?;
    Ok(parse_light_bytes_with_encoding(&bytes, encoding))
}

pub fn scan_light_file_with_sink(
    path: impl AsRef<Path>,
    sink: &mut impl LightItemSink,
) -> io::Result<LightScanReport> {
    scan_light_file_with_options_and_sink(path, LightParseOptions::default(), sink)
}

pub fn scan_light_file_with_options_and_sink(
    path: impl AsRef<Path>,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> io::Result<LightScanReport> {
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

pub fn scan_light_file_with_encoding_and_options_and_sink(
    path: impl AsRef<Path>,
    encoding: SourceEncoding,
    options: LightParseOptions,
    sink: &mut impl LightItemSink,
) -> io::Result<LightScanReport> {
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
    let source_map =
        SourceMap::from_source_to_display(decoded.offset_map.source_to_decoded.clone());
    let source_view = SourceView::new(&source_text, &source_map);
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
    fn on_item(&mut self, _: SourceView<'_>, item: LightItem) {
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
}

impl<'a> LightScanner<'a> {
    fn new(text: &'a str, options: LightParseOptions) -> Self {
        Self {
            text,
            options,
            errors: Vec::new(),
        }
    }

    fn scan_with_sink(
        &mut self,
        source: SourceView<'_>,
        sink: &mut impl LightItemSink,
        remap: Option<&OffsetMap>,
    ) {
        let mut cursor = self.skip_trivia(0);

        while cursor < self.text.len() {
            let (mut item, next_cursor) = if self.is_proc_start(cursor) {
                self.scan_proc_item(cursor)
            } else {
                self.scan_statement_item(cursor)
            };
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
        cursor = self.skip_trivia(self.consume_keyword(cursor, "proc").unwrap_or(cursor));

        let first_word = self.scan_simple_word(cursor);
        let mut name_range = None;
        if let Some((first_start, first_end)) = first_word {
            let after_first = self.skip_trivia(first_end);
            if self.peek_byte(after_first) == Some(b'(') {
                name_range = Some(text_range(first_start as u32, first_end as u32));
            } else if let Some((name_start, name_end)) = self.scan_simple_word(after_first) {
                name_range = Some(text_range(name_start as u32, name_end as u32));
            }
        }

        let end = self.scan_until_matching_body_end(start);
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
        let end = self.scan_statement_end(start);
        let body_end = self.statement_body_end(start, end);
        let Some((head_start, head_end)) = self.scan_simple_word(start) else {
            return (
                LightItem::Other {
                    span: text_range(start as u32, end as u32),
                },
                end,
            );
        };
        let head_range = text_range(head_start as u32, head_end as u32);
        let head_text = &self.text[head_start..head_end];
        let after_head = self.skip_trivia(head_end);
        if self.peek_byte(after_head) == Some(b'(') || is_non_command_head(head_text) {
            return (
                LightItem::Other {
                    span: text_range(start as u32, end as u32),
                },
                end,
            );
        }

        let mut words = Vec::new();
        let mut cursor = after_head;
        let mut opaque_tail = None;
        while cursor < body_end {
            cursor = self.skip_trivia(cursor);
            if cursor >= body_end {
                break;
            }
            let consumed_bytes = cursor.saturating_sub(head_end);
            if words.len() >= self.options.max_prefix_words
                || consumed_bytes >= self.options.max_prefix_bytes
            {
                opaque_tail = Some(text_range(cursor as u32, body_end as u32));
                break;
            }

            let Some((word, next_cursor)) = self.scan_word(cursor, body_end) else {
                opaque_tail = Some(text_range(cursor as u32, body_end as u32));
                break;
            };
            words.push(word);
            cursor = next_cursor;
        }

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

    fn scan_statement_end(&mut self, start: usize) -> usize {
        let mut cursor = start;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut in_string = false;
        let mut in_backquote = false;

        while cursor < self.text.len() {
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
                    in_string = true;
                    cursor += 1;
                }
                Some(b'`') => {
                    in_backquote = true;
                    cursor += 1;
                }
                Some(b'(') => {
                    paren_depth += 1;
                    cursor += 1;
                }
                Some(b')') => {
                    paren_depth = paren_depth.saturating_sub(1);
                    cursor += 1;
                }
                Some(b'[') => {
                    bracket_depth += 1;
                    cursor += 1;
                }
                Some(b']') => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                    cursor += 1;
                }
                Some(b'{') => {
                    brace_depth += 1;
                    cursor += 1;
                }
                Some(b'}') => {
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
                    return cursor + 1;
                }
                Some(_) => cursor = self.next_offset(cursor),
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
            return Some((LightWord::BraceList { range }, end));
        }
        if self.starts_with(start, "<<") {
            let end = self.scan_vector_literal(start);
            let range = text_range(start as u32, end as u32);
            return Some((LightWord::VectorLiteral { range }, end));
        }
        if self.byte_at(start) == Some(b'(') {
            let end = self.scan_balanced(start, b'(', b')');
            let range = text_range(start as u32, end as u32);
            return Some((LightWord::GroupedExpr { range }, end));
        }

        let end = self.scan_simple_word_until(start, body_end);
        if end <= start {
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
                Some(b'"') => return cursor + 1,
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        self.errors.push(ParseError {
            message: "unterminated string literal in lightweight surface parse".to_owned(),
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
                Some(b'`') => return cursor + 1,
                Some(b'"') => cursor = self.scan_quoted_string(cursor),
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        self.errors.push(ParseError {
            message: "unterminated backquote capture in lightweight surface parse".to_owned(),
            range: text_range(start as u32, self.text.len() as u32),
        });
        self.text.len()
    }

    fn scan_balanced(&mut self, start: usize, open: u8, close: u8) -> usize {
        let mut cursor = start;
        let mut depth = 0usize;
        while cursor < self.text.len() {
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
                    depth += 1;
                    cursor += 1;
                }
                Some(ch) if ch == close => {
                    depth = depth.saturating_sub(1);
                    cursor += 1;
                    if depth == 0 {
                        return cursor;
                    }
                }
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        self.errors.push(ParseError {
            message: "unterminated grouped surface in lightweight parse".to_owned(),
            range: text_range(start as u32, self.text.len() as u32),
        });
        self.text.len()
    }

    fn scan_vector_literal(&mut self, start: usize) -> usize {
        let mut cursor = start + 2;
        while cursor < self.text.len() {
            if self.starts_with(cursor, ">>") {
                return cursor + 2;
            }
            if self.byte_at(cursor) == Some(b'"') {
                cursor = self.scan_quoted_string(cursor);
                continue;
            }
            cursor = self.next_offset(cursor);
        }
        self.errors.push(ParseError {
            message: "unterminated vector literal in lightweight parse".to_owned(),
            range: text_range(start as u32, self.text.len() as u32),
        });
        self.text.len()
    }

    fn scan_until_matching_body_end(&mut self, start: usize) -> usize {
        let mut cursor = start;
        let mut depth = 0usize;
        let mut saw_body = false;
        while cursor < self.text.len() {
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
                    saw_body = true;
                    depth += 1;
                    cursor += 1;
                }
                Some(b'}') if saw_body => {
                    depth = depth.saturating_sub(1);
                    cursor += 1;
                    if depth == 0 {
                        return cursor;
                    }
                }
                Some(_) => cursor = self.next_offset(cursor),
                None => break,
            }
        }
        self.errors.push(ParseError {
            message: "unterminated proc body in lightweight surface parse".to_owned(),
            range: text_range(start as u32, self.text.len() as u32),
        });
        self.text.len()
    }

    fn scan_simple_word(&self, start: usize) -> Option<(usize, usize)> {
        let start = self.skip_trivia(start);
        let end = self.scan_simple_word_until(start, self.text.len());
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

    fn skip_trivia(&self, start: usize) -> usize {
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

    fn skip_block_comment(&self, start: usize) -> usize {
        let mut cursor = start + 2;
        while cursor < self.text.len() {
            if self.starts_with(cursor, "*/") {
                return cursor + 2;
            }
            cursor = self.next_offset(cursor);
        }
        self.text.len()
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

    fn is_proc_start(&self, start: usize) -> bool {
        self.consume_keyword(start, "proc").is_some()
            || self
                .consume_keyword(start, "global")
                .and_then(|after_global| {
                    self.consume_keyword(self.skip_trivia(after_global), "proc")
                })
                .is_some()
    }

    fn consume_keyword(&self, start: usize, keyword: &str) -> Option<usize> {
        let cursor = self.skip_trivia(start);
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
}

fn is_word_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$')
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
