use crate::model::{MayaLightTopLevelCommand, MayaLightTopLevelFacts, MayaLightTopLevelItem};
use crate::normalize::{LightParseLike, raw_item_from_light_word};
use crate::registry::OverlayRegistry;
use crate::specialize::specialize_light_command;
use mel_parser::{LightItem, LightParse, SharedLightParse};
use mel_sema::{CommandRegistry, EmptyCommandRegistry};

#[must_use]
pub fn collect_top_level_facts_light(parse: &LightParse) -> MayaLightTopLevelFacts {
    collect_top_level_facts_light_with_registry(parse, &EmptyCommandRegistry)
}

#[must_use]
pub fn collect_top_level_facts_light_shared(parse: &SharedLightParse) -> MayaLightTopLevelFacts {
    collect_top_level_facts_light_shared_with_registry(parse, &EmptyCommandRegistry)
}

#[must_use]
pub fn collect_top_level_facts_light_with_registry<R>(
    parse: &LightParse,
    registry: &R,
) -> MayaLightTopLevelFacts
where
    R: CommandRegistry + ?Sized,
{
    collect_top_level_facts_light_impl(parse, registry)
}

#[must_use]
pub fn collect_top_level_facts_light_shared_with_registry<R>(
    parse: &SharedLightParse,
    registry: &R,
) -> MayaLightTopLevelFacts
where
    R: CommandRegistry + ?Sized,
{
    collect_top_level_facts_light_impl(parse, registry)
}

fn collect_top_level_facts_light_impl<R, P>(parse: &P, registry: &R) -> MayaLightTopLevelFacts
where
    R: CommandRegistry + ?Sized,
    P: LightParseLike,
{
    let overlay = OverlayRegistry::new(registry);
    let mut items = Vec::new();

    for item in &parse.light_source().items {
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

pub(crate) fn maya_light_command_from_parse<R>(
    parse: &impl LightParseLike,
    command: &mel_parser::LightCommandSurface,
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
            parse,
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
