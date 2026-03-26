#![forbid(unsafe_code)]

use mel_ast::{Expr, InvokeSurface, Item, ProcDef, ShellWord, Stmt};
use mel_parser::{
    LightCommandSurface, LightItem, LightItemSink, LightParse, LightParseOptions, LightScanReport,
    LightWord, Parse, ParseOptions, SourceEncoding, parse_source_view_range_with_options,
    scan_light_bytes_with_encoding_and_options_and_sink, scan_light_bytes_with_options_and_sink,
    scan_light_file_with_encoding_and_options_and_sink, scan_light_file_with_options_and_sink,
    scan_light_source_with_options_and_sink,
};
use mel_sema::{
    CommandKind, CommandMode, CommandModeMask, CommandRegistry, CommandSchema, CommandSourceKind,
    EmptyCommandRegistry, FlagArity, FlagArityByMode, FlagSchema, NormalizedCommandItem,
    NormalizedFlag, PositionalArg, PositionalSchema, PositionalSlotSchema, PositionalSourcePolicy,
    PositionalTailSchema, ReturnBehavior, ValueShape,
};
use mel_syntax::{TextRange, range_end, range_start, text_range};
use std::sync::OnceLock;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MayaCommandRegistry;

impl MayaCommandRegistry {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl CommandRegistry for MayaCommandRegistry {
    fn lookup(&self, name: &str) -> Option<&CommandSchema> {
        shared_command_schemas()
            .binary_search_by(|schema| schema.name.as_ref().cmp(name))
            .ok()
            .map(|index| &shared_command_schemas()[index])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MayaTopLevelFacts {
    pub items: Vec<MayaTopLevelItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MayaLightTopLevelFacts {
    pub items: Vec<MayaLightTopLevelItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MayaSelectivePassthrough {
    #[default]
    TargetOnly,
    IncludeOtherCommands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MayaSelectiveOptions {
    pub passthrough: MayaSelectivePassthrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MayaTrackedSetAttrAttr {
    B,
    St,
    Stp,
    Ftn,
    Fn,
    F,
}

pub trait MayaSelectiveSetAttrSelector {
    fn classify(&self, attr_path: &str) -> Option<MayaTrackedSetAttrAttr>;
}

impl<F> MayaSelectiveSetAttrSelector for F
where
    F: Fn(&str) -> Option<MayaTrackedSetAttrAttr>,
{
    fn classify(&self, attr_path: &str) -> Option<MayaTrackedSetAttrAttr> {
        self(attr_path)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultMayaSelectiveSetAttrSelector;

impl MayaSelectiveSetAttrSelector for DefaultMayaSelectiveSetAttrSelector {
    fn classify(&self, attr_path: &str) -> Option<MayaTrackedSetAttrAttr> {
        let suffix = attr_path.rsplit('.').next()?;
        match suffix {
            "b" => Some(MayaTrackedSetAttrAttr::B),
            "st" => Some(MayaTrackedSetAttrAttr::St),
            "stp" => Some(MayaTrackedSetAttrAttr::Stp),
            "ftn" => Some(MayaTrackedSetAttrAttr::Ftn),
            "fn" => Some(MayaTrackedSetAttrAttr::Fn),
            "f" => Some(MayaTrackedSetAttrAttr::F),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MayaSelectiveItem {
    Requires(MayaSelectiveRequires),
    File(MayaSelectiveFile),
    CreateNode(MayaSelectiveCreateNode),
    SetAttr(MayaSelectiveSetAttr),
    OtherCommand {
        head_range: TextRange,
        span: TextRange,
    },
}

pub trait MayaSelectiveItemSink {
    fn on_item(&mut self, item: MayaSelectiveItem);
}

impl<F> MayaSelectiveItemSink for F
where
    F: FnMut(MayaSelectiveItem),
{
    fn on_item(&mut self, item: MayaSelectiveItem) {
        self(item);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaSelectiveRequires {
    pub head_range: TextRange,
    pub argument_ranges: Vec<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaSelectiveFile {
    pub head_range: TextRange,
    pub path_range: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaSelectiveCreateNode {
    pub head_range: TextRange,
    pub node_type_range: Option<TextRange>,
    pub name_range: Option<TextRange>,
    pub parent_range: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaSelectiveSetAttr {
    pub head_range: TextRange,
    pub attr_path_range: Option<TextRange>,
    pub type_name_range: Option<TextRange>,
    pub tracked_attr: Option<MayaTrackedSetAttrAttr>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MayaPromotionKind {
    FullParse,
    LightSynthesized,
    OpaqueTailPromoted,
    PolicyPromoted,
    CustomDeciderPromoted,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MayaPromotionPolicy {
    #[default]
    OpaqueTailOnly,
    ByCommandName(Vec<String>),
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaPromotionError {
    pub command_span: TextRange,
    pub head: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaPromotionDiagnostic {
    pub command_span: TextRange,
    pub head: Option<String>,
    pub attempted_kind: MayaPromotionKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaCommandValidationDiagnostic {
    pub command_span: TextRange,
    pub head: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MayaHybridTopLevelReport {
    pub facts: MayaTopLevelFacts,
    pub promotion_diagnostics: Vec<MayaPromotionDiagnostic>,
    pub validation_diagnostics: Vec<MayaCommandValidationDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MayaPromotionOptions {
    pub policy: MayaPromotionPolicy,
    pub parse_options: ParseOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MayaPromotionCandidate<'a> {
    pub command: &'a LightCommandSurface,
    pub raw_head: &'a str,
    pub canonical_name: Option<&'a str>,
}

pub trait MayaPromotionDecider {
    fn should_promote(&self, candidate: MayaPromotionCandidate<'_>) -> bool;
}

impl<F> MayaPromotionDecider for F
where
    F: for<'a> Fn(MayaPromotionCandidate<'a>) -> bool,
{
    fn should_promote(&self, candidate: MayaPromotionCandidate<'_>) -> bool {
        self(candidate)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopPromotionDecider;

impl MayaPromotionDecider for NoopPromotionDecider {
    fn should_promote(&self, _: MayaPromotionCandidate<'_>) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MayaTopLevelItem {
    Command(Box<MayaTopLevelCommand>),
    Proc {
        name: String,
        is_global: bool,
        span: TextRange,
    },
    Other {
        span: TextRange,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MayaLightTopLevelItem {
    Command(Box<MayaLightTopLevelCommand>),
    Proc {
        name: Option<String>,
        is_global: bool,
        span: TextRange,
    },
    Other {
        span: TextRange,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaTopLevelCommand {
    pub head: String,
    pub captured: bool,
    pub raw_items: Vec<MayaRawShellItem>,
    pub normalized: Option<MayaNormalizedCommand>,
    pub specialized: Option<MayaSpecializedCommand>,
    pub promotion_kind: MayaPromotionKind,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightTopLevelCommand {
    pub head: String,
    pub captured: bool,
    pub prefix_items: Vec<MayaRawShellItem>,
    pub opaque_tail: Option<TextRange>,
    pub specialized: Option<MayaLightSpecializedCommand>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaRawShellItem {
    pub source_text: String,
    pub value_text: Option<String>,
    pub kind: MayaRawShellItemKind,
    pub span: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MayaRawShellItemKind {
    Flag,
    Numeric,
    BareWord,
    QuotedString,
    Variable,
    GroupedExpr,
    BraceList,
    VectorLiteral,
    Capture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaPositionalArg {
    pub item: MayaRawShellItem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaNormalizedFlag {
    pub source_text: String,
    pub canonical_name: Option<String>,
    pub args: Vec<MayaPositionalArg>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightFlag {
    pub source_text: String,
    pub canonical_name: Option<String>,
    pub args: Vec<MayaPositionalArg>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MayaNormalizedCommandItem {
    Flag(MayaNormalizedFlag),
    Positional(MayaPositionalArg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaNormalizedCommand {
    pub head: String,
    head_range: TextRange,
    pub schema_name: String,
    pub kind: CommandKind,
    pub mode: CommandMode,
    pub items: Vec<MayaNormalizedCommandItem>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MayaSpecializedCommand {
    Requires(MayaRequiresCommand),
    CurrentUnit(MayaCurrentUnitCommand),
    FileInfo(MayaFileInfoCommand),
    CreateNode(MayaCreateNodeCommand),
    Rename(MayaRenameCommand),
    Select(MayaSelectCommand),
    SetAttr(MayaSetAttrCommand),
    AddAttr(MayaAddAttrCommand),
    ConnectAttr(MayaConnectAttrCommand),
    Relationship(MayaRelationshipCommand),
    File(MayaFileCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MayaLightSpecializedCommand {
    Requires(MayaLightRequiresCommand),
    CurrentUnit(MayaLightCurrentUnitCommand),
    FileInfo(MayaLightFileInfoCommand),
    CreateNode(MayaLightCreateNodeCommand),
    Rename(MayaLightRenameCommand),
    Select(MayaLightSelectCommand),
    SetAttr(MayaLightSetAttrCommand),
    AddAttr(MayaLightAddAttrCommand),
    ConnectAttr(MayaLightConnectAttrCommand),
    Relationship(MayaLightRelationshipCommand),
    File(MayaLightFileCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaRequiresCommand {
    pub requirements: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightRequiresCommand {
    pub requirements: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaCurrentUnitCommand {
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightCurrentUnitCommand {
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaFileInfoCommand {
    pub key: Option<MayaRawShellItem>,
    pub value: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightFileInfoCommand {
    pub key: Option<MayaRawShellItem>,
    pub value: Option<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaCreateNodeCommand {
    pub node_type: Option<MayaRawShellItem>,
    pub name: Option<MayaRawShellItem>,
    pub parent: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightCreateNodeCommand {
    pub node_type: Option<MayaRawShellItem>,
    pub name: Option<MayaRawShellItem>,
    pub parent: Option<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaRenameCommand {
    pub source: Option<MayaRawShellItem>,
    pub target: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightRenameCommand {
    pub source: Option<MayaRawShellItem>,
    pub target: Option<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaSelectCommand {
    pub targets: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightSelectCommand {
    pub targets: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaSetAttrCommand {
    pub attr_path: Option<MayaRawShellItem>,
    pub type_name: Option<MayaRawShellItem>,
    pub value_kind: MayaSetAttrValueKind,
    pub values: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightSetAttrCommand {
    pub attr_path: Option<MayaRawShellItem>,
    pub type_name: Option<MayaRawShellItem>,
    pub value_kind: MayaSetAttrValueKind,
    pub prefix_values: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MayaSetAttrValueKind {
    TypedNumbers,
    String,
    StringArray,
    Int32Array,
    ComponentList,
    OpaqueTyped,
    MatrixXform,
    DataReferenceEdits,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaAddAttrCommand {
    pub flags: Vec<MayaNormalizedFlag>,
    pub tail: Vec<MayaRawShellItem>,
    pub tail_kind: MayaAddAttrTailKind,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightAddAttrCommand {
    pub flags: Vec<MayaLightFlag>,
    pub tail: Vec<MayaRawShellItem>,
    pub tail_kind: MayaAddAttrTailKind,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MayaAddAttrTailKind {
    None,
    Numeric,
    String,
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaConnectAttrCommand {
    pub source_attr: Option<MayaRawShellItem>,
    pub target_attr: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightConnectAttrCommand {
    pub source_attr: Option<MayaRawShellItem>,
    pub target_attr: Option<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaRelationshipCommand {
    pub relationship: Option<MayaRawShellItem>,
    pub members: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightRelationshipCommand {
    pub relationship: Option<MayaRawShellItem>,
    pub members: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaFileCommand {
    pub path: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightFileCommand {
    pub path: Option<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[must_use]
pub fn collect_top_level_facts(parse: &Parse) -> MayaTopLevelFacts {
    collect_top_level_facts_with_registry(parse, &EmptyCommandRegistry)
}

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
    let mut bridge = MayaSelectiveBridge::new(options, selector, sink);
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
    let mut bridge = MayaSelectiveBridge::new(options, selector, sink);
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
    let mut bridge = MayaSelectiveBridge::new(options, selector, sink);
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
    let mut bridge = MayaSelectiveBridge::new(options, selector, sink);
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
    let mut bridge = MayaSelectiveBridge::new(options, selector, sink);
    scan_light_file_with_encoding_and_options_and_sink(path, encoding, light_options, &mut bridge)
}

struct MayaSelectiveBridge<'a, Sel: ?Sized, Sink: ?Sized> {
    options: &'a MayaSelectiveOptions,
    selector: &'a Sel,
    sink: &'a mut Sink,
}

impl<'a, Sel: ?Sized, Sink: ?Sized> MayaSelectiveBridge<'a, Sel, Sink> {
    fn new(options: &'a MayaSelectiveOptions, selector: &'a Sel, sink: &'a mut Sink) -> Self {
        Self {
            options,
            selector,
            sink,
        }
    }
}

impl<Sel, Sink> LightItemSink for MayaSelectiveBridge<'_, Sel, Sink>
where
    Sel: MayaSelectiveSetAttrSelector + ?Sized,
    Sink: MayaSelectiveItemSink + ?Sized,
{
    fn on_item(&mut self, source: mel_syntax::SourceView<'_>, item: LightItem) {
        let LightItem::Command(command) = item else {
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
    source: mel_syntax::SourceView<'_>,
    command: &LightCommandSurface,
    options: &MayaSelectiveOptions,
    selector: &(impl MayaSelectiveSetAttrSelector + ?Sized),
) -> Option<MayaSelectiveItem> {
    let head = source.slice(command.head_range);
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
            let tracked_attr = attr_path_range
                .and_then(|range| selector.classify(strip_outer_quotes(source.slice(range))));
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
    source: mel_syntax::SourceView<'_>,
    words: &[LightWord],
    names: &[&str],
) -> Option<TextRange> {
    let mut index = 0;
    while index < words.len() {
        let LightWord::Flag { text, .. } = &words[index] else {
            index += 1;
            continue;
        };
        let normalized = source.slice(*text).trim_start_matches('-');
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

#[must_use]
pub fn collect_top_level_facts_light(parse: &LightParse) -> MayaLightTopLevelFacts {
    collect_top_level_facts_light_with_registry(parse, &EmptyCommandRegistry)
}

pub fn collect_top_level_facts_hybrid(
    parse: &LightParse,
) -> Result<MayaTopLevelFacts, MayaPromotionError> {
    collect_top_level_facts_hybrid_with_registry_and_decider(
        parse,
        &EmptyCommandRegistry,
        &MayaPromotionOptions::default(),
        &NoopPromotionDecider,
    )
}

pub fn collect_top_level_facts_hybrid_with_decider<D>(
    parse: &LightParse,
    options: &MayaPromotionOptions,
    decider: &D,
) -> Result<MayaTopLevelFacts, MayaPromotionError>
where
    D: MayaPromotionDecider + ?Sized,
{
    collect_top_level_facts_hybrid_with_registry_and_decider(
        parse,
        &EmptyCommandRegistry,
        options,
        decider,
    )
}

pub fn collect_top_level_facts_hybrid_with_registry<R>(
    parse: &LightParse,
    registry: &R,
    policy: MayaPromotionPolicy,
) -> Result<MayaTopLevelFacts, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    collect_top_level_facts_hybrid_with_registry_and_options(
        parse,
        registry,
        &MayaPromotionOptions {
            policy,
            ..MayaPromotionOptions::default()
        },
    )
}

pub fn collect_top_level_facts_hybrid_with_registry_and_options<R>(
    parse: &LightParse,
    registry: &R,
    options: &MayaPromotionOptions,
) -> Result<MayaTopLevelFacts, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    collect_top_level_facts_hybrid_with_registry_and_decider(
        parse,
        registry,
        options,
        &NoopPromotionDecider,
    )
}

pub fn collect_top_level_facts_hybrid_with_registry_and_decider<R, D>(
    parse: &LightParse,
    registry: &R,
    options: &MayaPromotionOptions,
    decider: &D,
) -> Result<MayaTopLevelFacts, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
    D: MayaPromotionDecider + ?Sized,
{
    let overlay = OverlayRegistry::new(registry);
    let mut items = Vec::new();

    for item in &parse.source.items {
        match item {
            LightItem::Proc(proc_def) => items.push(MayaTopLevelItem::Proc {
                name: proc_def
                    .name_range
                    .map(|range| parse.source_slice(range).to_owned())
                    .unwrap_or_default(),
                is_global: proc_def.is_global,
                span: proc_def.span,
            }),
            LightItem::Command(command) => {
                let command = promote_or_synthesize_light_command(
                    parse,
                    command,
                    &overlay,
                    options,
                    decider,
                    PromotionErrorMode::Strict,
                )?;
                items.push(MayaTopLevelItem::Command(Box::new(command)));
            }
            LightItem::Other { span } => items.push(MayaTopLevelItem::Other { span: *span }),
        }
    }

    Ok(MayaTopLevelFacts { items })
}

pub fn collect_top_level_facts_hybrid_report(
    parse: &LightParse,
    options: &MayaPromotionOptions,
) -> MayaHybridTopLevelReport {
    collect_top_level_facts_hybrid_report_with_registry_and_decider(
        parse,
        &EmptyCommandRegistry,
        options,
        &NoopPromotionDecider,
    )
}

pub fn collect_top_level_facts_hybrid_report_with_decider<D>(
    parse: &LightParse,
    options: &MayaPromotionOptions,
    decider: &D,
) -> MayaHybridTopLevelReport
where
    D: MayaPromotionDecider + ?Sized,
{
    collect_top_level_facts_hybrid_report_with_registry_and_decider(
        parse,
        &EmptyCommandRegistry,
        options,
        decider,
    )
}

pub fn collect_top_level_facts_hybrid_report_with_registry<R>(
    parse: &LightParse,
    registry: &R,
    options: &MayaPromotionOptions,
) -> MayaHybridTopLevelReport
where
    R: CommandRegistry + ?Sized,
{
    collect_top_level_facts_hybrid_report_with_registry_and_decider(
        parse,
        registry,
        options,
        &NoopPromotionDecider,
    )
}

pub fn collect_top_level_facts_hybrid_report_with_registry_and_decider<R, D>(
    parse: &LightParse,
    registry: &R,
    options: &MayaPromotionOptions,
    decider: &D,
) -> MayaHybridTopLevelReport
where
    R: CommandRegistry + ?Sized,
    D: MayaPromotionDecider + ?Sized,
{
    let overlay = OverlayRegistry::new(registry);
    let mut items = Vec::new();
    let mut promotion_diagnostics = Vec::new();
    let mut validation_diagnostics = Vec::new();

    for item in &parse.source.items {
        match item {
            LightItem::Proc(proc_def) => items.push(MayaTopLevelItem::Proc {
                name: proc_def
                    .name_range
                    .map(|range| parse.source_slice(range).to_owned())
                    .unwrap_or_default(),
                is_global: proc_def.is_global,
                span: proc_def.span,
            }),
            LightItem::Command(command) => {
                let command = promote_or_synthesize_light_command(
                    parse,
                    command,
                    &overlay,
                    options,
                    decider,
                    PromotionErrorMode::Report(&mut promotion_diagnostics),
                )
                .expect("report mode must synthesize on promotion error");
                validation_diagnostics.extend(validate_maya_command(parse.source_view(), &command));
                items.push(MayaTopLevelItem::Command(Box::new(command)));
            }
            LightItem::Other { span } => items.push(MayaTopLevelItem::Other { span: *span }),
        }
    }

    MayaHybridTopLevelReport {
        facts: MayaTopLevelFacts { items },
        promotion_diagnostics,
        validation_diagnostics,
    }
}

pub fn promote_light_top_level_command_with_registry<R>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    promote_light_top_level_command_with_registry_and_decider(
        parse,
        command,
        registry,
        &MayaPromotionOptions::default(),
        &NoopPromotionDecider,
    )
}

pub fn promote_light_top_level_command_with_registry_and_options<R>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
    options: &MayaPromotionOptions,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    promote_light_top_level_command_with_registry_and_decider(
        parse,
        command,
        registry,
        options,
        &NoopPromotionDecider,
    )
}

pub fn promote_light_top_level_command_with_registry_and_decider<R, D>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
    options: &MayaPromotionOptions,
    decider: &D,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
    D: MayaPromotionDecider + ?Sized,
{
    let head = parse.source_slice(command.head_range).to_owned();
    let canonical_name = registry.lookup(&head).map(|schema| schema.name.clone());
    match promotion_attempt_kind(
        command,
        canonical_name.as_deref(),
        &head,
        &options.policy,
        decider,
    ) {
        Some(MayaPromotionKind::OpaqueTailPromoted) => promote_opaque_command_with_registry(
            parse,
            command,
            registry,
            options,
            MayaPromotionKind::OpaqueTailPromoted,
        ),
        Some(MayaPromotionKind::PolicyPromoted) => {
            if command.opaque_tail.is_some() {
                promote_opaque_command_with_registry(
                    parse,
                    command,
                    registry,
                    options,
                    MayaPromotionKind::PolicyPromoted,
                )
            } else {
                promote_policy_command_with_registry(parse, command, registry, options)
            }
        }
        Some(MayaPromotionKind::CustomDeciderPromoted) => {
            promote_custom_decider_command_with_registry(parse, command, registry, options)
        }
        Some(MayaPromotionKind::FullParse | MayaPromotionKind::LightSynthesized) | None => {
            build_nonopaque_top_level_command_with_registry(
                parse,
                command,
                registry,
                MayaPromotionKind::LightSynthesized,
            )
        }
    }
}

#[must_use]
pub fn collect_top_level_facts_light_with_registry<R>(
    parse: &LightParse,
    registry: &R,
) -> MayaLightTopLevelFacts
where
    R: CommandRegistry + ?Sized,
{
    let overlay = OverlayRegistry::new(registry);
    let mut items = Vec::new();

    for item in &parse.source.items {
        match item {
            LightItem::Proc(proc_def) => items.push(MayaLightTopLevelItem::Proc {
                name: proc_def
                    .name_range
                    .map(|range| parse.source_slice(range).to_owned()),
                is_global: proc_def.is_global,
                span: proc_def.span,
            }),
            LightItem::Command(command) => items.push(MayaLightTopLevelItem::Command(Box::new(
                maya_light_command_from_parse(parse, command, &overlay),
            ))),
            LightItem::Other { span } => items.push(MayaLightTopLevelItem::Other { span: *span }),
        }
    }

    MayaLightTopLevelFacts { items }
}

#[must_use]
pub fn collect_top_level_facts_with_registry<R>(parse: &Parse, registry: &R) -> MayaTopLevelFacts
where
    R: CommandRegistry + ?Sized,
{
    let overlay = OverlayRegistry::new(registry);
    let analysis = mel_sema::analyze_with_registry(&parse.syntax, parse.source_view(), &overlay);
    let mut remaining_normalized: Vec<Option<MayaNormalizedCommand>> = analysis
        .normalized_invokes
        .into_iter()
        .map(|invoke| Some(maya_normalized_command_from_parse(parse, invoke)))
        .collect();
    let mut items = Vec::new();

    for item in &parse.syntax.items {
        match item {
            Item::Proc(proc_def) => items.push(proc_item(parse, proc_def)),
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Proc { proc_def, .. } => items.push(proc_item(parse, proc_def)),
                Stmt::Expr { expr, .. } => {
                    let Expr::Invoke(invoke) = expr else {
                        continue;
                    };
                    if let InvokeSurface::ShellLike {
                        head_range,
                        words,
                        captured,
                    } = &invoke.surface
                    {
                        let head = parse.source_slice(*head_range).to_owned();
                        let normalized = take_matching_normalized(
                            &mut remaining_normalized,
                            *head_range,
                            invoke.range,
                        );
                        let raw_items = words
                            .iter()
                            .map(|word| raw_item_from_shell_word(parse, word))
                            .collect::<Vec<_>>();
                        let specialized = specialize_command(
                            &head,
                            invoke.range,
                            normalized.as_ref(),
                            &raw_items,
                        );
                        items.push(MayaTopLevelItem::Command(Box::new(MayaTopLevelCommand {
                            head,
                            captured: *captured,
                            raw_items,
                            normalized,
                            specialized,
                            promotion_kind: MayaPromotionKind::FullParse,
                            span: invoke.range,
                        })));
                    } else {
                        items.push(MayaTopLevelItem::Other {
                            span: stmt_range(stmt),
                        });
                    }
                }
                _ => items.push(MayaTopLevelItem::Other {
                    span: stmt_range(stmt),
                }),
            },
        }
    }

    MayaTopLevelFacts { items }
}

fn proc_item(parse: &Parse, proc_def: &ProcDef) -> MayaTopLevelItem {
    MayaTopLevelItem::Proc {
        name: parse.source_slice(proc_def.name_range).to_owned(),
        is_global: proc_def.is_global,
        span: proc_def.range,
    }
}

fn take_matching_normalized(
    invokes: &mut [Option<MayaNormalizedCommand>],
    head_range: TextRange,
    range: TextRange,
) -> Option<MayaNormalizedCommand> {
    let index = invokes.iter().position(|invoke| {
        invoke
            .as_ref()
            .is_some_and(|invoke| invoke.head_range == head_range && invoke.span == range)
    })?;
    invokes[index].take()
}

fn specialize_command(
    head: &str,
    span: TextRange,
    normalized: Option<&MayaNormalizedCommand>,
    raw_items: &[MayaRawShellItem],
) -> Option<MayaSpecializedCommand> {
    let normalized = normalized?;
    let flags = normalized_flags(normalized);
    let positionals = normalized_positionals(normalized);

    match normalized.schema_name.as_str() {
        "requires" => Some(MayaSpecializedCommand::Requires(MayaRequiresCommand {
            requirements: positionals,
            flags,
            span,
        })),
        "currentUnit" => Some(MayaSpecializedCommand::CurrentUnit(
            MayaCurrentUnitCommand { flags, span },
        )),
        "fileInfo" => Some(MayaSpecializedCommand::FileInfo(MayaFileInfoCommand {
            key: positionals.first().cloned(),
            value: positionals.get(1).cloned(),
            flags,
            span,
        })),
        "createNode" => Some(MayaSpecializedCommand::CreateNode(MayaCreateNodeCommand {
            node_type: positionals.first().cloned(),
            name: first_flag_arg(&flags, "name"),
            parent: first_flag_arg(&flags, "parent"),
            flags,
            span,
        })),
        "rename" => Some(MayaSpecializedCommand::Rename(MayaRenameCommand {
            source: positionals.first().cloned(),
            target: positionals.get(1).cloned(),
            flags,
            span,
        })),
        "select" => Some(MayaSpecializedCommand::Select(MayaSelectCommand {
            targets: positionals,
            flags,
            span,
        })),
        "setAttr" => Some(MayaSpecializedCommand::SetAttr(specialize_set_attr(
            span,
            &flags,
            &positionals,
        ))),
        "addAttr" => Some(MayaSpecializedCommand::AddAttr(MayaAddAttrCommand {
            tail_kind: classify_add_attr_tail(&positionals),
            tail: positionals,
            flags,
            span,
        })),
        "connectAttr" => Some(MayaSpecializedCommand::ConnectAttr(
            MayaConnectAttrCommand {
                source_attr: positionals.first().cloned(),
                target_attr: positionals.get(1).cloned(),
                flags,
                span,
            },
        )),
        "relationship" => Some(MayaSpecializedCommand::Relationship(
            MayaRelationshipCommand {
                relationship: positionals.first().cloned(),
                members: positionals.into_iter().skip(1).collect(),
                flags,
                span,
            },
        )),
        "file" => Some(MayaSpecializedCommand::File(MayaFileCommand {
            path: positionals
                .last()
                .cloned()
                .or_else(|| raw_items.last().cloned()),
            flags,
            span,
        })),
        _ => {
            let _ = head;
            None
        }
    }
}

fn specialize_set_attr(
    span: TextRange,
    flags: &[MayaNormalizedFlag],
    positionals: &[MayaRawShellItem],
) -> MayaSetAttrCommand {
    let attr_path = positionals.first().cloned();
    let values = positionals.iter().skip(1).cloned().collect::<Vec<_>>();
    let type_name = first_flag_arg(flags, "type");
    let type_text = type_name
        .as_ref()
        .and_then(|item| item.value_text.as_deref())
        .unwrap_or_default();
    let value_kind = match type_text {
        "string" if values.len() == 1 => MayaSetAttrValueKind::String,
        "stringArray" => MayaSetAttrValueKind::StringArray,
        "Int32Array" => MayaSetAttrValueKind::Int32Array,
        "componentList" => MayaSetAttrValueKind::ComponentList,
        "matrix" | "matrixXform" => MayaSetAttrValueKind::MatrixXform,
        "dataReferenceEdits" => MayaSetAttrValueKind::DataReferenceEdits,
        "" if values.len() == 1 && matches!(values[0].kind, MayaRawShellItemKind::QuotedString) => {
            MayaSetAttrValueKind::String
        }
        _ if values.iter().all(is_numeric_like) => MayaSetAttrValueKind::TypedNumbers,
        _ if !type_text.is_empty() => MayaSetAttrValueKind::OpaqueTyped,
        _ => MayaSetAttrValueKind::Unknown,
    };

    MayaSetAttrCommand {
        attr_path,
        type_name,
        value_kind,
        values,
        flags: flags.to_vec(),
        span,
    }
}

fn classify_add_attr_tail(positionals: &[MayaRawShellItem]) -> MayaAddAttrTailKind {
    if positionals.is_empty() {
        return MayaAddAttrTailKind::None;
    }
    if positionals.iter().all(is_numeric_like) {
        return MayaAddAttrTailKind::Numeric;
    }
    if positionals
        .iter()
        .all(|item| matches!(item.kind, MayaRawShellItemKind::QuotedString))
    {
        return MayaAddAttrTailKind::String;
    }
    MayaAddAttrTailKind::Mixed
}

fn first_flag_arg(flags: &[MayaNormalizedFlag], canonical_name: &str) -> Option<MayaRawShellItem> {
    flags
        .iter()
        .find(|flag| flag.canonical_name.as_deref() == Some(canonical_name))
        .and_then(|flag| flag.args.first())
        .map(|arg| arg.item.clone())
}

fn normalized_flags(command: &MayaNormalizedCommand) -> Vec<MayaNormalizedFlag> {
    command
        .items
        .iter()
        .filter_map(|item| match item {
            MayaNormalizedCommandItem::Flag(flag) => Some(flag.clone()),
            MayaNormalizedCommandItem::Positional(_) => None,
        })
        .collect()
}

fn normalized_positionals(command: &MayaNormalizedCommand) -> Vec<MayaRawShellItem> {
    command
        .items
        .iter()
        .filter_map(|item| match item {
            MayaNormalizedCommandItem::Flag(_) => None,
            MayaNormalizedCommandItem::Positional(arg) => Some(arg.item.clone()),
        })
        .collect()
}

fn is_numeric_like(item: &MayaRawShellItem) -> bool {
    matches!(item.kind, MayaRawShellItemKind::Numeric)
}

fn raw_item_from_shell_word(parse: &Parse, word: &ShellWord) -> MayaRawShellItem {
    raw_item_from_shell_word_with_source(parse.source_view(), word)
}

fn raw_item_from_shell_word_with_source(
    source: mel_syntax::SourceView<'_>,
    word: &ShellWord,
) -> MayaRawShellItem {
    let (value_text, kind, span) = match word {
        ShellWord::Flag { range, .. } => (None, MayaRawShellItemKind::Flag, *range),
        ShellWord::NumericLiteral { text, range } => (
            Some(source.slice(*text).to_owned()),
            MayaRawShellItemKind::Numeric,
            *range,
        ),
        ShellWord::BareWord { text, range } => (
            Some(source.slice(*text).to_owned()),
            MayaRawShellItemKind::BareWord,
            *range,
        ),
        ShellWord::QuotedString { text, range } => (
            source
                .slice(*text)
                .strip_prefix('"')
                .and_then(|text| text.strip_suffix('"'))
                .map(str::to_owned),
            MayaRawShellItemKind::QuotedString,
            *range,
        ),
        ShellWord::Variable { range, .. } => (None, MayaRawShellItemKind::Variable, *range),
        ShellWord::GroupedExpr { range, .. } => (None, MayaRawShellItemKind::GroupedExpr, *range),
        ShellWord::BraceList { range, .. } => (None, MayaRawShellItemKind::BraceList, *range),
        ShellWord::VectorLiteral { range, .. } => {
            (None, MayaRawShellItemKind::VectorLiteral, *range)
        }
        ShellWord::Capture { range, .. } => (None, MayaRawShellItemKind::Capture, *range),
    };
    MayaRawShellItem {
        source_text: source.display_slice(span).to_owned(),
        value_text,
        kind,
        span,
    }
}

fn validate_maya_command(
    source: mel_syntax::SourceView<'_>,
    command: &MayaTopLevelCommand,
) -> Vec<MayaCommandValidationDiagnostic> {
    match command.head.as_str() {
        "setAttr" => validate_set_attr_command(source, command),
        _ => Vec::new(),
    }
}

fn validate_set_attr_command(
    source: mel_syntax::SourceView<'_>,
    command: &MayaTopLevelCommand,
) -> Vec<MayaCommandValidationDiagnostic> {
    let Some(normalized) = &command.normalized else {
        return Vec::new();
    };
    let positionals = normalized_positionals(normalized);
    if positionals.is_empty() {
        return Vec::new();
    }

    let values = &positionals[1..];
    if !values.is_empty() {
        return Vec::new();
    }

    let has_type_flag = normalized_flags(normalized)
        .iter()
        .any(|flag| flag.canonical_name.as_deref() == Some("type"));
    let message = if has_type_flag {
        "setAttr requires at least one value after the attribute path when -type is present"
            .to_owned()
    } else {
        "setAttr requires at least one value after the attribute path".to_owned()
    };

    vec![MayaCommandValidationDiagnostic {
        command_span: command.span,
        head: Some(source.slice(normalized.head_range).to_owned()),
        message,
    }]
}

fn stmt_range(stmt: &Stmt) -> TextRange {
    match stmt {
        Stmt::Empty { range }
        | Stmt::Proc { range, .. }
        | Stmt::Block { range, .. }
        | Stmt::Expr { range, .. }
        | Stmt::VarDecl { range, .. }
        | Stmt::If { range, .. }
        | Stmt::While { range, .. }
        | Stmt::DoWhile { range, .. }
        | Stmt::Switch { range, .. }
        | Stmt::For { range, .. }
        | Stmt::ForIn { range, .. }
        | Stmt::Return { range, .. }
        | Stmt::Break { range }
        | Stmt::Continue { range } => *range,
    }
}

fn maya_normalized_command_from_parse(
    parse: &Parse,
    value: mel_sema::NormalizedCommandInvoke,
) -> MayaNormalizedCommand {
    maya_normalized_command_from_source(parse.source_view(), value)
}

fn maya_normalized_command_from_source(
    source: mel_syntax::SourceView<'_>,
    value: mel_sema::NormalizedCommandInvoke,
) -> MayaNormalizedCommand {
    MayaNormalizedCommand {
        head: source.slice(value.head_range).to_owned(),
        head_range: value.head_range,
        schema_name: value.schema_name.to_string(),
        kind: value.kind,
        mode: value.mode,
        items: value
            .items
            .into_iter()
            .map(|item| maya_normalized_command_item_from_source(source, item))
            .collect(),
        span: value.range,
    }
}

fn maya_normalized_command_item_from_source(
    source: mel_syntax::SourceView<'_>,
    value: NormalizedCommandItem,
) -> MayaNormalizedCommandItem {
    match value {
        NormalizedCommandItem::Flag(flag) => {
            MayaNormalizedCommandItem::Flag(maya_normalized_flag_from_source(source, flag))
        }
        NormalizedCommandItem::Positional(arg) => {
            MayaNormalizedCommandItem::Positional(maya_positional_arg_from_source(source, arg))
        }
    }
}

fn maya_normalized_flag_from_source(
    source: mel_syntax::SourceView<'_>,
    value: NormalizedFlag,
) -> MayaNormalizedFlag {
    MayaNormalizedFlag {
        source_text: source.display_slice(value.source_range).to_owned(),
        canonical_name: value.canonical_name.map(|name| name.to_string()),
        args: value
            .args
            .into_iter()
            .map(|arg| maya_positional_arg_from_source(source, arg))
            .collect(),
        span: value.range,
    }
}

fn maya_positional_arg_from_source(
    source: mel_syntax::SourceView<'_>,
    value: PositionalArg,
) -> MayaPositionalArg {
    MayaPositionalArg {
        item: raw_item_from_shell_word_with_source(source, &value.word),
    }
}

enum PromotionErrorMode<'a> {
    Strict,
    Report(&'a mut Vec<MayaPromotionDiagnostic>),
}

fn promote_or_synthesize_light_command<R, D>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
    options: &MayaPromotionOptions,
    decider: &D,
    error_mode: PromotionErrorMode<'_>,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
    D: MayaPromotionDecider + ?Sized,
{
    let head = parse.source_slice(command.head_range).to_owned();
    let canonical_name = registry.lookup(&head).map(|schema| schema.name.clone());
    let attempted_kind = promotion_attempt_kind(
        command,
        canonical_name.as_deref(),
        &head,
        &options.policy,
        decider,
    );
    let Some(attempted_kind) = attempted_kind else {
        return build_nonopaque_top_level_command_with_registry(
            parse,
            command,
            registry,
            MayaPromotionKind::LightSynthesized,
        );
    };

    match promote_light_top_level_command_with_registry_and_decider(
        parse, command, registry, options, decider,
    ) {
        Ok(command) => Ok(command),
        Err(error) => match error_mode {
            PromotionErrorMode::Strict => Err(error),
            PromotionErrorMode::Report(diagnostics) => {
                diagnostics.push(MayaPromotionDiagnostic {
                    command_span: error.command_span,
                    head: error.head.clone(),
                    attempted_kind,
                    message: error.message,
                });
                build_nonopaque_top_level_command_with_registry(
                    parse,
                    command,
                    registry,
                    MayaPromotionKind::LightSynthesized,
                )
            }
        },
    }
}

fn promotion_attempt_kind<D>(
    command: &LightCommandSurface,
    canonical_name: Option<&str>,
    raw_head: &str,
    policy: &MayaPromotionPolicy,
    decider: &D,
) -> Option<MayaPromotionKind>
where
    D: MayaPromotionDecider + ?Sized,
{
    if command.opaque_tail.is_some() {
        return Some(match policy {
            MayaPromotionPolicy::OpaqueTailOnly => MayaPromotionKind::OpaqueTailPromoted,
            MayaPromotionPolicy::Always => MayaPromotionKind::PolicyPromoted,
            MayaPromotionPolicy::ByCommandName(names) => {
                if names
                    .iter()
                    .any(|name| Some(name.as_str()) == canonical_name || name == raw_head)
                {
                    MayaPromotionKind::PolicyPromoted
                } else {
                    MayaPromotionKind::OpaqueTailPromoted
                }
            }
        });
    }

    match policy {
        MayaPromotionPolicy::OpaqueTailOnly => None,
        MayaPromotionPolicy::Always => Some(MayaPromotionKind::PolicyPromoted),
        MayaPromotionPolicy::ByCommandName(names) => names
            .iter()
            .find(|name| Some(name.as_str()) == canonical_name || *name == raw_head)
            .map(|_| MayaPromotionKind::PolicyPromoted),
    }
    .or_else(|| {
        decider
            .should_promote(MayaPromotionCandidate {
                command,
                raw_head,
                canonical_name,
            })
            .then_some(MayaPromotionKind::CustomDeciderPromoted)
    })
}

fn promote_custom_decider_command_with_registry<R>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
    options: &MayaPromotionOptions,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    promote_parsed_command_with_registry(
        parse,
        command,
        registry,
        options,
        MayaPromotionKind::CustomDeciderPromoted,
    )
}

fn build_nonopaque_top_level_command_with_registry<R>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
    promotion_kind: MayaPromotionKind,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    let head = parse.source_slice(command.head_range).to_owned();
    let raw_items = command
        .words
        .iter()
        .map(|word| raw_item_from_light_word(parse, word))
        .collect::<Vec<_>>();
    let promoted_span = command_payload_span(command.head_range, &raw_items);
    let normalized = registry.lookup(&head).map(|schema| {
        normalize_light_command(&head, command.head_range, promoted_span, schema, &raw_items)
    });
    let specialized = specialize_command(&head, promoted_span, normalized.as_ref(), &raw_items);

    Ok(MayaTopLevelCommand {
        head,
        captured: command.captured,
        raw_items,
        normalized,
        specialized,
        promotion_kind,
        span: promoted_span,
    })
}

fn promote_opaque_command_with_registry<R>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
    options: &MayaPromotionOptions,
    promotion_kind: MayaPromotionKind,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    promote_parsed_command_with_registry(parse, command, registry, options, promotion_kind)
}

fn promote_policy_command_with_registry<R>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
    options: &MayaPromotionOptions,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    promote_parsed_command_with_registry(
        parse,
        command,
        registry,
        options,
        MayaPromotionKind::PolicyPromoted,
    )
}

fn promote_parsed_command_with_registry<R>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
    options: &MayaPromotionOptions,
    promotion_kind: MayaPromotionKind,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    let head = parse.source_slice(command.head_range).to_owned();
    let slice = parse_source_view_range_with_options(
        parse.source_view(),
        command.span,
        options.parse_options,
    );
    if !slice.lex_errors.is_empty() || !slice.errors.is_empty() {
        return Err(MayaPromotionError {
            command_span: command.span,
            head: Some(head),
            message: "promoted command did not parse cleanly".to_owned(),
        });
    }

    let overlay = OverlayRegistry::new(registry);
    let analysis = mel_sema::analyze_with_registry(&slice.syntax, parse.source_view(), &overlay);
    let mut remaining_normalized: Vec<Option<MayaNormalizedCommand>> = analysis
        .normalized_invokes
        .into_iter()
        .map(|invoke| {
            Some(maya_normalized_command_from_source(
                parse.source_view(),
                invoke,
            ))
        })
        .collect();

    let Some(item) = slice.syntax.items.first() else {
        return Err(MayaPromotionError {
            command_span: command.span,
            head: Some(head),
            message: "promoted command slice was empty".to_owned(),
        });
    };
    if slice.syntax.items.len() != 1 {
        return Err(MayaPromotionError {
            command_span: command.span,
            head: Some(head),
            message: "promoted command slice contained multiple top-level items".to_owned(),
        });
    }

    let Item::Stmt(stmt) = item else {
        return Err(MayaPromotionError {
            command_span: command.span,
            head: Some(head),
            message: "promoted command slice was not a statement".to_owned(),
        });
    };
    let Stmt::Expr { expr, .. } = &**stmt else {
        return Err(MayaPromotionError {
            command_span: command.span,
            head: Some(head),
            message: "promoted command slice was not an invoke statement".to_owned(),
        });
    };
    let Expr::Invoke(invoke) = expr else {
        return Err(MayaPromotionError {
            command_span: command.span,
            head: Some(head),
            message: "promoted command slice was not an invoke statement".to_owned(),
        });
    };
    let InvokeSurface::ShellLike {
        head_range,
        words,
        captured,
    } = &invoke.surface
    else {
        return Err(MayaPromotionError {
            command_span: command.span,
            head: Some(head),
            message: "promoted command slice was not shell-like".to_owned(),
        });
    };

    let promoted_head = parse.source_slice(*head_range).to_owned();
    let normalized = take_matching_normalized(&mut remaining_normalized, *head_range, invoke.range);
    let raw_items = words
        .iter()
        .map(|word| raw_item_from_shell_word_with_source(parse.source_view(), word))
        .collect::<Vec<_>>();
    let specialized = specialize_command(
        &promoted_head,
        invoke.range,
        normalized.as_ref(),
        &raw_items,
    );

    Ok(MayaTopLevelCommand {
        head: promoted_head,
        captured: *captured,
        raw_items,
        normalized,
        specialized,
        promotion_kind,
        span: invoke.range,
    })
}

fn normalize_light_command(
    head: &str,
    head_range: TextRange,
    span: TextRange,
    schema: &CommandSchema,
    items: &[MayaRawShellItem],
) -> MayaNormalizedCommand {
    let mode = detect_light_mode(schema, items);
    let mut normalized_items = Vec::new();
    let mut index = 0;

    while index < items.len() {
        let item = &items[index];
        if item.kind != MayaRawShellItemKind::Flag {
            normalized_items.push(MayaNormalizedCommandItem::Positional(MayaPositionalArg {
                item: item.clone(),
            }));
            index += 1;
            continue;
        }

        let schema_flag = find_flag_schema(schema, &item.source_text);
        let expected_arity = schema_flag
            .as_ref()
            .map(|flag| arity_for_mode(flag.arity_by_mode, mode))
            .unwrap_or(FlagArity::None);
        let (_, max_arity) = arity_bounds(expected_arity);
        let mut args = Vec::new();
        let mut consumed = 0;
        while consumed < max_arity {
            let Some(next_item) = items.get(index + 1 + consumed) else {
                break;
            };
            if next_item.kind == MayaRawShellItemKind::Flag {
                break;
            }
            args.push(MayaPositionalArg {
                item: next_item.clone(),
            });
            consumed += 1;
        }
        let item_span = args.last().map_or(item.span, |arg| {
            text_range(range_start(item.span), range_end(arg.item.span))
        });
        normalized_items.push(MayaNormalizedCommandItem::Flag(MayaNormalizedFlag {
            source_text: item.source_text.clone(),
            canonical_name: schema_flag.as_ref().map(|flag| flag.long_name.to_string()),
            args,
            span: item_span,
        }));
        index += 1 + consumed;
    }

    let normalized_span = normalized_items.last().map_or(span, |item| {
        let end = match item {
            MayaNormalizedCommandItem::Flag(flag) => range_end(flag.span),
            MayaNormalizedCommandItem::Positional(arg) => range_end(arg.item.span),
        };
        text_range(range_start(head_range), end)
    });

    MayaNormalizedCommand {
        head: head.to_owned(),
        head_range,
        schema_name: schema.name.to_string(),
        kind: schema.kind,
        mode,
        items: normalized_items,
        span: normalized_span,
    }
}

fn command_payload_span(head_range: TextRange, raw_items: &[MayaRawShellItem]) -> TextRange {
    let end = raw_items
        .last()
        .map(|item| range_end(item.span))
        .unwrap_or_else(|| range_end(head_range));
    text_range(range_start(head_range), end)
}

fn maya_light_command_from_parse<R>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
) -> MayaLightTopLevelCommand
where
    R: CommandRegistry + ?Sized,
{
    let head = parse.source_slice(command.head_range).to_owned();
    let prefix_items = command
        .words
        .iter()
        .map(|word| raw_item_from_light_word(parse, word))
        .collect::<Vec<_>>();
    let specialized = registry.lookup(&head).and_then(|schema| {
        specialize_light_command(
            &head,
            command.span,
            command.opaque_tail,
            schema,
            &prefix_items,
        )
    });

    MayaLightTopLevelCommand {
        head,
        captured: command.captured,
        prefix_items,
        opaque_tail: command.opaque_tail,
        specialized,
        span: command.span,
    }
}

fn raw_item_from_light_word(parse: &LightParse, word: &LightWord) -> MayaRawShellItem {
    let span = word.range();
    let (value_text, kind) = match word {
        LightWord::Flag { .. } => (None, MayaRawShellItemKind::Flag),
        LightWord::NumericLiteral { text, .. } => (
            Some(parse.source_slice(*text).to_owned()),
            MayaRawShellItemKind::Numeric,
        ),
        LightWord::BareWord { text, .. } => (
            Some(parse.source_slice(*text).to_owned()),
            MayaRawShellItemKind::BareWord,
        ),
        LightWord::QuotedString { text, .. } => (
            parse.string_literal_contents(*text).map(str::to_owned),
            MayaRawShellItemKind::QuotedString,
        ),
        LightWord::Variable { .. } => (None, MayaRawShellItemKind::Variable),
        LightWord::GroupedExpr { .. } => (None, MayaRawShellItemKind::GroupedExpr),
        LightWord::BraceList { .. } => (None, MayaRawShellItemKind::BraceList),
        LightWord::VectorLiteral { .. } => (None, MayaRawShellItemKind::VectorLiteral),
        LightWord::Capture { .. } => (None, MayaRawShellItemKind::Capture),
    };
    MayaRawShellItem {
        source_text: parse.display_slice(span).to_owned(),
        value_text,
        kind,
        span,
    }
}

fn specialize_light_command(
    head: &str,
    span: TextRange,
    opaque_tail: Option<TextRange>,
    schema: &CommandSchema,
    prefix_items: &[MayaRawShellItem],
) -> Option<MayaLightSpecializedCommand> {
    let (flags, positionals) = normalize_light_items(schema, prefix_items);
    match schema.name.as_ref() {
        "requires" => Some(MayaLightSpecializedCommand::Requires(
            MayaLightRequiresCommand {
                requirements: positionals,
                flags,
                opaque_tail,
                span,
            },
        )),
        "currentUnit" => Some(MayaLightSpecializedCommand::CurrentUnit(
            MayaLightCurrentUnitCommand {
                flags,
                opaque_tail,
                span,
            },
        )),
        "fileInfo" => Some(MayaLightSpecializedCommand::FileInfo(
            MayaLightFileInfoCommand {
                key: positionals.first().cloned(),
                value: positionals.get(1).cloned(),
                flags,
                opaque_tail,
                span,
            },
        )),
        "createNode" => Some(MayaLightSpecializedCommand::CreateNode(
            MayaLightCreateNodeCommand {
                node_type: positionals.first().cloned(),
                name: first_light_flag_arg(&flags, "name"),
                parent: first_light_flag_arg(&flags, "parent"),
                flags,
                opaque_tail,
                span,
            },
        )),
        "rename" => Some(MayaLightSpecializedCommand::Rename(
            MayaLightRenameCommand {
                source: positionals.first().cloned(),
                target: positionals.get(1).cloned(),
                flags,
                opaque_tail,
                span,
            },
        )),
        "select" => Some(MayaLightSpecializedCommand::Select(
            MayaLightSelectCommand {
                targets: positionals,
                flags,
                opaque_tail,
                span,
            },
        )),
        "setAttr" => Some(MayaLightSpecializedCommand::SetAttr(
            specialize_light_set_attr(span, opaque_tail, &flags, &positionals),
        )),
        "addAttr" => Some(MayaLightSpecializedCommand::AddAttr(
            MayaLightAddAttrCommand {
                tail_kind: classify_add_attr_tail(&positionals),
                tail: positionals,
                flags,
                opaque_tail,
                span,
            },
        )),
        "connectAttr" => Some(MayaLightSpecializedCommand::ConnectAttr(
            MayaLightConnectAttrCommand {
                source_attr: positionals.first().cloned(),
                target_attr: positionals.get(1).cloned(),
                flags,
                opaque_tail,
                span,
            },
        )),
        "relationship" => Some(MayaLightSpecializedCommand::Relationship(
            MayaLightRelationshipCommand {
                relationship: positionals.first().cloned(),
                members: positionals.into_iter().skip(1).collect(),
                flags,
                opaque_tail,
                span,
            },
        )),
        "file" => Some(MayaLightSpecializedCommand::File(MayaLightFileCommand {
            path: positionals.last().cloned(),
            flags,
            opaque_tail,
            span,
        })),
        _ => {
            let _ = head;
            None
        }
    }
}

fn specialize_light_set_attr(
    span: TextRange,
    opaque_tail: Option<TextRange>,
    flags: &[MayaLightFlag],
    positionals: &[MayaRawShellItem],
) -> MayaLightSetAttrCommand {
    let attr_path = positionals.first().cloned();
    let prefix_values = positionals.iter().skip(1).cloned().collect::<Vec<_>>();
    let type_name = first_light_flag_arg(flags, "type");
    let type_text = type_name
        .as_ref()
        .and_then(|item| item.value_text.as_deref())
        .unwrap_or_default();
    let value_kind = match type_text {
        "string" if prefix_values.len() == 1 && opaque_tail.is_none() => {
            MayaSetAttrValueKind::String
        }
        "stringArray" => MayaSetAttrValueKind::StringArray,
        "Int32Array" => MayaSetAttrValueKind::Int32Array,
        "componentList" => MayaSetAttrValueKind::ComponentList,
        "matrix" | "matrixXform" => MayaSetAttrValueKind::MatrixXform,
        "dataReferenceEdits" => MayaSetAttrValueKind::DataReferenceEdits,
        "" if prefix_values.len() == 1
            && opaque_tail.is_none()
            && matches!(prefix_values[0].kind, MayaRawShellItemKind::QuotedString) =>
        {
            MayaSetAttrValueKind::String
        }
        _ if opaque_tail.is_none() && prefix_values.iter().all(is_numeric_like) => {
            MayaSetAttrValueKind::TypedNumbers
        }
        _ if !type_text.is_empty() => MayaSetAttrValueKind::OpaqueTyped,
        _ => MayaSetAttrValueKind::Unknown,
    };

    MayaLightSetAttrCommand {
        attr_path,
        type_name,
        value_kind,
        prefix_values,
        flags: flags.to_vec(),
        opaque_tail,
        span,
    }
}

fn normalize_light_items(
    schema: &CommandSchema,
    items: &[MayaRawShellItem],
) -> (Vec<MayaLightFlag>, Vec<MayaRawShellItem>) {
    let mode = detect_light_mode(schema, items);
    let mut index = 0;
    let mut flags = Vec::new();
    let mut positionals = Vec::new();

    while index < items.len() {
        let item = &items[index];
        if item.kind != MayaRawShellItemKind::Flag {
            positionals.push(item.clone());
            index += 1;
            continue;
        }

        let schema_flag = find_flag_schema(schema, &item.source_text);
        let expected_arity = schema_flag
            .as_ref()
            .map(|flag| arity_for_mode(flag.arity_by_mode, mode))
            .unwrap_or(FlagArity::None);
        let (_, max_arity) = arity_bounds(expected_arity);
        let mut args = Vec::new();
        let mut consumed = 0;
        while consumed < max_arity {
            let Some(next_item) = items.get(index + 1 + consumed) else {
                break;
            };
            if next_item.kind == MayaRawShellItemKind::Flag {
                break;
            }
            args.push(MayaPositionalArg {
                item: next_item.clone(),
            });
            consumed += 1;
        }
        let span = args.last().map_or(item.span, |arg| {
            text_range(range_start(item.span), range_end(arg.item.span))
        });
        flags.push(MayaLightFlag {
            source_text: item.source_text.clone(),
            canonical_name: schema_flag.as_ref().map(|flag| flag.long_name.to_string()),
            args,
            span,
        });
        index += 1 + consumed;
    }

    (flags, positionals)
}

fn detect_light_mode(schema: &CommandSchema, items: &[MayaRawShellItem]) -> CommandMode {
    let mut create = false;
    let mut edit = false;
    let mut query = false;
    for item in items {
        if item.kind != MayaRawShellItemKind::Flag {
            continue;
        }
        match item.source_text.trim_start_matches('-') {
            "create" | "c" if schema.mode_mask.create => create = true,
            "edit" | "e" if schema.mode_mask.edit => edit = true,
            "query" | "q" if schema.mode_mask.query => query = true,
            _ => {}
        }
    }
    match (create, edit, query) {
        (false, false, false) | (true, false, false) => CommandMode::Create,
        (false, true, false) => CommandMode::Edit,
        (false, false, true) => CommandMode::Query,
        _ => CommandMode::Unknown,
    }
}

fn find_flag_schema(command: &CommandSchema, text: &str) -> Option<FlagSchema> {
    let normalized = text.strip_prefix('-').unwrap_or(text);
    command
        .flags
        .iter()
        .find(|flag| {
            normalized == flag.long_name.as_ref()
                || flag
                    .short_name
                    .as_deref()
                    .is_some_and(|short| short == normalized)
        })
        .cloned()
        .or_else(|| synthetic_mode_flag_for_name(command, normalized))
}

fn synthetic_mode_flag_for_name(command: &CommandSchema, name: &str) -> Option<FlagSchema> {
    match name {
        "create" | "c" if command.mode_mask.create => Some(synthetic_mode_flag("create", "c")),
        "edit" | "e" if command.mode_mask.edit => Some(synthetic_mode_flag("edit", "e")),
        "query" | "q" if command.mode_mask.query => Some(synthetic_mode_flag("query", "q")),
        _ => None,
    }
}

fn synthetic_mode_flag(long_name: &str, short_name: &str) -> FlagSchema {
    FlagSchema {
        long_name: long_name.into(),
        short_name: Some(short_name.into()),
        mode_mask: CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        arity_by_mode: FlagArityByMode {
            create: FlagArity::None,
            edit: FlagArity::None,
            query: FlagArity::None,
        },
        value_shapes: Vec::new().into(),
        allows_multiple: false,
    }
}

fn arity_for_mode(arity_by_mode: FlagArityByMode, mode: CommandMode) -> FlagArity {
    match mode {
        CommandMode::Create | CommandMode::Unknown => arity_by_mode.create,
        CommandMode::Edit => arity_by_mode.edit,
        CommandMode::Query => arity_by_mode.query,
    }
}

fn arity_bounds(arity: FlagArity) -> (usize, usize) {
    match arity {
        FlagArity::None => (0, 0),
        FlagArity::Exact(value) => (usize::from(value), usize::from(value)),
        FlagArity::Range { min, max } => (usize::from(min), usize::from(max)),
    }
}

fn first_light_flag_arg(flags: &[MayaLightFlag], canonical_name: &str) -> Option<MayaRawShellItem> {
    flags
        .iter()
        .find(|flag| flag.canonical_name.as_deref() == Some(canonical_name))
        .and_then(|flag| flag.args.first())
        .map(|arg| arg.item.clone())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EmbeddedFlagSchema {
    long_name: &'static str,
    short_name: Option<&'static str>,
    mode_mask: CommandModeMask,
    arity_by_mode: FlagArityByMode,
    value_shapes: &'static [ValueShape],
    allows_multiple: bool,
}

impl EmbeddedFlagSchema {
    fn to_shared_schema(self) -> FlagSchema {
        FlagSchema {
            long_name: self.long_name.into(),
            short_name: self.short_name.map(Into::into),
            mode_mask: self.mode_mask,
            arity_by_mode: self.arity_by_mode,
            value_shapes: self.value_shapes.into(),
            allows_multiple: self.allows_multiple,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EmbeddedCommandSchema {
    name: &'static str,
    kind: CommandKind,
    source_kind: CommandSourceKind,
    mode_mask: CommandModeMask,
    return_behavior: ReturnBehavior,
    positionals: PositionalSchema,
    flags: &'static [EmbeddedFlagSchema],
}

impl EmbeddedCommandSchema {
    fn to_shared_schema(self) -> CommandSchema {
        CommandSchema {
            name: self.name.into(),
            kind: self.kind,
            source_kind: self.source_kind,
            mode_mask: self.mode_mask,
            return_behavior: self.return_behavior,
            positionals: self.positionals,
            flags: self.build_effective_flags().into(),
        }
    }

    fn build_effective_flags(self) -> Vec<FlagSchema> {
        let mut flags: Vec<FlagSchema> = self
            .flags
            .iter()
            .copied()
            .map(EmbeddedFlagSchema::to_shared_schema)
            .collect();
        push_synthetic_mode_flag(&mut flags, self.mode_mask.create, "create", "c");
        push_synthetic_mode_flag(&mut flags, self.mode_mask.edit, "edit", "e");
        push_synthetic_mode_flag(&mut flags, self.mode_mask.query, "query", "q");
        flags
    }
}

static EMBEDDED_COMMAND_SCHEMAS: &[EmbeddedCommandSchema] =
    include!(concat!(env!("OUT_DIR"), "/embedded_command_schemas.rs"));

fn shared_command_schemas() -> &'static [CommandSchema] {
    static COMMAND_SCHEMAS: OnceLock<Vec<CommandSchema>> = OnceLock::new();
    COMMAND_SCHEMAS.get_or_init(|| {
        EMBEDDED_COMMAND_SCHEMAS
            .iter()
            .copied()
            .map(EmbeddedCommandSchema::to_shared_schema)
            .collect()
    })
}

fn push_synthetic_mode_flag(
    flags: &mut Vec<FlagSchema>,
    enabled: bool,
    long_name: &str,
    short_name: &str,
) {
    if !enabled
        || flags.iter().any(|flag| {
            flag.long_name.as_ref() == long_name || flag.short_name.as_deref() == Some(short_name)
        })
    {
        return;
    }

    flags.push(FlagSchema {
        long_name: long_name.into(),
        short_name: Some(short_name.into()),
        mode_mask: CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        arity_by_mode: FlagArityByMode {
            create: FlagArity::None,
            edit: FlagArity::None,
            query: FlagArity::None,
        },
        value_shapes: Vec::new().into(),
        allows_multiple: false,
    });
}

struct OverlayRegistry<'a, R: ?Sized> {
    primary: &'a R,
    fallback: MayaCommandRegistry,
}

impl<'a, R> OverlayRegistry<'a, R>
where
    R: CommandRegistry + ?Sized,
{
    const fn new(primary: &'a R) -> Self {
        Self {
            primary,
            fallback: MayaCommandRegistry::new(),
        }
    }
}

impl<R> CommandRegistry for OverlayRegistry<'_, R>
where
    R: CommandRegistry + ?Sized,
{
    fn lookup(&self, name: &str) -> Option<&CommandSchema> {
        self.primary
            .lookup(name)
            .or_else(|| self.fallback.lookup(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::SHIFT_JIS;
    use std::path::Path;

    use mel_parser::{
        LightParseOptions, parse_bytes, parse_light_bytes_with_encoding, parse_light_file,
        parse_light_source, parse_light_source_with_options, parse_source,
    };

    struct TestRegistry {
        commands: Vec<CommandSchema>,
    }

    impl CommandRegistry for TestRegistry {
        fn lookup(&self, name: &str) -> Option<&CommandSchema> {
            self.commands.iter().find(|info| info.name.as_ref() == name)
        }
    }

    #[test]
    fn embedded_registry_keeps_script_source_kind() {
        let registry = MayaCommandRegistry::new();
        let schema = registry
            .lookup("addNewShelfTab")
            .expect("embedded schema for addNewShelfTab");
        assert_eq!(schema.kind, CommandKind::Builtin);
        assert_eq!(schema.source_kind, CommandSourceKind::Script);
    }

    #[test]
    fn embedded_registry_synthesizes_mode_flags() {
        let registry = MayaCommandRegistry::new();
        let schema = registry
            .lookup("addAttr")
            .expect("embedded schema for addAttr");
        assert!(
            schema
                .flags
                .iter()
                .any(|flag| flag.long_name.as_ref() == "create")
        );
        assert!(
            schema
                .flags
                .iter()
                .any(|flag| flag.long_name.as_ref() == "edit")
        );
        assert!(
            schema
                .flags
                .iter()
                .any(|flag| flag.long_name.as_ref() == "query")
        );
    }

    #[test]
    fn embedded_registry_keeps_selection_aware_positional_policy() {
        let registry = MayaCommandRegistry::new();
        for command_name in ["ikHandle", "delete", "sets", "polyListComponentConversion"] {
            let schema = registry
                .lookup(command_name)
                .unwrap_or_else(|| panic!("embedded schema for {command_name}"));
            assert_eq!(
                schema.positionals.prefix[0].source_policy,
                PositionalSourcePolicy::ExplicitOrCurrentSelection
            );
        }
    }

    #[test]
    fn embedded_registry_keeps_relaxed_backlog_positional_shapes() {
        let registry = MayaCommandRegistry::new();

        let filter_expand = registry
            .lookup("filterExpand")
            .expect("embedded schema for filterExpand");
        assert!(matches!(
            filter_expand.positionals.tail,
            PositionalTailSchema::Opaque { min: 0, max: None }
        ));

        let shading_node = registry
            .lookup("shadingNode")
            .expect("embedded schema for shadingNode");
        assert_eq!(shading_node.positionals.prefix.len(), 1);
        assert!(matches!(
            shading_node.positionals.tail,
            PositionalTailSchema::Shaped {
                min: 0,
                max: Some(1),
                value_shapes,
            } if value_shapes == [ValueShape::String]
        ));

        let attribute_exists = registry
            .lookup("attributeExists")
            .expect("embedded schema for attributeExists");
        assert_eq!(attribute_exists.positionals.prefix.len(), 2);

        let namespace_info = registry
            .lookup("namespaceInfo")
            .expect("embedded schema for namespaceInfo");
        assert!(namespace_info.positionals.prefix.is_empty());
        assert!(matches!(
            namespace_info.positionals.tail,
            PositionalTailSchema::Opaque { min: 0, max: None }
        ));

        let particle = registry
            .lookup("particle")
            .expect("embedded schema for particle");
        assert_eq!(
            particle.positionals.prefix[0].source_policy,
            PositionalSourcePolicy::ExplicitOrCurrentSelection
        );
        assert!(matches!(
            particle.positionals.tail,
            PositionalTailSchema::Opaque { min: 0, max: None }
        ));
    }

    #[test]
    fn collects_top_level_command_proc_and_other_items() {
        let parse = parse_source("global proc foo() { }\nsetAttr \".tx\" 1;\nint $x = 1;\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        assert!(matches!(facts.items[0], MayaTopLevelItem::Proc { .. }));
        assert!(matches!(facts.items[1], MayaTopLevelItem::Command(_)));
        assert!(matches!(facts.items[2], MayaTopLevelItem::Other { .. }));
    }

    #[test]
    fn raw_items_preserve_exponent_numeric_literals() {
        let parse = parse_source("setAttr \".v\" .5e+2;\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.raw_items[1].kind, MayaRawShellItemKind::Numeric);
        assert_eq!(command.raw_items[1].value_text.as_deref(), Some(".5e+2"));
        assert_eq!(command.raw_items[1].source_text, ".5e+2");
    }

    #[test]
    fn set_attr_data_reference_edits_is_specialized_losslessly() {
        let parse =
            parse_source("setAttr \".ed\" -type \"dataReferenceEdits\" \"rootRN\" \"\" 5;\n");
        assert!(parse.errors.is_empty());
        let mut flags = vec![FlagSchema {
            long_name: "type".into(),
            short_name: Some("typ".into()),
            mode_mask: CommandModeMask {
                create: true,
                edit: true,
                query: true,
            },
            arity_by_mode: FlagArityByMode {
                create: FlagArity::Exact(1),
                edit: FlagArity::Exact(1),
                query: FlagArity::Exact(1),
            },
            value_shapes: vec![ValueShape::String].into(),
            allows_multiple: false,
        }];
        push_synthetic_mode_flag(&mut flags, true, "create", "c");
        push_synthetic_mode_flag(&mut flags, true, "edit", "e");
        push_synthetic_mode_flag(&mut flags, true, "query", "q");
        let command = CommandSchema {
            name: "setAttr".into(),
            kind: CommandKind::Builtin,
            source_kind: CommandSourceKind::Command,
            mode_mask: CommandModeMask {
                create: true,
                edit: true,
                query: true,
            },
            return_behavior: ReturnBehavior::Unknown,
            positionals: PositionalSchema {
                prefix: &[mel_sema::PositionalSlotSchema {
                    value_shapes: &[ValueShape::String],
                    source_policy: PositionalSourcePolicy::ExplicitOnly,
                }],
                tail: PositionalTailSchema::Opaque { min: 0, max: None },
            },
            flags: flags.into(),
        };
        let registry = TestRegistry {
            commands: vec![command],
        };

        let facts = collect_top_level_facts_with_registry(&parse, &registry);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        let Some(MayaSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref() else {
            panic!("expected setAttr specialization");
        };
        assert_eq!(
            set_attr.value_kind,
            MayaSetAttrValueKind::DataReferenceEdits
        );
        assert_eq!(
            set_attr
                .type_name
                .as_ref()
                .and_then(|item| item.value_text.as_deref()),
            Some("dataReferenceEdits")
        );
        assert_eq!(set_attr.values.len(), 3);
        assert_eq!(set_attr.values[2].value_text.as_deref(), Some("5"));
    }

    #[test]
    fn create_node_specialization_extracts_parent_and_name() {
        let parse = parse_source("createNode transform -n \"pCube1\" -p \"|group1\";\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        let Some(MayaSpecializedCommand::CreateNode(create_node)) = command.specialized.as_ref()
        else {
            panic!("expected createNode specialization");
        };
        assert_eq!(
            create_node
                .name
                .as_ref()
                .and_then(|item| item.value_text.as_deref()),
            Some("pCube1")
        );
        assert_eq!(
            create_node
                .parent
                .as_ref()
                .and_then(|item| item.value_text.as_deref()),
            Some("|group1")
        );
    }

    #[test]
    fn grouped_expr_raw_item_preserves_full_source_text() {
        let parse = parse_source("setAttr \".b\" -type \"string\" (\"a\" + \"b\");\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.raw_items[3].kind, MayaRawShellItemKind::GroupedExpr);
        assert_eq!(command.raw_items[3].source_text, "(\"a\" + \"b\")");
    }

    #[test]
    fn variable_raw_item_preserves_full_source_text() {
        let parse = parse_source("python $cmd;\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.raw_items[0].kind, MayaRawShellItemKind::Variable);
        assert_eq!(command.raw_items[0].source_text, "$cmd");
    }

    #[test]
    fn brace_list_raw_item_preserves_full_source_text() {
        let parse = parse_source("foo {\"a\", \"b\"};\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.raw_items[0].kind, MayaRawShellItemKind::BraceList);
        assert_eq!(command.raw_items[0].source_text, "{\"a\", \"b\"}");
    }

    #[test]
    fn vector_literal_raw_item_preserves_full_source_text() {
        let parse = parse_source("move <<1, 2, 3>>;\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(
            command.raw_items[0].kind,
            MayaRawShellItemKind::VectorLiteral
        );
        assert_eq!(command.raw_items[0].source_text, "<<1, 2, 3>>");
    }

    #[test]
    fn capture_raw_item_preserves_full_source_text() {
        let parse = parse_source("python `someCmd -q`;\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.raw_items[0].kind, MayaRawShellItemKind::Capture);
        assert_eq!(command.raw_items[0].source_text, "`someCmd -q`");
    }

    #[test]
    fn mixed_shell_word_kinds_remain_lossless() {
        let parse = parse_source("python $cmd (\"a\" + \"b\") {\"x\"} <<1, 2, 3>> `someCmd -q`;\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        let actual = command
            .raw_items
            .iter()
            .map(|item| item.source_text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            vec![
                "$cmd",
                "(\"a\" + \"b\")",
                "{\"x\"}",
                "<<1, 2, 3>>",
                "`someCmd -q`"
            ]
        );
    }

    #[test]
    fn grouped_expr_raw_item_uses_display_range_for_lossless_slice() {
        let bytes = b"setAttr \".b\" -type \"string\" (\"\xFF\");\n";
        let parse = parse_bytes(bytes);
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.raw_items[3].kind, MayaRawShellItemKind::GroupedExpr);
        assert_eq!(command.raw_items[3].source_text, "(\"\u{FFFD}\")");
    }

    #[test]
    fn light_collector_keeps_heavy_set_attr_tail_opaque() {
        let parse = parse_light_source(
            "createNode mesh -n \"meshShape\";\nsetAttr \".fc[0]\" -type \"polyFaces\" f 4 0 1 2 3 mu 0 4 0 1 2 3;\n",
        );
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts_light(&parse);
        assert!(matches!(facts.items[0], MayaLightTopLevelItem::Command(_)));
        let MayaLightTopLevelItem::Command(command) = &facts.items[1] else {
            panic!("expected command");
        };
        let Some(MayaLightSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref()
        else {
            panic!("expected light setAttr specialization");
        };
        assert_eq!(
            set_attr
                .attr_path
                .as_ref()
                .and_then(|item| item.value_text.as_deref()),
            Some(".fc[0]")
        );
        assert_eq!(
            set_attr
                .type_name
                .as_ref()
                .and_then(|item| item.value_text.as_deref()),
            Some("polyFaces")
        );
        assert!(command.opaque_tail.is_none());
        assert_eq!(set_attr.prefix_values[0].value_text.as_deref(), Some("f"));
    }

    #[test]
    fn light_collector_uses_opaque_tail_when_prefix_limit_hits() {
        let parse = mel_parser::parse_light_source_with_options(
            "setAttr \".pt\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n",
            mel_parser::LightParseOptions {
                max_prefix_words: 5,
                max_prefix_bytes: 32,
            },
        );
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts_light(&parse);
        let MayaLightTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        let Some(MayaLightSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref()
        else {
            panic!("expected light setAttr specialization");
        };
        assert!(command.opaque_tail.is_some());
        assert!(set_attr.opaque_tail.is_some());
        assert_eq!(set_attr.prefix_values.len(), 2);
    }

    #[test]
    fn light_collector_can_smoke_tmp_mesh_sample_when_present() {
        let path = Path::new("/mnt/e/Projects/RnD/MayaMelParser/tmp/Test_Mesh_Horizon.ma");
        if !path.exists() {
            return;
        }
        let parse = parse_light_file(path).expect("light parse sample");
        assert!(parse.decode_errors.is_empty());
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts_light(&parse);
        assert!(!facts.items.is_empty());
    }

    #[test]
    fn hybrid_collector_matches_full_shape_for_non_opaque_command() {
        let full = collect_top_level_facts(&parse_source(
            "createNode transform -n \"pCube1\" -p \"|group1\";\n",
        ));
        let light = parse_light_source("createNode transform -n \"pCube1\" -p \"|group1\";\n");
        let hybrid = collect_top_level_facts_hybrid(&light).expect("hybrid facts");

        let MayaTopLevelItem::Command(full_command) = &full.items[0] else {
            panic!("expected full command");
        };
        let MayaTopLevelItem::Command(hybrid_command) = &hybrid.items[0] else {
            panic!("expected hybrid command");
        };
        assert_eq!(hybrid_command.head, full_command.head);
        assert_eq!(hybrid_command.raw_items, full_command.raw_items);
        assert_eq!(hybrid_command.normalized, full_command.normalized);
        assert_eq!(hybrid_command.specialized, full_command.specialized);
        assert_eq!(full_command.promotion_kind, MayaPromotionKind::FullParse);
        assert_eq!(
            hybrid_command.promotion_kind,
            MayaPromotionKind::LightSynthesized
        );
    }

    #[test]
    fn hybrid_collector_promotes_opaque_set_attr_tail_to_full_raw_items() {
        let parse = parse_light_source_with_options(
            "setAttr \".pt\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n",
            LightParseOptions {
                max_prefix_words: 5,
                max_prefix_bytes: 32,
            },
        );
        let hybrid = collect_top_level_facts_hybrid(&parse).expect("hybrid facts");
        let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
            panic!("expected command");
        };
        assert!(command.raw_items.len() > 5);
        assert_eq!(
            command.promotion_kind,
            MayaPromotionKind::OpaqueTailPromoted
        );
        let Some(MayaSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref() else {
            panic!("expected setAttr specialization");
        };
        assert_eq!(set_attr.values.len(), 10);
    }

    #[test]
    fn promoted_command_keeps_cp932_source_slices() {
        let parse = parse_light_bytes_with_encoding(
            SHIFT_JIS
                .encode("setAttr \".名\" -type \"string\" \"値\";\n")
                .0
                .as_ref(),
            mel_parser::SourceEncoding::Cp932,
        );
        let hybrid = collect_top_level_facts_hybrid_with_registry(
            &parse,
            &EmptyCommandRegistry,
            MayaPromotionPolicy::Always,
        )
        .expect("hybrid facts");
        let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.promotion_kind, MayaPromotionKind::PolicyPromoted);
        assert_eq!(command.raw_items[0].source_text, "\".名\"");
        assert_eq!(command.raw_items[3].value_text.as_deref(), Some("値"));
    }

    #[test]
    fn hybrid_always_policy_promotes_file_command_with_grouped_expr() {
        let parse = parse_light_source("file -command (\"print \\\"hi\\\";\");\n");
        let hybrid = collect_top_level_facts_hybrid_with_registry(
            &parse,
            &EmptyCommandRegistry,
            MayaPromotionPolicy::Always,
        )
        .expect("hybrid facts");
        let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.promotion_kind, MayaPromotionKind::PolicyPromoted);
        assert_eq!(command.raw_items[1].kind, MayaRawShellItemKind::GroupedExpr);
        let Some(MayaSpecializedCommand::File(file)) = command.specialized.as_ref() else {
            panic!("expected file specialization");
        };
        assert_eq!(file.flags.len(), 1);
    }

    #[test]
    fn hybrid_custom_decider_promotes_grouped_expr_command() {
        let parse = parse_light_source("file -command (\"print \\\"hi\\\";\");\n");
        let decider: &dyn MayaPromotionDecider = &|candidate: MayaPromotionCandidate<'_>| {
            candidate
                .command
                .words
                .iter()
                .any(|word| matches!(word, LightWord::GroupedExpr { .. }))
        };
        let hybrid = collect_top_level_facts_hybrid_with_decider(
            &parse,
            &MayaPromotionOptions::default(),
            decider,
        )
        .expect("hybrid facts");

        let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
            panic!("expected command");
        };
        assert_eq!(
            command.promotion_kind,
            MayaPromotionKind::CustomDeciderPromoted
        );
        assert_eq!(command.raw_items[1].kind, MayaRawShellItemKind::GroupedExpr);
    }

    #[test]
    fn opaque_tail_promotion_takes_precedence_over_custom_decider() {
        let parse = parse_light_source_with_options(
            "setAttr \".pt\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n",
            LightParseOptions {
                max_prefix_words: 5,
                max_prefix_bytes: 32,
            },
        );
        let hybrid = collect_top_level_facts_hybrid_with_decider(
            &parse,
            &MayaPromotionOptions::default(),
            &|_: MayaPromotionCandidate<'_>| true,
        )
        .expect("hybrid facts");

        let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
            panic!("expected command");
        };
        assert_eq!(
            command.promotion_kind,
            MayaPromotionKind::OpaqueTailPromoted
        );
    }

    #[test]
    fn hybrid_promotes_data_reference_edits_tail() {
        let parse = parse_light_source_with_options(
            "setAttr \".ed\" -type \"dataReferenceEdits\" \"rootRN\" \"\" 5;\n",
            LightParseOptions {
                max_prefix_words: 3,
                max_prefix_bytes: 24,
            },
        );
        let mut flags = vec![FlagSchema {
            long_name: "type".into(),
            short_name: Some("typ".into()),
            mode_mask: CommandModeMask {
                create: true,
                edit: true,
                query: true,
            },
            arity_by_mode: FlagArityByMode {
                create: FlagArity::Exact(1),
                edit: FlagArity::Exact(1),
                query: FlagArity::Exact(1),
            },
            value_shapes: vec![ValueShape::String].into(),
            allows_multiple: false,
        }];
        push_synthetic_mode_flag(&mut flags, true, "create", "c");
        push_synthetic_mode_flag(&mut flags, true, "edit", "e");
        push_synthetic_mode_flag(&mut flags, true, "query", "q");
        let command = CommandSchema {
            name: "setAttr".into(),
            kind: CommandKind::Builtin,
            source_kind: CommandSourceKind::Command,
            mode_mask: CommandModeMask {
                create: true,
                edit: true,
                query: true,
            },
            return_behavior: ReturnBehavior::Unknown,
            positionals: PositionalSchema {
                prefix: &[mel_sema::PositionalSlotSchema {
                    value_shapes: &[ValueShape::String],
                    source_policy: PositionalSourcePolicy::ExplicitOnly,
                }],
                tail: PositionalTailSchema::Opaque { min: 0, max: None },
            },
            flags: flags.into(),
        };
        let registry = TestRegistry {
            commands: vec![command],
        };

        let hybrid = collect_top_level_facts_hybrid_with_registry(
            &parse,
            &registry,
            MayaPromotionPolicy::default(),
        )
        .expect("hybrid facts");
        let MayaTopLevelItem::Command(command) = &hybrid.items[0] else {
            panic!("expected command");
        };
        assert_eq!(
            command.promotion_kind,
            MayaPromotionKind::OpaqueTailPromoted
        );
        let Some(MayaSpecializedCommand::SetAttr(set_attr)) = command.specialized.as_ref() else {
            panic!("expected setAttr specialization");
        };
        assert_eq!(
            set_attr.value_kind,
            MayaSetAttrValueKind::DataReferenceEdits
        );
        assert_eq!(set_attr.values.len(), 3);
    }

    #[test]
    fn hybrid_report_keeps_light_command_when_policy_promotion_fails() {
        let parse = parse_light_source("createNode transform -n \"pCube1\"");
        let report = collect_top_level_facts_hybrid_report(
            &parse,
            &MayaPromotionOptions {
                policy: MayaPromotionPolicy::Always,
                ..MayaPromotionOptions::default()
            },
        );

        assert_eq!(report.promotion_diagnostics.len(), 1);
        assert_eq!(
            report.promotion_diagnostics[0].attempted_kind,
            MayaPromotionKind::PolicyPromoted
        );
        let MayaTopLevelItem::Command(command) = &report.facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.promotion_kind, MayaPromotionKind::LightSynthesized);
    }

    #[test]
    fn hybrid_report_keeps_light_command_when_custom_decider_promotion_fails() {
        let parse = parse_light_source("file -command (\"print \\\"hi\\\";\")");
        let report = collect_top_level_facts_hybrid_report_with_decider(
            &parse,
            &MayaPromotionOptions::default(),
            &|candidate: MayaPromotionCandidate<'_>| {
                candidate
                    .command
                    .words
                    .iter()
                    .any(|word| matches!(word, LightWord::GroupedExpr { .. }))
            },
        );

        assert_eq!(report.promotion_diagnostics.len(), 1);
        assert_eq!(
            report.promotion_diagnostics[0].attempted_kind,
            MayaPromotionKind::CustomDeciderPromoted
        );
        let MayaTopLevelItem::Command(command) = &report.facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.promotion_kind, MayaPromotionKind::LightSynthesized);
    }

    #[test]
    fn hybrid_report_collects_set_attr_validation_diagnostic() {
        let parse = parse_light_source("setAttr \".tx\" -type \"string\";\n");
        let report =
            collect_top_level_facts_hybrid_report(&parse, &MayaPromotionOptions::default());

        assert_eq!(report.validation_diagnostics.len(), 1);
        assert_eq!(
            report.validation_diagnostics[0].head.as_deref(),
            Some("setAttr")
        );
        assert_eq!(
            report.validation_diagnostics[0].message,
            "setAttr requires at least one value after the attribute path when -type is present"
        );
    }

    #[test]
    fn hybrid_report_leaves_validation_diagnostics_empty_for_valid_set_attr() {
        let parse = parse_light_source("setAttr \".tx\" -type \"string\" \"value\";\n");
        let report =
            collect_top_level_facts_hybrid_report(&parse, &MayaPromotionOptions::default());

        assert!(report.validation_diagnostics.is_empty());
    }

    #[test]
    fn hybrid_strict_options_forward_parse_mode_to_promotion() {
        let parse = parse_light_source("createNode transform -n \"pCube1\"");
        let facts = collect_top_level_facts_hybrid_with_registry_and_options(
            &parse,
            &EmptyCommandRegistry,
            &MayaPromotionOptions {
                policy: MayaPromotionPolicy::Always,
                parse_options: ParseOptions {
                    mode: mel_parser::ParseMode::AllowTrailingStmtWithoutSemi,
                },
            },
        )
        .expect("hybrid facts");

        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.promotion_kind, MayaPromotionKind::PolicyPromoted);
        assert_eq!(command.raw_items[0].source_text, "transform");
    }

    #[test]
    fn selective_collector_extracts_target_commands_only() {
        let source =
            "rename \"a\" \"b\";\ncreateNode mesh -n \"meshShape\";\nsetAttr \".b\" yes;\n";
        let mut items = Vec::new();
        let report =
            collect_selective_top_level_source_with_sink(source, &mut |item: MayaSelectiveItem| {
                items.push(item)
            });

        assert!(report.errors.is_empty());
        assert_eq!(items.len(), 2);
        let MayaSelectiveItem::CreateNode(create_node) = &items[0] else {
            panic!("expected createNode item");
        };
        assert_eq!(
            create_node
                .node_type_range
                .map(|range| report.source_slice(range)),
            Some("mesh")
        );
        let MayaSelectiveItem::SetAttr(set_attr) = &items[1] else {
            panic!("expected setAttr item");
        };
        assert_eq!(set_attr.tracked_attr, Some(MayaTrackedSetAttrAttr::B));
    }

    #[test]
    fn selective_collector_can_include_other_commands_as_passthrough() {
        let source = "rename \"a\" \"b\";\ncreateNode transform;\n";
        let mut items = Vec::new();
        let report = collect_selective_top_level_source_with_options_and_sink(
            source,
            LightParseOptions::default(),
            &MayaSelectiveOptions {
                passthrough: MayaSelectivePassthrough::IncludeOtherCommands,
            },
            &DefaultMayaSelectiveSetAttrSelector,
            &mut |item: MayaSelectiveItem| items.push(item),
        );

        assert!(report.errors.is_empty());
        assert_eq!(items.len(), 2);
        let MayaSelectiveItem::OtherCommand { head_range, .. } = &items[0] else {
            panic!("expected passthrough command");
        };
        assert_eq!(report.source_slice(*head_range), "rename");
    }

    #[test]
    fn selective_collector_keeps_opaque_set_attr_without_promotion() {
        let source = "setAttr \".f\" -type \"doubleArray\" 1 2 3 4 5 6 7 8 9 10;\n";
        let mut items = Vec::new();
        let report = collect_selective_top_level_source_with_options_and_sink(
            source,
            LightParseOptions {
                max_prefix_words: 4,
                max_prefix_bytes: 24,
            },
            &MayaSelectiveOptions::default(),
            &DefaultMayaSelectiveSetAttrSelector,
            &mut |item: MayaSelectiveItem| items.push(item),
        );

        assert!(report.errors.is_empty());
        let MayaSelectiveItem::SetAttr(set_attr) = &items[0] else {
            panic!("expected setAttr item");
        };
        assert_eq!(set_attr.tracked_attr, Some(MayaTrackedSetAttrAttr::F));
        assert!(set_attr.opaque_tail.is_some());
    }

    #[test]
    fn selective_collector_uses_custom_set_attr_selector() {
        let source = "setAttr \".custom\" 1;\n";
        let mut items = Vec::new();
        let report = collect_selective_top_level_source_with_options_and_sink(
            source,
            LightParseOptions::default(),
            &MayaSelectiveOptions::default(),
            &|attr_path: &str| (attr_path == ".custom").then_some(MayaTrackedSetAttrAttr::Fn),
            &mut |item: MayaSelectiveItem| items.push(item),
        );

        assert!(report.errors.is_empty());
        let MayaSelectiveItem::SetAttr(set_attr) = &items[0] else {
            panic!("expected setAttr item");
        };
        assert_eq!(set_attr.tracked_attr, Some(MayaTrackedSetAttrAttr::Fn));
    }

    #[test]
    fn selective_collector_preserves_cp932_source_slices() {
        let bytes = SHIFT_JIS
            .encode("setAttr \".名\" -type \"string\" \"値\";\n")
            .0;
        let mut items = Vec::new();
        let report = collect_selective_top_level_bytes_with_encoding_and_sink(
            bytes.as_ref(),
            SourceEncoding::Cp932,
            &MayaSelectiveOptions::default(),
            &DefaultMayaSelectiveSetAttrSelector,
            &mut |item: MayaSelectiveItem| items.push(item),
        );

        let MayaSelectiveItem::SetAttr(set_attr) = &items[0] else {
            panic!("expected setAttr item");
        };
        assert_eq!(
            set_attr
                .attr_path_range
                .map(|range| report.source_slice(range)),
            Some("\".名\"")
        );
    }
}
