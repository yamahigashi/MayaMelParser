use crate::model::{
    DefaultMayaSelectiveSetAttrSelector, MayaSelectiveCreateNode, MayaSelectiveFile,
    MayaSelectiveItem, MayaSelectiveItemSink, MayaSelectiveOptions, MayaSelectivePassthrough,
    MayaSelectiveRequires, MayaSelectiveSetAttr, MayaSelectiveSetAttrSelector,
};
use mel_parser::{
    LightParseOptions, LightScanReport, LightSourceView, LightWord, SourceEncoding,
    scan_light_bytes_with_encoding_and_options_and_sink, scan_light_bytes_with_options_and_sink,
    scan_light_file_with_encoding_and_options_and_sink, scan_light_file_with_options_and_sink,
    scan_light_source_with_options_and_sink,
};
use mel_syntax::TextRange;

pub fn collect_selective_top_level_source_with_sink(
    input: &str,
    sink: &mut impl MayaSelectiveItemSink,
) -> LightScanReport {
    collect_selective_top_level_source_with_options_and_sink(
        input,
        LightParseOptions::default(),
        &MayaSelectiveOptions::default(),
        &DefaultMayaSelectiveSetAttrSelector,
        sink,
    )
}

pub fn collect_selective_top_level_source_with_options_and_sink(
    input: &str,
    light_options: LightParseOptions,
    options: &MayaSelectiveOptions,
    selector: &impl MayaSelectiveSetAttrSelector,
    sink: &mut impl MayaSelectiveItemSink,
) -> LightScanReport {
    let mut bridge = SelectiveBridge::new(options, selector, sink);
    scan_light_source_with_options_and_sink(input, light_options, &mut bridge)
}

pub fn collect_selective_top_level_bytes_with_sink(
    input: &[u8],
    sink: &mut impl MayaSelectiveItemSink,
) -> LightScanReport {
    collect_selective_top_level_bytes_with_options_and_sink(
        input,
        LightParseOptions::default(),
        &MayaSelectiveOptions::default(),
        &DefaultMayaSelectiveSetAttrSelector,
        sink,
    )
}

pub fn collect_selective_top_level_bytes_with_options_and_sink(
    input: &[u8],
    light_options: LightParseOptions,
    options: &MayaSelectiveOptions,
    selector: &impl MayaSelectiveSetAttrSelector,
    sink: &mut impl MayaSelectiveItemSink,
) -> LightScanReport {
    let mut bridge = SelectiveBridge::new(options, selector, sink);
    scan_light_bytes_with_options_and_sink(input, light_options, &mut bridge)
}

pub fn collect_selective_top_level_bytes_with_encoding_and_sink(
    input: &[u8],
    encoding: SourceEncoding,
    options: &MayaSelectiveOptions,
    selector: &impl MayaSelectiveSetAttrSelector,
    sink: &mut impl MayaSelectiveItemSink,
) -> LightScanReport {
    collect_selective_top_level_bytes_with_encoding_and_options_and_sink(
        input,
        encoding,
        LightParseOptions::default(),
        options,
        selector,
        sink,
    )
}

pub fn collect_selective_top_level_bytes_with_encoding_and_options_and_sink(
    input: &[u8],
    encoding: SourceEncoding,
    light_options: LightParseOptions,
    options: &MayaSelectiveOptions,
    selector: &impl MayaSelectiveSetAttrSelector,
    sink: &mut impl MayaSelectiveItemSink,
) -> LightScanReport {
    let mut bridge = SelectiveBridge::new(options, selector, sink);
    scan_light_bytes_with_encoding_and_options_and_sink(input, encoding, light_options, &mut bridge)
}

pub fn collect_selective_top_level_file_with_sink(
    path: impl AsRef<std::path::Path>,
    sink: &mut impl MayaSelectiveItemSink,
) -> std::io::Result<LightScanReport> {
    collect_selective_top_level_file_with_options_and_sink(
        path,
        &MayaSelectiveOptions::default(),
        &DefaultMayaSelectiveSetAttrSelector,
        sink,
    )
}

pub fn collect_selective_top_level_file_with_options_and_sink(
    path: impl AsRef<std::path::Path>,
    options: &MayaSelectiveOptions,
    selector: &impl MayaSelectiveSetAttrSelector,
    sink: &mut impl MayaSelectiveItemSink,
) -> std::io::Result<LightScanReport> {
    collect_selective_top_level_file_with_light_options_and_sink(
        path,
        LightParseOptions::default(),
        options,
        selector,
        sink,
    )
}

pub fn collect_selective_top_level_file_with_light_options_and_sink(
    path: impl AsRef<std::path::Path>,
    light_options: LightParseOptions,
    options: &MayaSelectiveOptions,
    selector: &impl MayaSelectiveSetAttrSelector,
    sink: &mut impl MayaSelectiveItemSink,
) -> std::io::Result<LightScanReport> {
    let mut bridge = SelectiveBridge::new(options, selector, sink);
    scan_light_file_with_options_and_sink(path, light_options, &mut bridge)
}

pub fn collect_selective_top_level_file_with_encoding_and_sink(
    path: impl AsRef<std::path::Path>,
    encoding: SourceEncoding,
    options: &MayaSelectiveOptions,
    selector: &impl MayaSelectiveSetAttrSelector,
    sink: &mut impl MayaSelectiveItemSink,
) -> std::io::Result<LightScanReport> {
    collect_selective_top_level_file_with_encoding_and_options_and_sink(
        path,
        encoding,
        LightParseOptions::default(),
        options,
        selector,
        sink,
    )
}

pub fn collect_selective_top_level_file_with_encoding_and_options_and_sink(
    path: impl AsRef<std::path::Path>,
    encoding: SourceEncoding,
    light_options: LightParseOptions,
    options: &MayaSelectiveOptions,
    selector: &impl MayaSelectiveSetAttrSelector,
    sink: &mut impl MayaSelectiveItemSink,
) -> std::io::Result<LightScanReport> {
    let mut bridge = SelectiveBridge::new(options, selector, sink);
    scan_light_file_with_encoding_and_options_and_sink(path, encoding, light_options, &mut bridge)
}

struct SelectiveBridge<'a, Sel: ?Sized, Sink: ?Sized> {
    options: &'a MayaSelectiveOptions,
    selector: &'a Sel,
    sink: &'a mut Sink,
}

impl<'a, Sel: ?Sized, Sink: ?Sized> SelectiveBridge<'a, Sel, Sink> {
    fn new(options: &'a MayaSelectiveOptions, selector: &'a Sel, sink: &'a mut Sink) -> Self {
        Self {
            options,
            selector,
            sink,
        }
    }
}

impl<Sel, Sink> mel_parser::LightItemSink for SelectiveBridge<'_, Sel, Sink>
where
    Sel: MayaSelectiveSetAttrSelector + ?Sized,
    Sink: MayaSelectiveItemSink + ?Sized,
{
    fn on_item(&mut self, source: LightSourceView<'_>, item: mel_parser::LightItem) {
        let mel_parser::LightItem::Command(command) = item else {
            return;
        };
        let Some(item) = selective_item_from_command(source, &command, self.options, self.selector)
        else {
            return;
        };
        self.sink.on_item(item);
    }
}

fn selective_item_from_command(
    source: LightSourceView<'_>,
    command: &mel_parser::LightCommandSurface,
    options: &MayaSelectiveOptions,
    selector: &(impl MayaSelectiveSetAttrSelector + ?Sized),
) -> Option<MayaSelectiveItem> {
    let head = source.try_ascii_slice(command.head_range)?;
    match head {
        "requires" => Some(MayaSelectiveItem::Requires(MayaSelectiveRequires {
            head_range: command.head_range,
            argument_ranges: collect_non_flag_ranges(&command.words),
            span: command.span,
        })),
        "file" => Some(MayaSelectiveItem::File(MayaSelectiveFile {
            head_range: command.head_range,
            path_range: last_non_flag_range(&command.words),
            span: command.span,
        })),
        "createNode" => Some(MayaSelectiveItem::CreateNode(MayaSelectiveCreateNode {
            head_range: command.head_range,
            node_type_range: first_non_flag_range(&command.words),
            name_range: first_flag_arg_range(source, &command.words, &["name", "n"]),
            parent_range: first_flag_arg_range(source, &command.words, &["parent", "p"]),
            span: command.span,
        })),
        "setAttr" => {
            let attr_path_range = first_non_flag_range(&command.words);
            let type_name_range = first_flag_arg_range(source, &command.words, &["type", "typ"]);
            let tracked_attr = attr_path_range.and_then(|range| {
                let decoded = source.decode_slice(range);
                selector.classify(strip_outer_quotes(decoded.text.as_ref()))
            });
            Some(MayaSelectiveItem::SetAttr(MayaSelectiveSetAttr {
                head_range: command.head_range,
                attr_path_range,
                type_name_range,
                tracked_attr,
                opaque_tail: command.opaque_tail,
                span: command.span,
            }))
        }
        _ => match options.passthrough {
            MayaSelectivePassthrough::TargetOnly => None,
            MayaSelectivePassthrough::IncludeOtherCommands => {
                Some(MayaSelectiveItem::OtherCommand {
                    head_range: command.head_range,
                    span: command.span,
                })
            }
        },
    }
}

fn collect_non_flag_ranges(words: &[LightWord]) -> Vec<TextRange> {
    words.iter().filter_map(non_flag_range).collect()
}

fn first_non_flag_range(words: &[LightWord]) -> Option<TextRange> {
    words.iter().find_map(non_flag_range)
}

fn last_non_flag_range(words: &[LightWord]) -> Option<TextRange> {
    words.iter().rev().find_map(non_flag_range)
}

fn non_flag_range(word: &LightWord) -> Option<TextRange> {
    (!matches!(word, LightWord::Flag { .. })).then_some(word.range())
}

fn first_flag_arg_range(
    source: LightSourceView<'_>,
    words: &[LightWord],
    names: &[&str],
) -> Option<TextRange> {
    let mut index = 0;
    while index < words.len() {
        let LightWord::Flag { text, .. } = &words[index] else {
            index += 1;
            continue;
        };
        let normalized = source.try_ascii_slice(*text)?.trim_start_matches('-');
        if names.contains(&normalized) {
            return words.get(index + 1).and_then(non_flag_range);
        }
        index += 1;
    }
    None
}

fn strip_outer_quotes(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(text)
}
