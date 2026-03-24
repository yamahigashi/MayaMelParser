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
    Range { min: u8, max: u8 },
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
pub enum PositionalTailSchema {
    None,
    Shaped {
        min: u8,
        max: Option<u8>,
        value_shapes: &'static [ValueShape],
    },
    Opaque {
        min: u8,
        max: Option<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionalSlotSchema {
    pub value_shapes: &'static [ValueShape],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionalSchema {
    pub prefix: &'static [PositionalSlotSchema],
    pub tail: PositionalTailSchema,
}

impl PositionalSchema {
    #[must_use]
    pub const fn unconstrained() -> Self {
        Self {
            prefix: &[],
            tail: PositionalTailSchema::Opaque { min: 0, max: None },
        }
    }
}

impl Default for PositionalSchema {
    fn default() -> Self {
        Self::unconstrained()
    }
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
    pub positionals: PositionalSchema,
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
