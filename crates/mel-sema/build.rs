use std::{env, fs, path::PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RawRoot {
    schema_version: u32,
    command_count: usize,
    failure_count: usize,
    commands: Vec<RawCommand>,
}

#[derive(Debug, Deserialize)]
struct RawCommand {
    name: String,
    kind: String,
    mode_mask: RawModeMask,
    return_behavior: RawReturnBehavior,
    flags: Vec<RawFlag>,
}

#[derive(Debug, Deserialize)]
struct RawReturnBehavior {
    #[serde(rename = "type")]
    kind: String,
    value_shape: Option<RawValueShape>,
}

#[derive(Debug, Deserialize)]
struct RawFlag {
    long_name: String,
    short_name: Option<String>,
    mode_mask: RawModeMask,
    arity_by_mode: RawArityByMode,
    value_shapes: Vec<RawValueShape>,
    allows_multiple: bool,
}

#[derive(Debug, Deserialize)]
struct RawModeMask {
    create: bool,
    edit: bool,
    query: bool,
}

#[derive(Debug, Deserialize)]
struct RawArity {
    #[serde(rename = "type")]
    kind: String,
    value: Option<u8>,
    min: Option<u8>,
    max: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct RawArityByMode {
    create: RawArity,
    edit: RawArity,
    query: RawArity,
}

#[derive(Debug, Deserialize)]
struct RawValueShape {
    #[serde(rename = "type")]
    kind: String,
    size: Option<u8>,
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let input_path = manifest_dir.join("../../commands/mel_command_schemas_2026.json");
    println!("cargo:rerun-if-changed={}", input_path.display());

    let input = fs::read_to_string(&input_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", input_path.display()));
    let mut root: RawRoot = serde_json::from_str(&input)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", input_path.display()));

    assert_eq!(
        root.schema_version,
        3,
        "unsupported command schema version in {}",
        input_path.display()
    );
    assert_eq!(
        root.failure_count,
        0,
        "command schema import contains failures in {}",
        input_path.display()
    );
    assert_eq!(
        root.command_count,
        root.commands.len(),
        "command_count mismatch in {}",
        input_path.display()
    );

    root.commands.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));

    let mut rendered = String::from("&[\n");
    for command in &root.commands {
        rendered.push_str("    EmbeddedCommandSchema {\n");
        rendered.push_str("        name: ");
        rendered.push_str(&render_string(&command.name));
        rendered.push_str(",\n");
        rendered.push_str("        kind: CommandKind::Builtin,\n");
        rendered.push_str("        source_kind: ");
        rendered.push_str(match command.kind.as_str() {
            "command" => "CommandSourceKind::Command",
            "script" => "CommandSourceKind::Script",
            other => panic!("unsupported command kind {other:?}"),
        });
        rendered.push_str(",\n");
        rendered.push_str("        mode_mask: CommandModeMask {\n");
        rendered.push_str(&format!(
            "            create: {},\n            edit: {},\n            query: {},\n",
            command.mode_mask.create, command.mode_mask.edit, command.mode_mask.query
        ));
        rendered.push_str("        },\n");
        rendered.push_str("        return_behavior: ");
        rendered.push_str(&render_return_behavior(&command.return_behavior));
        rendered.push_str(",\n");
        rendered.push_str("        flags: &[\n");
        for flag in &command.flags {
            rendered.push_str("            EmbeddedFlagSchema {\n");
            rendered.push_str("                long_name: ");
            rendered.push_str(&render_string(&flag.long_name));
            rendered.push_str(",\n");
            rendered.push_str("                short_name: ");
            match &flag.short_name {
                Some(short_name) => {
                    rendered.push_str("Some(");
                    rendered.push_str(&render_string(short_name));
                    rendered.push_str("),\n");
                }
                None => rendered.push_str("None,\n"),
            }
            rendered.push_str("                mode_mask: CommandModeMask {\n");
            rendered.push_str(&format!(
                "                    create: {},\n                    edit: {},\n                    query: {},\n",
                flag.mode_mask.create, flag.mode_mask.edit, flag.mode_mask.query
            ));
            rendered.push_str("                },\n");
            rendered.push_str("                arity_by_mode: FlagArityByMode {\n");
            rendered.push_str("                    create: ");
            rendered.push_str(&render_arity(&flag.arity_by_mode.create));
            rendered.push_str(",\n");
            rendered.push_str("                    edit: ");
            rendered.push_str(&render_arity(&flag.arity_by_mode.edit));
            rendered.push_str(",\n");
            rendered.push_str("                    query: ");
            rendered.push_str(&render_arity(&flag.arity_by_mode.query));
            rendered.push_str(",\n");
            rendered.push_str("                },\n");
            rendered.push_str("                value_shapes: &[");
            for (index, value_shape) in flag.value_shapes.iter().enumerate() {
                if index > 0 {
                    rendered.push_str(", ");
                }
                rendered.push_str(&render_value_shape(value_shape));
            }
            rendered.push_str("],\n");
            rendered.push_str(&format!(
                "                allows_multiple: {},\n",
                flag.allows_multiple
            ));
            rendered.push_str("            },\n");
        }
        rendered.push_str("        ],\n");
        rendered.push_str("    },\n");
    }
    rendered.push_str("]\n");

    let out_path =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("embedded_command_schemas.rs");
    fs::write(&out_path, rendered)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", out_path.display()));
}

fn render_string(value: &str) -> String {
    serde_json::to_string(value).expect("string render")
}

fn render_return_behavior(value: &RawReturnBehavior) -> String {
    match value.kind.as_str() {
        "none" => "ReturnBehavior::None".to_owned(),
        "fixed" => format!(
            "ReturnBehavior::Fixed({})",
            render_value_shape(
                value
                    .value_shape
                    .as_ref()
                    .expect("fixed return behavior requires value_shape"),
            )
        ),
        "query_depends_on_flag" => "ReturnBehavior::QueryDependsOnFlag".to_owned(),
        "unknown" => "ReturnBehavior::Unknown".to_owned(),
        other => panic!("unsupported return behavior {other:?}"),
    }
}

fn render_arity(value: &RawArity) -> String {
    match value.kind.as_str() {
        "none" => "FlagArity::None".to_owned(),
        "exact" => format!(
            "FlagArity::Exact({})",
            value.value.expect("exact arity requires value")
        ),
        "range" => {
            let min = value.min.expect("range arity requires min");
            let max = value.max.expect("range arity requires max");
            assert!(min <= max, "range arity requires min <= max");
            format!("FlagArity::Range {{ min: {min}, max: {max} }}")
        }
        other => panic!("unsupported arity kind {other:?}"),
    }
}

fn render_value_shape(value: &RawValueShape) -> String {
    match value.kind.as_str() {
        "bool" => "ValueShape::Bool".to_owned(),
        "int" => "ValueShape::Int".to_owned(),
        "float" => "ValueShape::Float".to_owned(),
        "string" => "ValueShape::String".to_owned(),
        "script" => "ValueShape::Script".to_owned(),
        "string_array" => "ValueShape::StringArray".to_owned(),
        "node_name" => "ValueShape::NodeName".to_owned(),
        "unknown" => "ValueShape::Unknown".to_owned(),
        "float_tuple" => format!(
            "ValueShape::FloatTuple({})",
            value.size.expect("float_tuple requires size")
        ),
        "int_tuple" => format!(
            "ValueShape::IntTuple({})",
            value.size.expect("int_tuple requires size")
        ),
        other => panic!("unsupported value shape {other:?}"),
    }
}
