use super::*;

#[test]
fn parses_proc_fixtures() {
    let parse = parse_source(include_str!(
        "../../../../../tests/corpus/parser/proc/basic-global-proc.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Proc(proc_def) => {
            assert!(proc_def.is_global);
            assert_eq!(parse.source_slice(proc_def.name_range), "greetUser");
            assert!(matches!(
                proc_def.return_type,
                Some(mel_ast::ProcReturnType {
                    ty: TypeName::String,
                    is_array: false,
                    ..
                })
            ));
            assert_eq!(proc_def.params.len(), 1);
            assert!(matches!(proc_def.params[0].ty, TypeName::String));
            assert_eq!(parse.source_slice(proc_def.params[0].name_range), "$name");
            assert!(!proc_def.params[0].is_array);
            assert!(matches!(proc_def.body, Stmt::Block { .. }));
        }
        _ => panic!("expected proc item"),
    }

    let parse = parse_source(include_str!(
        "../../../../../tests/corpus/parser/proc/local-array-param-proc.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Proc(proc_def) => {
            assert!(!proc_def.is_global);
            assert!(proc_def.return_type.is_none());
            assert_eq!(proc_def.params.len(), 1);
            assert!(matches!(proc_def.params[0].ty, TypeName::Vector));
            assert!(proc_def.params[0].is_array);
            assert!(matches!(proc_def.body, Stmt::Block { .. }));
        }
        _ => panic!("expected proc item"),
    }

    let parse = parse_source(include_str!(
        "../../../../../tests/corpus/parser/proc/array-return-proc.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Proc(proc_def) => {
            assert!(matches!(
                proc_def.return_type,
                Some(mel_ast::ProcReturnType {
                    ty: TypeName::String,
                    is_array: true,
                    ..
                })
            ));
            assert_eq!(proc_def.params.len(), 1);
            assert!(matches!(proc_def.params[0].ty, TypeName::String));
            assert!(!proc_def.params[0].is_array);
        }
        _ => panic!("expected proc item"),
    }
}
