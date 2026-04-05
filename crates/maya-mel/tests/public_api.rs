use maya_mel::{MayaTopLevelItem, analyze, collect_top_level_facts, parse_source};

#[test]
fn root_api_covers_common_workflow() {
    let parse = parse_source("global proc hello() {} hello();");
    let analysis = analyze(&parse.syntax, parse.source_view());
    let facts = collect_top_level_facts(&parse);

    assert!(analysis.diagnostics.is_empty());
    assert!(
        facts
            .items
            .iter()
            .any(|item| matches!(item, MayaTopLevelItem::Proc { .. }))
    );
}

#[test]
fn advanced_modules_remain_public() {
    let light = maya_mel::parser::parse_light_source("polyCube -w 1;");
    let facts = maya_mel::maya::collect_top_level_facts_light(&light);

    assert_eq!(facts.items.len(), 1);
}
