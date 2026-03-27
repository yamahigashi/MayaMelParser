use mel_sema::{
    CommandKind, CommandModeMask, CommandRegistry, CommandSchema, CommandSourceKind, FlagArity,
    FlagArityByMode, FlagSchema, PositionalSchema, PositionalSlotSchema, PositionalSourcePolicy,
    PositionalTailSchema, ReturnBehavior, ValueShape,
};
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

pub(crate) fn push_synthetic_mode_flag(
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

pub(crate) struct OverlayRegistry<'a, R: ?Sized> {
    primary: &'a R,
    fallback: MayaCommandRegistry,
}

impl<'a, R> OverlayRegistry<'a, R>
where
    R: CommandRegistry + ?Sized,
{
    pub(crate) const fn new(primary: &'a R) -> Self {
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
