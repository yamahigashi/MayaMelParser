use std::{ops::Deref, sync::Arc};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionalSourcePolicy {
    #[default]
    ExplicitOnly,
    ExplicitOrCurrentSelection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionalSlotSchema {
    pub value_shapes: &'static [ValueShape],
    pub source_policy: PositionalSourcePolicy,
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

impl PositionalSchema {
    fn validate(self, command_name: &Arc<str>) -> Result<(), CommandSchemaValidationError> {
        let mut seen_selection_aware = false;
        for (slot_index, slot) in self.prefix.iter().enumerate() {
            let is_selection_aware = matches!(
                slot.source_policy,
                PositionalSourcePolicy::ExplicitOrCurrentSelection
            );
            if is_selection_aware {
                seen_selection_aware = true;
                continue;
            }
            if seen_selection_aware {
                return Err(
                    CommandSchemaValidationError::SelectionAwarePositionalNotTrailingSuffix {
                        command_name: command_name.clone(),
                        slot_index,
                    },
                );
            }
        }

        Ok(())
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
    pub long_name: Arc<str>,
    pub short_name: Option<Arc<str>>,
    pub mode_mask: CommandModeMask,
    pub arity_by_mode: FlagArityByMode,
    pub value_shapes: Arc<[ValueShape]>,
    pub allows_multiple: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSchema {
    pub name: Arc<str>,
    pub kind: CommandKind,
    pub source_kind: CommandSourceKind,
    pub mode_mask: CommandModeMask,
    pub return_behavior: ReturnBehavior,
    pub flags: Arc<[FlagSchema]>,
    pub positionals: PositionalSchema,
}

impl CommandSchema {
    pub fn validate(self) -> Result<ValidatedCommandSchema, CommandSchemaValidationError> {
        self.positionals.validate(&self.name)?;
        Ok(ValidatedCommandSchema(self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSchemaValidationError {
    SelectionAwarePositionalNotTrailingSuffix {
        command_name: Arc<str>,
        slot_index: usize,
    },
}

impl std::fmt::Display for CommandSchemaValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelectionAwarePositionalNotTrailingSuffix {
                command_name,
                slot_index,
            } => write!(
                f,
                "command schema \"{command_name}\" has non-trailing selection-aware positional slot before explicit-only slot at prefix index {slot_index}"
            ),
        }
    }
}

impl std::error::Error for CommandSchemaValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedCommandSchema(CommandSchema);

impl ValidatedCommandSchema {
    pub fn new(schema: CommandSchema) -> Result<Self, CommandSchemaValidationError> {
        schema.validate()
    }

    #[must_use]
    pub fn schema(&self) -> &CommandSchema {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> CommandSchema {
        self.0
    }
}

impl TryFrom<CommandSchema> for ValidatedCommandSchema {
    type Error = CommandSchemaValidationError;

    fn try_from(value: CommandSchema) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Deref for ValidatedCommandSchema {
    type Target = CommandSchema;

    fn deref(&self) -> &Self::Target {
        self.schema()
    }
}

pub trait CommandRegistry {
    fn lookup(&self, name: &str) -> Option<&ValidatedCommandSchema>;
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EmptyCommandRegistry;

impl CommandRegistry for EmptyCommandRegistry {
    fn lookup(&self, _name: &str) -> Option<&ValidatedCommandSchema> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StaticCommandRegistry {
    commands: Vec<ValidatedCommandSchema>,
}

impl StaticCommandRegistry {
    pub fn try_new(commands: Vec<CommandSchema>) -> Result<Self, CommandSchemaValidationError> {
        let mut validated = Vec::with_capacity(commands.len());
        for command in commands {
            validated.push(command.validate()?);
        }
        Ok(Self {
            commands: validated,
        })
    }
}

impl CommandRegistry for StaticCommandRegistry {
    fn lookup(&self, name: &str) -> Option<&ValidatedCommandSchema> {
        self.commands.iter().find(|info| info.name.as_ref() == name)
    }
}
