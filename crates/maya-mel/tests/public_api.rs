use maya_mel::{analyze, collect_top_level_facts, parse_source};

#[test]
fn root_api_covers_common_workflow() {
    let parse = parse_source("global proc hello() {} hello();");
    let analysis = analyze(&parse.syntax, parse.source_view());
    let facts = collect_top_level_facts(&parse);

    assert!(analysis.diagnostics.is_empty());
    assert!(!facts.items.is_empty());
}

#[test]
fn advanced_modules_remain_public() {
    let lexed = maya_mel::lexer::lex("polyCube -w 1;");
    let parse = maya_mel::parser::parse_source("global proc hello() {}");
    let light = maya_mel::parser::parse_light_source("polyCube -w 1;");
    let facts = maya_mel::maya::collect_top_level_facts_light(&light);
    let range = maya_mel::syntax::text_range(0, 4);
    let mode = maya_mel::sema::command_norm::CommandMode::Create;

    let _: &maya_mel::ast::SourceFile = &parse.syntax;
    let _: maya_mel::maya::model::MayaLightTopLevelFacts = facts.clone();
    let _ = maya_mel::maya::model::MayaTopLevelItem::Proc {
        name: String::from("hello"),
        is_global: true,
        span: range,
    };
    let _ = maya_mel::sema::command_schema::CommandModeMask {
        create: true,
        edit: false,
        query: false,
    };

    assert_eq!(lexed.diagnostics.len(), 0);
    assert_eq!(facts.items.len(), 1);
    assert_eq!(maya_mel::syntax::range_end(range), 4);
    assert!(matches!(
        mode,
        maya_mel::sema::command_norm::CommandMode::Create
    ));
}
