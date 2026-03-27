use crate::model::{MayaPromotionKind, MayaTopLevelCommand, MayaTopLevelFacts, MayaTopLevelItem};
use crate::normalize::{
    normalized_invoke_lookup_from_parse, proc_item, raw_item_from_shell_word, stmt_range,
    take_matching_normalized,
};
use crate::registry::OverlayRegistry;
use crate::specialize::specialize_command;
use mel_ast::{Expr, InvokeSurface, Item, Stmt};
use mel_parser::Parse;
use mel_sema::{CommandRegistry, EmptyCommandRegistry};

#[must_use]
pub fn collect_top_level_facts(parse: &Parse) -> MayaTopLevelFacts {
    collect_top_level_facts_with_registry(parse, &EmptyCommandRegistry)
}

#[must_use]
pub fn collect_top_level_facts_with_registry<R>(parse: &Parse, registry: &R) -> MayaTopLevelFacts
where
    R: CommandRegistry + ?Sized,
{
    let overlay = OverlayRegistry::new(registry);
    let analysis = mel_sema::analyze_with_registry(&parse.syntax, parse.source_view(), &overlay);
    let mut remaining_normalized =
        normalized_invoke_lookup_from_parse(parse, analysis.normalized_invokes);
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
                            parse.source_view(),
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
