use encoding_rs::{Encoding, GBK, SHIFT_JIS};
use std::borrow::Cow;

use mel_syntax::{TextRange, range_end, range_start, text_range};

use crate::{DecodeDiagnostic, SourceEncoding};

pub(crate) struct DecodedSource<'a> {
    pub(crate) encoding: SourceEncoding,
    pub(crate) text: Cow<'a, str>,
    pub(crate) offset_map: OffsetMap,
    pub(crate) diagnostics: Vec<DecodeDiagnostic>,
}

#[derive(Debug, Clone)]
enum OffsetMapKind {
    Identity {
        len: usize,
    },
    Indexed {
        decoded_to_source: Vec<u32>,
        source_to_decoded: Vec<u32>,
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
                decoded_to_source,
                source_to_decoded,
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
            } => mel_syntax::SourceMap::from_source_to_display(source_to_decoded.clone()),
        }
    }
}

pub(crate) fn decode_source_auto(input: &[u8]) -> DecodedSource<'_> {
    if let Ok(text) = std::str::from_utf8(input) {
        return DecodedSource {
            encoding: SourceEncoding::Utf8,
            text: Cow::Borrowed(text),
            offset_map: OffsetMap::identity(text.len()),
            diagnostics: Vec::new(),
        };
    }

    let utf8_lossy = decode_lossy_utf8(input);

    let cp932 = decode_source_with_encoding(input, SourceEncoding::Cp932);
    let gbk = decode_source_with_encoding(input, SourceEncoding::Gbk);
    let best_non_utf8 = if decode_candidate_rank(&cp932) <= decode_candidate_rank(&gbk) {
        cp932
    } else {
        gbk
    };

    if should_prefer_non_utf8(&best_non_utf8, &utf8_lossy) {
        best_non_utf8
    } else {
        utf8_lossy
    }
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

    let (text, _, had_errors) = encoding_rs_encoding(encoding).decode(input);
    let offset_map = OffsetMap::from_decoded_text(text.as_ref(), input.len(), encoding)
        .unwrap_or_else(|| OffsetMap::identity(text.len()));
    let diagnostics = if had_errors {
        vec![DecodeDiagnostic {
            message: format!(
                "source is not valid {}; decoded with replacement",
                encoding.label()
            ),
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

fn decode_lossy_utf8(input: &[u8]) -> DecodedSource<'_> {
    match std::str::from_utf8(input) {
        Ok(text) => DecodedSource {
            encoding: SourceEncoding::Utf8,
            text: Cow::Borrowed(text),
            offset_map: OffsetMap::identity(text.len()),
            diagnostics: Vec::new(),
        },
        Err(error) => decode_lossy_utf8_with_error(input, error.valid_up_to() as u32, error),
    }
}

fn decode_candidate_rank(decoded: &DecodedSource<'_>) -> (u8, usize, u8) {
    let has_decode_errors = u8::from(!decoded.diagnostics.is_empty());
    let suspicious_score = suspicious_text_score(decoded.text.as_ref());
    let encoding_bias = match decoded.encoding {
        SourceEncoding::Cp932 => 0,
        SourceEncoding::Gbk => 1,
        SourceEncoding::Utf8 => 2,
    };
    (has_decode_errors, suspicious_score, encoding_bias)
}

fn should_prefer_non_utf8(non_utf8: &DecodedSource<'_>, utf8_lossy: &DecodedSource<'_>) -> bool {
    non_utf8.diagnostics.is_empty()
        && suspicious_text_score(non_utf8.text.as_ref())
            < suspicious_text_score(utf8_lossy.text.as_ref())
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
            message: "source is not valid UTF-8; decoded lossily".to_owned(),
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
                decoded_to_source,
                source_to_decoded,
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
