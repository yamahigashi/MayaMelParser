use mel_parser::{LightCommandSurface, ParseOptions};
use mel_sema::{CommandKind, CommandMode};
use mel_syntax::{SourceView, TextRange};

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
    /// Surface classification for this shell item.
    pub kind: MayaRawShellItemKind,
    /// Lossless source span for the full raw item surface as it appeared in the command.
    pub span: TextRange,
    pub(crate) text_range: Option<TextRange>,
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
    pub source_range: TextRange,
    pub canonical_name: Option<String>,
    pub args: Vec<MayaPositionalArg>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightFlag {
    pub source_range: TextRange,
    pub canonical_name: Option<String>,
    pub args: Vec<MayaPositionalArg>,
    pub span: TextRange,
}

impl MayaRawShellItem {
    /// Returns the lexical text span used to derive [`Self::value_text`], when this item kind has one.
    ///
    /// This range is distinct from [`Self::span`]: `span` covers the full raw item surface,
    /// while `text_range` points at the slice that backs value extraction for literal-like words.
    #[must_use]
    pub fn text_range(&self) -> Option<TextRange> {
        self.text_range
    }

    /// Returns the lossless source text for the full raw item surface.
    #[must_use]
    pub fn source_text<'a>(&self, source: SourceView<'a>) -> &'a str {
        source.display_slice(self.span)
    }

    /// Returns the lexical value text for literal-like words.
    ///
    /// This accessor uses [`Self::text_range`] and may further normalize the sliced text by kind,
    /// such as stripping the surrounding quotes from quoted strings.
    #[must_use]
    pub fn value_text<'a>(&self, source: SourceView<'a>) -> Option<&'a str> {
        let text = source.slice(self.text_range?);
        match self.kind {
            MayaRawShellItemKind::Numeric | MayaRawShellItemKind::BareWord => Some(text),
            MayaRawShellItemKind::QuotedString => text
                .strip_prefix('"')
                .and_then(|text| text.strip_suffix('"')),
            MayaRawShellItemKind::Flag
            | MayaRawShellItemKind::Variable
            | MayaRawShellItemKind::GroupedExpr
            | MayaRawShellItemKind::BraceList
            | MayaRawShellItemKind::VectorLiteral
            | MayaRawShellItemKind::Capture => None,
        }
    }

    /// Returns the preferred consumer-facing text for this shell item.
    ///
    /// Literal-like words use [`Self::value_text`] so downstream consumers can read decoded
    /// values without stripping quotes manually. Other shell surfaces fall back to full
    /// source-preserving text.
    #[must_use]
    pub fn preferred_text<'a>(&self, source: SourceView<'a>) -> &'a str {
        self.value_text(source)
            .unwrap_or_else(|| self.source_text(source))
    }
}

impl MayaPositionalArg {
    #[must_use]
    pub fn preferred_text<'a>(&self, source: SourceView<'a>) -> &'a str {
        self.item.preferred_text(source)
    }
}

impl MayaNormalizedFlag {
    #[must_use]
    pub fn source_text<'a>(&self, source: SourceView<'a>) -> &'a str {
        source.display_slice(self.source_range)
    }

    #[must_use]
    pub fn preferred_name<'a>(&'a self, source: SourceView<'a>) -> &'a str {
        self.canonical_name
            .as_deref()
            .unwrap_or_else(|| self.source_text(source))
    }

    #[must_use]
    pub fn matches_name(&self, source: SourceView<'_>, canonical: &str, short: &str) -> bool {
        self.canonical_name.as_deref() == Some(canonical)
            || self.source_text(source) == short
            || self.source_text(source).trim_start_matches('-') == short.trim_start_matches('-')
    }
}

impl MayaLightFlag {
    #[must_use]
    pub fn source_text<'a>(&self, source: SourceView<'a>) -> &'a str {
        source.display_slice(self.source_range)
    }

    #[must_use]
    pub fn preferred_name<'a>(&'a self, source: SourceView<'a>) -> &'a str {
        self.canonical_name
            .as_deref()
            .unwrap_or_else(|| self.source_text(source))
    }

    #[must_use]
    pub fn matches_name(&self, source: SourceView<'_>, canonical: &str, short: &str) -> bool {
        self.canonical_name.as_deref() == Some(canonical)
            || self.source_text(source) == short
            || self.source_text(source).trim_start_matches('-') == short.trim_start_matches('-')
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MayaNormalizedCommandItem {
    Flag(MayaNormalizedFlag),
    Positional(MayaPositionalArg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaNormalizedCommand {
    pub head: String,
    pub(crate) head_range: TextRange,
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
    AddAttr(Box<MayaAddAttrCommand>),
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
    AddAttr(Box<MayaLightAddAttrCommand>),
    ConnectAttr(MayaLightConnectAttrCommand),
    Relationship(MayaLightRelationshipCommand),
    File(MayaLightFileCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaRequiresCommand {
    pub plugin_name: Option<MayaRawShellItem>,
    pub plugin_version: Option<MayaRawShellItem>,
    pub option_items: Vec<MayaRawShellItem>,
    pub requirements: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightRequiresCommand {
    pub plugin_name: Option<MayaRawShellItem>,
    pub plugin_version: Option<MayaRawShellItem>,
    pub option_items: Vec<MayaRawShellItem>,
    pub requirements: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaCurrentUnitCommand {
    pub linear: Option<MayaRawShellItem>,
    pub angle: Option<MayaRawShellItem>,
    pub time: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightCurrentUnitCommand {
    pub linear: Option<MayaRawShellItem>,
    pub angle: Option<MayaRawShellItem>,
    pub time: Option<MayaRawShellItem>,
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
    pub shared: bool,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightCreateNodeCommand {
    pub node_type: Option<MayaRawShellItem>,
    pub name: Option<MayaRawShellItem>,
    pub parent: Option<MayaRawShellItem>,
    pub shared: bool,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaRenameCommand {
    pub uuid: Option<MayaRawShellItem>,
    pub source: Option<MayaRawShellItem>,
    pub target: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightRenameCommand {
    pub uuid: Option<MayaRawShellItem>,
    pub source: Option<MayaRawShellItem>,
    pub target: Option<MayaRawShellItem>,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaSelectCommand {
    pub no_expand: bool,
    pub targets: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightSelectCommand {
    pub no_expand: bool,
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
    pub short_name: Option<MayaRawShellItem>,
    pub long_name: Option<MayaRawShellItem>,
    pub parent: Option<MayaRawShellItem>,
    pub number_of_children: Option<MayaRawShellItem>,
    pub nice_name: Option<MayaRawShellItem>,
    pub default_value: Option<MayaRawShellItem>,
    pub min_value: Option<MayaRawShellItem>,
    pub max_value: Option<MayaRawShellItem>,
    pub soft_min_value: Option<MayaRawShellItem>,
    pub soft_max_value: Option<MayaRawShellItem>,
    pub enum_name: Option<MayaRawShellItem>,
    pub attribute_type: Option<MayaRawShellItem>,
    pub data_type: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub tail: Vec<MayaRawShellItem>,
    pub tail_kind: MayaAddAttrTailKind,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightAddAttrCommand {
    pub short_name: Option<MayaRawShellItem>,
    pub long_name: Option<MayaRawShellItem>,
    pub parent: Option<MayaRawShellItem>,
    pub number_of_children: Option<MayaRawShellItem>,
    pub nice_name: Option<MayaRawShellItem>,
    pub default_value: Option<MayaRawShellItem>,
    pub min_value: Option<MayaRawShellItem>,
    pub max_value: Option<MayaRawShellItem>,
    pub soft_min_value: Option<MayaRawShellItem>,
    pub soft_max_value: Option<MayaRawShellItem>,
    pub enum_name: Option<MayaRawShellItem>,
    pub attribute_type: Option<MayaRawShellItem>,
    pub data_type: Option<MayaRawShellItem>,
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
    pub next_available: bool,
    pub lock_arg: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightConnectAttrCommand {
    pub source_attr: Option<MayaRawShellItem>,
    pub target_attr: Option<MayaRawShellItem>,
    pub next_available: bool,
    pub lock_arg: Option<MayaRawShellItem>,
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
    pub namespace: Option<MayaRawShellItem>,
    pub reference_node: Option<MayaRawShellItem>,
    pub file_type: Option<MayaRawShellItem>,
    pub options: Option<MayaRawShellItem>,
    pub is_reference: bool,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaLightFileCommand {
    pub path: Option<MayaRawShellItem>,
    pub namespace: Option<MayaRawShellItem>,
    pub reference_node: Option<MayaRawShellItem>,
    pub file_type: Option<MayaRawShellItem>,
    pub options: Option<MayaRawShellItem>,
    pub is_reference: bool,
    pub flags: Vec<MayaLightFlag>,
    pub opaque_tail: Option<TextRange>,
    pub span: TextRange,
}
