use crate::model::{
    MayaHybridTopLevelReport, MayaPromotionCandidate, MayaPromotionDecider,
    MayaPromotionDiagnostic, MayaPromotionError, MayaPromotionKind, MayaPromotionOptions,
    MayaPromotionPolicy, MayaTopLevelCommand, MayaTopLevelFacts, MayaTopLevelItem,
    NoopPromotionDecider,
};
use crate::normalize::{
    command_payload_span, normalize_light_command, normalized_invoke_lookup_from_source,
    raw_item_from_light_word, raw_item_from_shell_word_with_source, take_matching_normalized,
};
use crate::registry::OverlayRegistry;
use crate::specialize::specialize_command;
use crate::validate::validate_maya_command;
use mel_ast::{Expr, InvokeSurface, Item, Stmt};
use mel_parser::{
    LightCommandSurface, LightItem, LightParse, parse_source_view_range_with_options,
};
use mel_sema::{CommandRegistry, EmptyCommandRegistry};

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
            LightItem::Command(command) => items.push(MayaTopLevelItem::Command(Box::new(
                promote_or_synthesize_light_command(
                    parse,
                    command,
                    &overlay,
                    options,
                    decider,
                    PromotionErrorMode::Strict,
                )?,
            ))),
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
    let mut facts = MayaTopLevelFacts::default();
    let mut promotion_diagnostics = Vec::new();
    let mut validation_diagnostics = Vec::new();

    for item in &parse.source.items {
        match item {
            LightItem::Proc(proc_def) => facts.items.push(MayaTopLevelItem::Proc {
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
                .expect("report mode should never bubble promotion failure");
                validation_diagnostics.extend(validate_maya_command(parse.source_view(), &command));
                facts
                    .items
                    .push(MayaTopLevelItem::Command(Box::new(command)));
            }
            LightItem::Other { span } => facts.items.push(MayaTopLevelItem::Other { span: *span }),
        }
    }

    MayaHybridTopLevelReport {
        facts,
        promotion_diagnostics,
        validation_diagnostics,
    }
}

pub fn promote_light_top_level_command_with_registry<R>(
    parse: &LightParse,
    command: &LightCommandSurface,
    registry: &R,
    policy: MayaPromotionPolicy,
) -> Result<MayaTopLevelCommand, MayaPromotionError>
where
    R: CommandRegistry + ?Sized,
{
    promote_light_top_level_command_with_registry_and_options(
        parse,
        command,
        registry,
        &MayaPromotionOptions {
            policy,
            ..MayaPromotionOptions::default()
        },
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
    let attempt = promotion_attempt_kind(
        command,
        canonical_name.as_deref(),
        &head,
        &options.policy,
        decider,
    );
    match attempt {
        Some(MayaPromotionKind::CustomDeciderPromoted) => {
            promote_custom_decider_command_with_registry(parse, command, registry, options)
        }
        Some(kind @ MayaPromotionKind::OpaqueTailPromoted) => {
            promote_opaque_command_with_registry(parse, command, registry, options, kind)
        }
        Some(MayaPromotionKind::PolicyPromoted) => {
            promote_policy_command_with_registry(parse, command, registry, options)
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
        normalize_light_command(
            parse,
            &head,
            command.head_range,
            promoted_span,
            schema,
            &raw_items,
        )
    });
    let specialized = specialize_command(
        parse.source_view(),
        &head,
        promoted_span,
        normalized.as_ref(),
        &raw_items,
    );

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
    let mut remaining_normalized =
        normalized_invoke_lookup_from_source(parse.source_view(), analysis.normalized_invokes);

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
        parse.source_view(),
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
