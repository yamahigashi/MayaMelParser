#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Builtin,
    Plugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSourceKind {
    Command,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnBehavior {
    None,
    Fixed(ValueShape),
    QueryDependsOnFlag,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagArity {
    None,
    Exact(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagArityByMode {
    pub create: FlagArity,
    pub edit: FlagArity,
    pub query: FlagArity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueShape {
    Bool,
    Int,
    Float,
    String,
    Script,
    StringArray,
    FloatTuple(u8),
    IntTuple(u8),
    NodeName,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandModeMask {
    pub create: bool,
    pub edit: bool,
    pub query: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagSchema {
    pub long_name: String,
    pub short_name: Option<String>,
    pub mode_mask: CommandModeMask,
    pub arity_by_mode: FlagArityByMode,
    pub value_shapes: Vec<ValueShape>,
    pub allows_multiple: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSchema {
    pub name: String,
    pub kind: CommandKind,
    pub source_kind: CommandSourceKind,
    pub mode_mask: CommandModeMask,
    pub return_behavior: ReturnBehavior,
    pub flags: Vec<FlagSchema>,
}

pub trait CommandRegistry {
    fn lookup(&self, name: &str) -> Option<CommandSchema>;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EmptyCommandRegistry;

impl CommandRegistry for EmptyCommandRegistry {
    fn lookup(&self, _name: &str) -> Option<CommandSchema> {
        None
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedCommandRegistry;

impl EmbeddedCommandRegistry {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl CommandRegistry for EmbeddedCommandRegistry {
    fn lookup(&self, name: &str) -> Option<CommandSchema> {
        EMBEDDED_COMMAND_SCHEMAS
            .binary_search_by(|schema| schema.name.cmp(name))
            .ok()
            .map(|index| EMBEDDED_COMMAND_SCHEMAS[index].to_owned_schema())
    }
}

pub(crate) struct OverlayCommandRegistry<'a, R: ?Sized> {
    primary: &'a R,
    fallback: EmbeddedCommandRegistry,
}

impl<'a, R> OverlayCommandRegistry<'a, R>
where
    R: CommandRegistry + ?Sized,
{
    pub(crate) const fn new(primary: &'a R) -> Self {
        Self {
            primary,
            fallback: EmbeddedCommandRegistry::new(),
        }
    }
}

impl<R> CommandRegistry for OverlayCommandRegistry<'_, R>
where
    R: CommandRegistry + ?Sized,
{
    fn lookup(&self, name: &str) -> Option<CommandSchema> {
        self.primary
            .lookup(name)
            .or_else(|| self.fallback.lookup(name))
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
