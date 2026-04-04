use encoding_rs::{DecoderResult, Encoding, GBK, SHIFT_JIS};
use std::borrow::Cow;
use std::str::Utf8Error;
use std::sync::Arc;

use mel_syntax::{SourceMapEdit, TextRange, range_end, range_start, text_range};

use crate::{DecodeDiagnostic, SourceEncoding};

pub(crate) struct DecodedSource<'a> {
    pub(crate) encoding: SourceEncoding,
    pub(crate) text: Cow<'a, str>,
    pub(crate) offset_map: OffsetMap,
    pub(crate) diagnostics: Vec<DecodeDiagnostic>,
}

pub(crate) struct DecodedOwnedSource {
    pub(crate) encoding: SourceEncoding,
    pub(crate) text: String,
    pub(crate) offset_map: OffsetMap,
    pub(crate) diagnostics: Vec<DecodeDiagnostic>,
}

#[derive(Debug, Clone)]
enum OffsetMapKind {
    Identity {
        len: usize,
    },
    Indexed {
        decoded_to_source: Box<[u32]>,
        source_to_decoded: Arc<[u32]>,
    },
    Sparse {
        source_len: usize,
        display_len: usize,
        edits: Arc<[SourceMapEdit]>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct OffsetMap {
    kind: OffsetMapKind,
}

impl OffsetMap {
    fn identity(len: usize) -> Self {
        Self {
            kind: OffsetMapKind::Identity { len },
        }
    }

    fn from_decoded_text(text: &str, source_len: usize, encoding: SourceEncoding) -> Option<Self> {
        let mut decoded_to_source = vec![0; text.len() + 1];
        let mut source_to_decoded = vec![0; source_len + 1];
        let mut decoded_offset = 0usize;
        let mut source_offset = 0usize;

        for ch in text.chars() {
            let decoded_len = ch.len_utf8();
            let source_char_len = source_len_for_char(ch, encoding)?;
            let source_end = source_offset.saturating_add(source_char_len);
            let decoded_end = decoded_offset.saturating_add(decoded_len);
            for step in 1..=decoded_len {
                decoded_to_source[decoded_offset + step] =
                    u32::try_from(source_end).unwrap_or(u32::MAX);
            }
            for step in 1..=source_char_len {
                source_to_decoded[source_offset + step] =
                    u32::try_from(decoded_end).unwrap_or(u32::MAX);
            }
            decoded_offset += decoded_len;
            source_offset = source_end;
        }

        if source_offset != source_len {
            return None;
        }

        decoded_to_source[text.len()] = u32::try_from(source_len).unwrap_or(u32::MAX);
        source_to_decoded[source_len] = u32::try_from(text.len()).unwrap_or(u32::MAX);
        Some(Self {
            kind: OffsetMapKind::Indexed {
                decoded_to_source: decoded_to_source.into_boxed_slice(),
                source_to_decoded: Arc::from(source_to_decoded),
            },
        })
    }

    fn map_offset(&self, offset: u32) -> u32 {
        match &self.kind {
            OffsetMapKind::Identity { len } => {
                u32::try_from(usize::try_from(offset).unwrap_or(*len).min(*len)).unwrap_or(u32::MAX)
            }
            OffsetMapKind::Indexed {
                decoded_to_source, ..
            } => decoded_to_source
                .get(offset as usize)
                .copied()
                .or_else(|| decoded_to_source.last().copied())
                .unwrap_or(offset),
            OffsetMapKind::Sparse {
                source_len,
                display_len,
                edits,
            } => sparse_display_to_source(*source_len, *display_len, edits, offset as usize),
        }
    }

    pub(crate) fn map_range(&self, range: TextRange) -> TextRange {
        text_range(
            self.map_offset(range_start(range)),
            self.map_offset(range_end(range)),
        )
    }

    pub(crate) fn source_map(&self) -> mel_syntax::SourceMap {
        match &self.kind {
            OffsetMapKind::Identity { len } => mel_syntax::SourceMap::identity(*len),
            OffsetMapKind::Indexed {
                source_to_decoded, ..
            } => {
                mel_syntax::SourceMap::from_shared_source_to_display(Arc::clone(source_to_decoded))
            }
            OffsetMapKind::Sparse {
                source_len,
                display_len,
                edits,
            } => mel_syntax::SourceMap::from_sparse_edits(
                *source_len,
                *display_len,
                Arc::clone(edits),
            ),
        }
    }
}

pub(crate) fn decode_source_auto(input: &[u8]) -> DecodedSource<'_> {
    match std::str::from_utf8(input) {
        Ok(text) => DecodedSource {
            encoding: SourceEncoding::Utf8,
            text: Cow::Borrowed(text),
            offset_map: OffsetMap::identity(text.len()),
            diagnostics: Vec::new(),
        },
        Err(error) => decode_source_auto_with_error(input, error),
    }
}

pub(crate) fn decode_owned_bytes_auto(input: Vec<u8>) -> DecodedOwnedSource {
    match String::from_utf8(input) {
        Ok(text) => {
            let len = text.len();
            DecodedOwnedSource {
                encoding: SourceEncoding::Utf8,
                text,
                offset_map: OffsetMap::identity(len),
                diagnostics: Vec::new(),
            }
        }
        Err(error) => decode_source_auto(error.as_bytes()).into_owned(),
    }
}

fn decode_source_auto_with_error(input: &[u8], utf8_error: Utf8Error) -> DecodedSource<'_> {
    let sample = decode_auto_sample(input, utf8_error.valid_up_to());
    let utf8_lossy_rank = decode_utf8_lossy_sample_rank(sample);
    let cp932_rank = decode_non_utf8_sample_rank(sample, SourceEncoding::Cp932);
    let gbk_rank = decode_non_utf8_sample_rank(sample, SourceEncoding::Gbk);
    let (best_encoding, best_non_utf8_rank) = if cp932_rank <= gbk_rank {
        (SourceEncoding::Cp932, cp932_rank)
    } else {
        (SourceEncoding::Gbk, gbk_rank)
    };

    if best_non_utf8_rank.0 == 0 && best_non_utf8_rank.1 < utf8_lossy_rank.1 {
        let decoded = decode_source_with_encoding(input, best_encoding);
        if decoded.diagnostics.is_empty() {
            return decoded;
        }
    }

    decode_lossy_utf8_with_error(input, utf8_error.valid_up_to() as u32, utf8_error)
}

pub(crate) fn decode_source_with_encoding(
    input: &[u8],
    encoding: SourceEncoding,
) -> DecodedSource<'_> {
    if matches!(encoding, SourceEncoding::Utf8) {
        return match std::str::from_utf8(input) {
            Ok(text) => DecodedSource {
                encoding,
                text: Cow::Borrowed(text),
                offset_map: OffsetMap::identity(text.len()),
                diagnostics: Vec::new(),
            },
            Err(error) => decode_lossy_utf8_with_error(input, error.valid_up_to() as u32, error),
        };
    }

    let encoding_rs = encoding_rs_encoding(encoding);
    if Encoding::ascii_valid_up_to(input) == input.len() {
        let text = std::str::from_utf8(input).unwrap_or_default();
        return DecodedSource {
            encoding,
            text: Cow::Borrowed(text),
            offset_map: OffsetMap::identity(text.len()),
            diagnostics: Vec::new(),
        };
    }

    let (text, _, had_errors) = encoding_rs.decode(input);
    let offset_map = if had_errors {
        OffsetMap::from_decoded_text(text.as_ref(), input.len(), encoding)
            .unwrap_or_else(|| OffsetMap::identity(text.len()))
    } else {
        OffsetMap::from_ascii_compatible_text(input, text.as_ref(), encoding)
            .or_else(|| OffsetMap::from_decoded_text(text.as_ref(), input.len(), encoding))
            .unwrap_or_else(|| OffsetMap::identity(text.len()))
    };
    let diagnostics = if had_errors {
        vec![DecodeDiagnostic {
            message: format!(
                "source is not valid {}; decoded with replacement",
                encoding.label()
            )
            .into(),
            range: text_range(0, input.len() as u32),
        }]
    } else {
        Vec::new()
    };

    DecodedSource {
        encoding,
        text,
        offset_map,
        diagnostics,
    }
}

pub(crate) fn decode_owned_bytes_with_encoding(
    input: Vec<u8>,
    encoding: SourceEncoding,
) -> DecodedOwnedSource {
    if matches!(encoding, SourceEncoding::Utf8) {
        return match String::from_utf8(input) {
            Ok(text) => {
                let len = text.len();
                DecodedOwnedSource {
                    encoding,
                    text,
                    offset_map: OffsetMap::identity(len),
                    diagnostics: Vec::new(),
                }
            }
            Err(error) => {
                decode_source_with_encoding(error.as_bytes(), SourceEncoding::Utf8).into_owned()
            }
        };
    }

    if Encoding::ascii_valid_up_to(&input) == input.len() {
        let text = String::from_utf8(input).unwrap_or_default();
        let len = text.len();
        return DecodedOwnedSource {
            encoding,
            text,
            offset_map: OffsetMap::identity(len),
            diagnostics: Vec::new(),
        };
    }

    decode_source_with_encoding(&input, encoding).into_owned()
}

impl DecodedSource<'_> {
    fn into_owned(self) -> DecodedOwnedSource {
        DecodedOwnedSource {
            encoding: self.encoding,
            text: self.text.into_owned(),
            offset_map: self.offset_map,
            diagnostics: self.diagnostics,
        }
    }
}

fn sparse_display_to_source(
    source_len: usize,
    display_len: usize,
    edits: &[SourceMapEdit],
    offset: usize,
) -> u32 {
    let clamped = offset.min(display_len) as u32;
    let Some(index) = edits
        .partition_point(|edit| edit.display_start() <= clamped)
        .checked_sub(1)
    else {
        return clamped;
    };
    let edit = edits[index];
    if clamped == edit.display_start() {
        return edit.source_start();
    }
    if clamped <= edit.display_end() {
        return edit.source_end();
    }
    let mapped = (clamped as i64 - (edit.display_end() as i64 - edit.source_end() as i64))
        .clamp(0, source_len as i64);
    mapped as u32
}

fn decode_auto_sample(input: &[u8], valid_up_to: usize) -> &[u8] {
    const SAMPLE_PREFIX_CONTEXT: usize = 256;
    const SAMPLE_MAX_BYTES: usize = 64 * 1024;

    let start = valid_up_to.saturating_sub(SAMPLE_PREFIX_CONTEXT);
    let end = input.len().min(start.saturating_add(SAMPLE_MAX_BYTES));
    &input[start..end]
}

fn decode_utf8_lossy_sample_rank(sample: &[u8]) -> (u8, usize, u8) {
    let text = String::from_utf8_lossy(sample);
    (
        1,
        suspicious_text_score(text.as_ref()),
        decode_encoding_bias(SourceEncoding::Utf8),
    )
}

fn decode_non_utf8_sample_rank(sample: &[u8], encoding: SourceEncoding) -> (u8, usize, u8) {
    let (text, _, had_errors) = encoding_rs_encoding(encoding).decode(sample);
    (
        u8::from(had_errors),
        suspicious_text_score(text.as_ref()),
        decode_encoding_bias(encoding),
    )
}

fn decode_encoding_bias(encoding: SourceEncoding) -> u8 {
    match encoding {
        SourceEncoding::Cp932 => 0,
        SourceEncoding::Gbk => 1,
        SourceEncoding::Utf8 => 2,
    }
}

fn suspicious_text_score(text: &str) -> usize {
    text.chars().map(suspicious_char_weight).sum()
}

fn suspicious_char_weight(ch: char) -> usize {
    match ch {
        '\u{FFFD}' => 1,
        '\u{0080}'..='\u{009F}' => 1,
        '\u{E000}'..='\u{F8FF}' => 1,
        '\u{FF61}'..='\u{FF9F}' => 1,
        _ => 0,
    }
}

fn decode_lossy_utf8_with_error(
    input: &[u8],
    start: u32,
    error: std::str::Utf8Error,
) -> DecodedSource<'_> {
    let end = error
        .error_len()
        .map_or(input.len() as u32, |len| start + len as u32);
    let (text, offset_map) = decode_lossy_utf8_text_and_offset_map(input);

    DecodedSource {
        encoding: SourceEncoding::Utf8,
        offset_map,
        text: Cow::Owned(text),
        diagnostics: vec![DecodeDiagnostic {
            message: "source is not valid UTF-8; decoded lossily".into(),
            range: text_range(start, end),
        }],
    }
}

fn decode_lossy_utf8_text_and_offset_map(input: &[u8]) -> (String, OffsetMap) {
    let mut text = String::new();
    let mut decoded_to_source = vec![0];
    let mut source_to_decoded = vec![0; input.len() + 1];
    let mut source_offset = 0usize;

    while source_offset < input.len() {
        match std::str::from_utf8(&input[source_offset..]) {
            Ok(valid) => {
                for ch in valid.chars() {
                    append_decoded_char_mapping(
                        &mut text,
                        &mut decoded_to_source,
                        &mut source_to_decoded,
                        source_offset,
                        ch.len_utf8(),
                        ch,
                    );
                    source_offset += ch.len_utf8();
                }
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    let valid =
                        std::str::from_utf8(&input[source_offset..source_offset + valid_up_to])
                            .unwrap_or_default();
                    for ch in valid.chars() {
                        append_decoded_char_mapping(
                            &mut text,
                            &mut decoded_to_source,
                            &mut source_to_decoded,
                            source_offset,
                            ch.len_utf8(),
                            ch,
                        );
                        source_offset += ch.len_utf8();
                    }
                }

                let invalid_len = error.error_len().unwrap_or(input.len() - source_offset);
                append_decoded_char_mapping(
                    &mut text,
                    &mut decoded_to_source,
                    &mut source_to_decoded,
                    source_offset,
                    invalid_len,
                    char::REPLACEMENT_CHARACTER,
                );
                source_offset += invalid_len;
            }
        }
    }

    (
        text,
        OffsetMap {
            kind: OffsetMapKind::Indexed {
                decoded_to_source: decoded_to_source.into_boxed_slice(),
                source_to_decoded: Arc::from(source_to_decoded),
            },
        },
    )
}

fn append_decoded_char_mapping(
    text: &mut String,
    decoded_to_source: &mut Vec<u32>,
    source_to_decoded: &mut [u32],
    source_start: usize,
    source_len: usize,
    ch: char,
) {
    let decoded_start = text.len();
    let source_end = source_start + source_len;

    text.push(ch);
    let decoded_end = text.len();
    decoded_to_source.resize(decoded_end + 1, source_end as u32);
    for mapped in decoded_to_source
        .iter_mut()
        .take(decoded_end + 1)
        .skip(decoded_start + 1)
    {
        *mapped = source_end as u32;
    }

    for mapped in source_to_decoded
        .iter_mut()
        .take(source_end + 1)
        .skip(source_start + 1)
    {
        *mapped = decoded_end as u32;
    }
}

impl OffsetMap {
    fn from_ascii_compatible_text(
        input: &[u8],
        text: &str,
        encoding: SourceEncoding,
    ) -> Option<Self> {
        let mut source_offset = 0usize;
        let mut display_offset = 0usize;
        let mut edits = Vec::new();

        while source_offset < input.len() || display_offset < text.len() {
            let ascii_run = Encoding::ascii_valid_up_to(&input[source_offset..]);
            if ascii_run > 0 {
                source_offset += ascii_run;
                display_offset += ascii_run;
                continue;
            }

            let run_display_end = next_ascii_display_boundary(text, display_offset);
            let display_len = run_display_end.saturating_sub(display_offset);
            let display_run = &text[display_offset..run_display_end];
            let source_len =
                source_len_for_decoded_run(&input[source_offset..], display_run, encoding)?;
            if source_len != display_len {
                edits.push(SourceMapEdit::new(
                    u32::try_from(source_offset).unwrap_or(u32::MAX),
                    u32::try_from(source_offset + source_len).unwrap_or(u32::MAX),
                    u32::try_from(display_offset).unwrap_or(u32::MAX),
                    u32::try_from(run_display_end).unwrap_or(u32::MAX),
                ));
            }
            source_offset += source_len;
            display_offset = run_display_end;
        }

        if source_offset != input.len() || display_offset != text.len() {
            return None;
        }

        if edits.is_empty() && input.len() == text.len() {
            return Some(Self::identity(text.len()));
        }

        Some(Self {
            kind: OffsetMapKind::Sparse {
                source_len: input.len(),
                display_len: text.len(),
                edits: Arc::from(edits),
            },
        })
    }
}

fn next_ascii_display_boundary(text: &str, display_offset: usize) -> usize {
    let mut end = display_offset;
    for ch in text[display_offset..].chars() {
        if ch.is_ascii() {
            break;
        }
        end += ch.len_utf8();
    }
    end
}

fn source_len_for_decoded_run(
    input: &[u8],
    display_run: &str,
    encoding: SourceEncoding,
) -> Option<usize> {
    let mut decoder = encoding_rs_encoding(encoding).new_decoder_without_bom_handling();
    let mut output = vec![0; display_run.len()];
    let (result, read, written) =
        decoder.decode_to_utf8_without_replacement(input, &mut output, false);

    match result {
        DecoderResult::InputEmpty | DecoderResult::OutputFull => (written == display_run.len()
            && &output[..written] == display_run.as_bytes())
        .then_some(read),
        DecoderResult::Malformed(_, _) => None,
    }
}

impl SourceEncoding {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Cp932 => "cp932",
            Self::Gbk => "gbk",
        }
    }
}

fn encoding_rs_encoding(encoding: SourceEncoding) -> &'static Encoding {
    match encoding {
        SourceEncoding::Utf8 => encoding_rs::UTF_8,
        SourceEncoding::Cp932 => SHIFT_JIS,
        SourceEncoding::Gbk => GBK,
    }
}

fn source_len_for_char(ch: char, encoding: SourceEncoding) -> Option<usize> {
    if matches!(encoding, SourceEncoding::Utf8) {
        return Some(ch.len_utf8());
    }

    let mut text = String::new();
    text.push(ch);
    let (encoded, _, had_errors) = encoding_rs_encoding(encoding).encode(&text);
    (!had_errors).then(|| encoded.len())
}
