#![forbid(unsafe_code)]

use mel_ast::{Expr, InvokeSurface, Item, ProcDef, ShellWord, SourceFile, Stmt};
use mel_sema::{
    CommandKind, CommandMode, CommandModeMask, CommandRegistry, CommandSchema, CommandSourceKind,
    EmptyCommandRegistry, FlagArity, FlagArityByMode, FlagSchema, NormalizedCommandInvoke,
    NormalizedCommandItem, NormalizedFlag, PositionalArg, ReturnBehavior, ValueShape,
};
use mel_syntax::TextRange;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MayaCommandRegistry;

impl MayaCommandRegistry {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl CommandRegistry for MayaCommandRegistry {
    fn lookup(&self, name: &str) -> Option<CommandSchema> {
        EMBEDDED_COMMAND_SCHEMAS
            .binary_search_by(|schema| schema.name.cmp(name))
            .ok()
            .map(|index| EMBEDDED_COMMAND_SCHEMAS[index].to_owned_schema())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MayaTopLevelFacts {
    pub items: Vec<MayaTopLevelItem>,
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
pub struct MayaTopLevelCommand {
    pub head: String,
    pub captured: bool,
    pub raw_items: Vec<MayaRawShellItem>,
    pub normalized: Option<MayaNormalizedCommand>,
    pub specialized: Option<MayaSpecializedCommand>,
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
pub enum MayaNormalizedCommandItem {
    Flag(MayaNormalizedFlag),
    Positional(MayaPositionalArg),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaNormalizedCommand {
    pub head: String,
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
pub struct MayaRequiresCommand {
    pub requirements: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaCurrentUnitCommand {
    pub flags: Vec<MayaNormalizedFlag>,
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
pub struct MayaCreateNodeCommand {
    pub node_type: Option<MayaRawShellItem>,
    pub name: Option<MayaRawShellItem>,
    pub parent: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
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
pub struct MayaSelectCommand {
    pub targets: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
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
pub struct MayaRelationshipCommand {
    pub relationship: Option<MayaRawShellItem>,
    pub members: Vec<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MayaFileCommand {
    pub path: Option<MayaRawShellItem>,
    pub flags: Vec<MayaNormalizedFlag>,
    pub span: TextRange,
}

#[must_use]
pub fn collect_top_level_facts(source: &SourceFile) -> MayaTopLevelFacts {
    collect_top_level_facts_with_registry(source, &EmptyCommandRegistry)
}

#[must_use]
pub fn collect_top_level_facts_with_registry<R>(
    source: &SourceFile,
    registry: &R,
) -> MayaTopLevelFacts
where
    R: CommandRegistry + ?Sized,
{
    let overlay = OverlayRegistry::new(registry);
    let analysis = mel_sema::analyze_with_registry(source, &overlay);
    let mut remaining_normalized: Vec<Option<MayaNormalizedCommand>> = analysis
        .normalized_invokes
        .into_iter()
        .map(|invoke| Some(MayaNormalizedCommand::from(invoke)))
        .collect();
    let mut items = Vec::new();

    for item in &source.items {
        match item {
            Item::Proc(proc_def) => items.push(proc_item(proc_def)),
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Proc { proc_def, .. } => items.push(proc_item(proc_def)),
                Stmt::Expr {
                    expr: Expr::Invoke(invoke),
                    ..
                } => {
                    if let InvokeSurface::ShellLike {
                        head,
                        words,
                        captured,
                    } = &invoke.surface
                    {
                        let normalized =
                            take_matching_normalized(&mut remaining_normalized, head, invoke.range);
                        let raw_items = words
                            .iter()
                            .map(raw_item_from_shell_word)
                            .collect::<Vec<_>>();
                        let specialized =
                            specialize_command(head, invoke.range, normalized.as_ref(), &raw_items);
                        items.push(MayaTopLevelItem::Command(Box::new(MayaTopLevelCommand {
                            head: head.clone(),
                            captured: *captured,
                            raw_items,
                            normalized,
                            specialized,
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

fn proc_item(proc_def: &ProcDef) -> MayaTopLevelItem {
    MayaTopLevelItem::Proc {
        name: proc_def.name.clone(),
        is_global: proc_def.is_global,
        span: proc_def.range,
    }
}

fn take_matching_normalized(
    invokes: &mut [Option<MayaNormalizedCommand>],
    head: &str,
    range: TextRange,
) -> Option<MayaNormalizedCommand> {
    let index = invokes.iter().position(|invoke| {
        invoke
            .as_ref()
            .is_some_and(|invoke| invoke.head == head && invoke.span == range)
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

fn raw_item_from_shell_word(word: &ShellWord) -> MayaRawShellItem {
    let (source_text, value_text, kind, span) = match word {
        ShellWord::Flag { text, range } => (text.clone(), None, MayaRawShellItemKind::Flag, *range),
        ShellWord::NumericLiteral { text, range } => (
            text.clone(),
            Some(text.clone()),
            MayaRawShellItemKind::Numeric,
            *range,
        ),
        ShellWord::BareWord { text, range } => (
            text.clone(),
            Some(text.clone()),
            MayaRawShellItemKind::BareWord,
            *range,
        ),
        ShellWord::QuotedString { text, range } => (
            text.clone(),
            unquote_shell_string(text).map(str::to_owned),
            MayaRawShellItemKind::QuotedString,
            *range,
        ),
        ShellWord::Variable { expr, range } => (
            shell_expr_source_text(expr),
            None,
            MayaRawShellItemKind::Variable,
            *range,
        ),
        ShellWord::GroupedExpr { expr, range } => (
            shell_expr_source_text(expr),
            None,
            MayaRawShellItemKind::GroupedExpr,
            *range,
        ),
        ShellWord::BraceList { expr, range } => (
            shell_expr_source_text(expr),
            None,
            MayaRawShellItemKind::BraceList,
            *range,
        ),
        ShellWord::VectorLiteral { expr, range } => (
            shell_expr_source_text(expr),
            None,
            MayaRawShellItemKind::VectorLiteral,
            *range,
        ),
        ShellWord::Capture { invoke, range } => (
            match &invoke.surface {
                InvokeSurface::Function { name, .. } => format!("`{name}(...)`"),
                InvokeSurface::ShellLike { head, .. } => format!("`{head} ...`"),
            },
            None,
            MayaRawShellItemKind::Capture,
            *range,
        ),
    };
    MayaRawShellItem {
        source_text,
        value_text,
        kind,
        span,
    }
}

fn shell_expr_source_text(expr: &Expr) -> String {
    match expr {
        Expr::Ident { name, .. } => name.clone(),
        Expr::BareWord { text, .. } => text.clone(),
        Expr::Int { value, .. } => value.to_string(),
        Expr::Float { text, .. } | Expr::String { text, .. } => text.clone(),
        Expr::MemberAccess { member, .. } => member.clone(),
        Expr::ComponentAccess { component, .. } => format!("{component:?}"),
        _ => "<expr>".to_owned(),
    }
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

fn unquote_shell_string(text: &str) -> Option<&str> {
    text.strip_prefix('"')?.strip_suffix('"')
}

impl From<NormalizedCommandInvoke> for MayaNormalizedCommand {
    fn from(value: NormalizedCommandInvoke) -> Self {
        Self {
            head: value.head,
            schema_name: value.schema_name,
            kind: value.kind,
            mode: value.mode,
            items: value
                .items
                .into_iter()
                .map(MayaNormalizedCommandItem::from)
                .collect(),
            span: value.range,
        }
    }
}

impl From<NormalizedCommandItem> for MayaNormalizedCommandItem {
    fn from(value: NormalizedCommandItem) -> Self {
        match value {
            NormalizedCommandItem::Flag(flag) => Self::Flag(flag.into()),
            NormalizedCommandItem::Positional(arg) => Self::Positional(arg.into()),
        }
    }
}

impl From<NormalizedFlag> for MayaNormalizedFlag {
    fn from(value: NormalizedFlag) -> Self {
        Self {
            source_text: value.source_text,
            canonical_name: value.canonical_name,
            args: value
                .args
                .into_iter()
                .map(MayaPositionalArg::from)
                .collect(),
            span: value.range,
        }
    }
}

impl From<PositionalArg> for MayaPositionalArg {
    fn from(value: PositionalArg) -> Self {
        Self {
            item: raw_item_from_shell_word(&value.word),
        }
    }
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
    fn to_owned_schema(self) -> FlagSchema {
        FlagSchema {
            long_name: self.long_name.to_owned(),
            short_name: self.short_name.map(str::to_owned),
            mode_mask: self.mode_mask,
            arity_by_mode: self.arity_by_mode,
            value_shapes: self.value_shapes.to_vec(),
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
    flags: &'static [EmbeddedFlagSchema],
}

impl EmbeddedCommandSchema {
    fn to_owned_schema(self) -> CommandSchema {
        CommandSchema {
            name: self.name.to_owned(),
            kind: self.kind,
            source_kind: self.source_kind,
            mode_mask: self.mode_mask,
            return_behavior: self.return_behavior,
            flags: self.build_effective_flags(),
        }
    }

    fn build_effective_flags(self) -> Vec<FlagSchema> {
        let mut flags: Vec<FlagSchema> = self
            .flags
            .iter()
            .copied()
            .map(EmbeddedFlagSchema::to_owned_schema)
            .collect();
        push_synthetic_mode_flag(&mut flags, self.mode_mask.create, "create", "c");
        push_synthetic_mode_flag(&mut flags, self.mode_mask.edit, "edit", "e");
        push_synthetic_mode_flag(&mut flags, self.mode_mask.query, "query", "q");
        flags
    }
}

static EMBEDDED_COMMAND_SCHEMAS: &[EmbeddedCommandSchema] =
    include!(concat!(env!("OUT_DIR"), "/embedded_command_schemas.rs"));

fn push_synthetic_mode_flag(
    flags: &mut Vec<FlagSchema>,
    enabled: bool,
    long_name: &str,
    short_name: &str,
) {
    if !enabled
        || flags.iter().any(|flag| {
            flag.long_name == long_name || flag.short_name.as_deref() == Some(short_name)
        })
    {
        return;
    }

    flags.push(FlagSchema {
        long_name: long_name.to_owned(),
        short_name: Some(short_name.to_owned()),
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
        value_shapes: Vec::new(),
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
    fn lookup(&self, name: &str) -> Option<CommandSchema> {
        self.primary
            .lookup(name)
            .or_else(|| self.fallback.lookup(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mel_ast::{CalleeResolution, InvokeExpr};
    use mel_parser::parse_source;
    use mel_syntax::text_range;

    struct TestRegistry {
        commands: Vec<CommandSchema>,
    }

    impl CommandRegistry for TestRegistry {
        fn lookup(&self, name: &str) -> Option<CommandSchema> {
            self.commands.iter().find(|info| info.name == name).cloned()
        }
    }

    #[test]
    fn embedded_registry_keeps_script_source_kind() {
        let schema = MayaCommandRegistry::new()
            .lookup("addNewShelfTab")
            .expect("embedded schema for addNewShelfTab");
        assert_eq!(schema.kind, CommandKind::Builtin);
        assert_eq!(schema.source_kind, CommandSourceKind::Script);
    }

    #[test]
    fn embedded_registry_synthesizes_mode_flags() {
        let schema = MayaCommandRegistry::new()
            .lookup("addAttr")
            .expect("embedded schema for addAttr");
        assert!(schema.flags.iter().any(|flag| flag.long_name == "create"));
        assert!(schema.flags.iter().any(|flag| flag.long_name == "edit"));
        assert!(schema.flags.iter().any(|flag| flag.long_name == "query"));
    }

    #[test]
    fn collects_top_level_command_proc_and_other_items() {
        let parse = parse_source("global proc foo() { }\nsetAttr \".tx\" 1;\nint $x = 1;\n");
        assert!(parse.errors.is_empty());
        let facts = collect_top_level_facts(&parse.syntax);
        assert!(matches!(facts.items[0], MayaTopLevelItem::Proc { .. }));
        assert!(matches!(facts.items[1], MayaTopLevelItem::Command(_)));
        assert!(matches!(facts.items[2], MayaTopLevelItem::Other { .. }));
    }

    #[test]
    fn raw_items_preserve_exponent_numeric_literals() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "setAttr".to_owned(),
                        words: vec![
                            ShellWord::QuotedString {
                                text: "\".v\"".to_owned(),
                                range: text_range(8, 12),
                            },
                            ShellWord::NumericLiteral {
                                text: ".5e+2".to_owned(),
                                range: text_range(13, 18),
                            },
                        ],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 18),
                }),
                range: text_range(0, 19),
            }))],
        };

        let facts = collect_top_level_facts(&source);
        let MayaTopLevelItem::Command(command) = &facts.items[0] else {
            panic!("expected command");
        };
        assert_eq!(command.raw_items[1].kind, MayaRawShellItemKind::Numeric);
        assert_eq!(command.raw_items[1].value_text.as_deref(), Some(".5e+2"));
    }

    #[test]
    fn set_attr_data_reference_edits_is_specialized_losslessly() {
        let source = SourceFile {
            items: vec![Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "setAttr".to_owned(),
                        words: vec![
                            ShellWord::QuotedString {
                                text: "\".ed\"".to_owned(),
                                range: text_range(8, 13),
                            },
                            ShellWord::Flag {
                                text: "-type".to_owned(),
                                range: text_range(14, 19),
                            },
                            ShellWord::QuotedString {
                                text: "\"dataReferenceEdits\"".to_owned(),
                                range: text_range(20, 40),
                            },
                            ShellWord::QuotedString {
                                text: "\"rootRN\"".to_owned(),
                                range: text_range(41, 49),
                            },
                            ShellWord::QuotedString {
                                text: "\"\"".to_owned(),
                                range: text_range(50, 52),
                            },
                            ShellWord::NumericLiteral {
                                text: "5".to_owned(),
                                range: text_range(53, 54),
                            },
                        ],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 54),
                }),
                range: text_range(0, 55),
            }))],
        };
        let mut command = CommandSchema {
            name: "setAttr".to_owned(),
            kind: CommandKind::Builtin,
            source_kind: CommandSourceKind::Command,
            mode_mask: CommandModeMask {
                create: true,
                edit: true,
                query: true,
            },
            return_behavior: ReturnBehavior::Unknown,
            flags: vec![FlagSchema {
                long_name: "type".to_owned(),
                short_name: Some("typ".to_owned()),
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
                value_shapes: vec![ValueShape::String],
                allows_multiple: false,
            }],
        };
        push_synthetic_mode_flag(&mut command.flags, true, "create", "c");
        push_synthetic_mode_flag(&mut command.flags, true, "edit", "e");
        push_synthetic_mode_flag(&mut command.flags, true, "query", "q");
        let registry = TestRegistry {
            commands: vec![command],
        };

        let facts = collect_top_level_facts_with_registry(&source, &registry);
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
        let facts = collect_top_level_facts(&parse.syntax);
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
}
