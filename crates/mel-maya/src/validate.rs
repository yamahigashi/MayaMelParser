use crate::model::{MayaCommandValidationDiagnostic, MayaTopLevelCommand};
use crate::normalize::{normalized_flags, normalized_positionals};
use mel_syntax::SourceView;

pub(crate) fn validate_maya_command(
    source: SourceView<'_>,
    command: &MayaTopLevelCommand,
) -> Vec<MayaCommandValidationDiagnostic> {
    match command.head.as_str() {
        "setAttr" => validate_set_attr_command(source, command),
        _ => Vec::new(),
    }
}

fn validate_set_attr_command(
    source: SourceView<'_>,
    command: &MayaTopLevelCommand,
) -> Vec<MayaCommandValidationDiagnostic> {
    let Some(normalized) = &command.normalized else {
        return Vec::new();
    };
    let positionals = normalized_positionals(normalized);
    if positionals.is_empty() {
        return Vec::new();
    }

    let values = &positionals[1..];
    if !values.is_empty() {
        return Vec::new();
    }

    let has_type_flag = normalized_flags(normalized)
        .iter()
        .any(|flag| flag.canonical_name.as_deref() == Some("type"));
    let message = if has_type_flag {
        "setAttr requires at least one value after the attribute path when -type is present"
            .to_owned()
    } else {
        "setAttr requires at least one value after the attribute path".to_owned()
    };

    vec![MayaCommandValidationDiagnostic {
        command_span: command.span,
        head: Some(source.slice(normalized.head_range).to_owned()),
        message,
    }]
}
