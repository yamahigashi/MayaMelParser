use super::{
    ParseMode, ParseOptions, SourceEncoding, parse_bytes, parse_bytes_with_encoding, parse_source,
    parse_source_with_options,
};
use encoding_rs::{GBK, SHIFT_JIS};
use mel_ast::{
    AssignOp, BinaryOp, Expr, InvokeSurface, Item, ShellWord, Stmt, SwitchLabel, TypeName, UnaryOp,
    UpdateOp, VectorComponent,
};
use mel_syntax::text_range;

#[test]
fn parses_proc_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/proc/basic-global-proc.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Proc(proc_def) => {
            assert!(proc_def.is_global);
            assert_eq!(proc_def.name, "greetUser");
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
            assert_eq!(proc_def.params[0].name, "$name");
            assert!(!proc_def.params[0].is_array);
            assert!(matches!(proc_def.body, Stmt::Block { .. }));
        }
        _ => panic!("expected proc item"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/proc/local-array-param-proc.mel"
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
        "../../../tests/corpus/parser/proc/array-return-proc.mel"
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

#[test]
fn parses_nested_proc_definition_statement_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/nested-proc-definition.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Proc(proc_def) => match &proc_def.body {
            Stmt::Block { statements, .. } => {
                assert!(matches!(statements[0], Stmt::Proc { .. }));
                assert!(matches!(statements[1], Stmt::Proc { .. }));
            }
            _ => panic!("expected proc body block"),
        },
        _ => panic!("expected outer proc item"),
    }
}

#[test]
fn reports_missing_nested_proc_body() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-nested-proc-missing-body.mel"
    ));
    assert!(
        parse
            .errors
            .iter()
            .any(|error| error.message == "expected proc body block")
    );
}

#[test]
fn parses_command_statement_with_flags() {
    let parse = parse_source("frameLayout -edit -label $title $fl;");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(words[1], ShellWord::Flag { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected command statement"),
    }
}

#[test]
fn parses_command_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "print");
                    assert!(matches!(words[0], ShellWord::BareWord { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected command statement"),
    }
}

#[test]
fn parses_command_dotdot_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotdot-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "setParent");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. } if text == ".."
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_dotdot_after_flag_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotdot-flag-arg.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. } if text == ".."
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_dotdot_without_whitespace() {
    let parse = parse_source("setParent..;");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "setParent");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. } if text == ".."
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn keeps_quoted_dotdot_as_quoted_string() {
    let parse = parse_source(r#"setParent "..";"#);
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(
                        words[0],
                        ShellWord::QuotedString { ref text, .. } if text == "\"..\""
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn keeps_no_whitespace_ident_lparen_as_function_stmt() {
    let parse = parse_source("doItDRA();");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::Function { name, args } => {
                    assert_eq!(name, "doItDRA");
                    assert!(args.is_empty());
                }
                _ => panic!("expected function invoke"),
            },
            _ => panic!("expected expression statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_function_stmt_spaced_lparen_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/function-stmt-spaced-lparen.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::Function { name, args } => {
                    assert_eq!(name, "tmBuildSet");
                    assert_eq!(args.len(), 2);
                }
                _ => panic!("expected function invoke"),
            },
            _ => panic!("expected expression statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_leading_grouped_arg_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-leading-grouped-arg.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "renameAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::GroupedExpr {
                            expr: Expr::Binary { .. },
                            ..
                        }
                    ));
                    assert!(matches!(words[1], ShellWord::Variable { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_numeric_arg_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-numeric-arg.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(words[1], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[2],
                        ShellWord::NumericLiteral { ref text, .. } if text == "0"
                    ));
                    assert!(matches!(words[3], ShellWord::Variable { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_signed_numeric_arg_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-signed-numeric-arg.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(
                        words[3],
                        ShellWord::NumericLiteral { ref text, .. } if text == "-10"
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_leading_dot_float_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-leading-dot-float.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::NumericLiteral { ref text, .. } if text == ".7"
                    ));
                    assert!(matches!(words[2], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[3],
                        ShellWord::NumericLiteral { ref text, .. } if text == ".001"
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_trailing_dot_float_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-trailing-dot-float.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => {
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(words[1], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[2],
                        ShellWord::NumericLiteral { ref text, .. } if text == "-1000."
                    ));
                    assert!(matches!(words[3], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[4],
                        ShellWord::NumericLiteral { ref text, .. } if text == "1000."
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_grouped_subtraction_call_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/grouped-subtraction-call.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::Function { args, .. } => {
                        assert!(matches!(
                            args[1],
                            Expr::Binary {
                                op: BinaryOp::Sub,
                                ..
                            }
                        ));
                    }
                    _ => panic!("expected function invoke"),
                },
                _ => panic!("expected invoke initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_spaced_flag_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-spaced-flag.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "optionVar");
                        assert!(matches!(
                            words[0],
                            ShellWord::Flag { ref text, .. } if text == "- q"
                        ));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                            if text == "LayoutPreviewResolution"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected invoke initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_multiline_grouped_args_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-multiline-grouped-args.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "connectAttr");
                    assert_eq!(words.len(), 2);
                    assert!(matches!(words[0], ShellWord::GroupedExpr { .. }));
                    assert!(matches!(words[1], ShellWord::GroupedExpr { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "setAttr");
                    assert!(matches!(words[0], ShellWord::GroupedExpr { .. }));
                    assert!(
                        matches!(words[1], ShellWord::Flag { ref text, .. } if text == "-type")
                    );
                    assert!(matches!(
                        words[2],
                        ShellWord::BareWord { ref text, .. } if text == "double3"
                    ));
                    assert!(matches!(words[3], ShellWord::GroupedExpr { .. }));
                    assert!(matches!(words[4], ShellWord::GroupedExpr { .. }));
                    assert!(matches!(words[5], ShellWord::GroupedExpr { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_point_constraint_brace_list_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-point-constraint-brace-list.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "applyPointConstraintArgs");
                    assert!(matches!(
                        words[0],
                        ShellWord::NumericLiteral { ref text, .. } if text == "2"
                    ));
                    assert!(matches!(
                        words[1],
                        ShellWord::BraceList {
                            expr: Expr::ArrayLiteral { ref elements, .. },
                            ..
                        } if elements.len() == 10
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_orient_constraint_brace_list_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-orient-constraint-brace-list.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "applyOrientConstraintArgs");
                    match &words[1] {
                        ShellWord::BraceList {
                            expr: Expr::ArrayLiteral { elements, .. },
                            ..
                        } => {
                            assert!(matches!(
                                elements[0],
                                Expr::String { ref text, .. } if text == "\"1\""
                            ));
                            assert!(matches!(
                                elements[7],
                                Expr::String { ref text, .. } if text == "\"8\""
                            ));
                            assert!(matches!(
                                elements[8],
                                Expr::String { ref text, .. } if text == "\"\""
                            ));
                        }
                        _ => panic!("expected brace-list shell word"),
                    }
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_capture_vector_literal_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-capture-vector-literal.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::ShellLike {
                        head,
                        words,
                        captured,
                    } => {
                        assert_eq!(head, "hsv_to_rgb");
                        assert!(*captured);
                        assert!(matches!(
                            words[0],
                            ShellWord::VectorLiteral {
                                expr: Expr::VectorLiteral { ref elements, .. },
                                ..
                            } if elements.len() == 3
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected invoke initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "text");
                    assert!(matches!(
                        words[2],
                        ShellWord::GroupedExpr {
                            expr: Expr::ComponentAccess { .. },
                            ..
                        }
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_dotted_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotted-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "setDrivenKeyframe");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if text == "N_arm_01.rotateX"
                    ));
                    assert!(matches!(
                        words[2],
                        ShellWord::BareWord { ref text, .. }
                            if text == "N_arm_01_H.rotateX"
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_dotted_indexed_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotted-indexed-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "connectAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. }
                            if text == "foo.worldMatrix[0]"
                    ));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if text == "bar.inputWorldMatrix"
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_dotted_variable_indexed_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotted-variable-indexed-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "connectAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::GroupedExpr {
                            expr: Expr::Binary { .. },
                            ..
                        }
                    ));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if text == "LayerRegistry.layerSlot[$index]"
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_dotted_global_attr_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-dotted-global-attr.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "getAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. }
                            if text == "defaultRenderGlobals.hyperShadeBinList"
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_pipe_dag_path_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-pipe-dag-path.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "select");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if text == "Null|Spine_00|Tail_00"
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_pipe_wildcard_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-pipe-wildcard-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "select");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. } if text == "*|_x005"
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_absolute_plug_path_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-absolute-plug-path.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "defaultNavigation");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(
                        matches!(words[1], ShellWord::BareWord { ref text, .. } if text == "shaderNodePreview1")
                    );
                    assert!(matches!(words[2], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[3],
                        ShellWord::BareWord { ref text, .. }
                            if text == "|geoPreview1|geoPreviewShape1.instObjGroups[0]"
                    ));
                    assert!(matches!(words[4], ShellWord::Flag { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_namespace_pipe_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-namespace-pipe-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "select");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if text == "ns:root|ns:spine|ns:ctrl"
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_leading_colon_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-leading-colon-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::ShellLike { head, words, .. } => {
                        assert_eq!(head, "camera");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                                if text == ":previewViewportCamera"
                        ));
                    }
                    _ => panic!("expected shell-like invoke"),
                },
                _ => panic!("expected invoke initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_command_grouped_args_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-grouped-args.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "iconTextButton");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(words[1], ShellWord::QuotedString { .. }));
                    assert!(matches!(
                        words[5],
                        ShellWord::GroupedExpr {
                            expr: Expr::Binary { .. },
                            ..
                        }
                    ));
                    assert!(matches!(
                        words[7],
                        ShellWord::GroupedExpr {
                            expr: Expr::Binary { .. },
                            ..
                        }
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected command statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "menuItem");
                    assert!(matches!(
                        words[1],
                        ShellWord::GroupedExpr {
                            expr: Expr::Binary { .. },
                            ..
                        }
                    ));
                    assert!(matches!(
                        words[3],
                        ShellWord::Variable {
                            expr: Expr::MemberAccess { ref member, .. },
                            ..
                        } if member == "name"
                    ));
                    assert!(matches!(words[5], ShellWord::Capture { .. }));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected command statement"),
    }
}

#[test]
fn parses_command_capture_grouped_function_call_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/command-capture-grouped-function-call.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Unary { expr, .. }) => match &**expr {
                    Expr::Invoke(invoke) => match &invoke.surface {
                        InvokeSurface::ShellLike {
                            head,
                            words,
                            captured,
                        } => {
                            assert_eq!(head, "optionVar");
                            assert!(*captured);
                            assert!(matches!(words[0], ShellWord::Flag { .. }));
                            assert!(matches!(
                                words[1],
                                ShellWord::GroupedExpr {
                                    expr: Expr::Invoke(_),
                                    ..
                                }
                            ));
                        }
                        _ => panic!("expected shell-like capture"),
                    },
                    _ => panic!("expected invoke under unary expression"),
                },
                _ => panic!("expected unary initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "optionVar");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::GroupedExpr {
                            expr: Expr::Invoke(_),
                            ..
                        }
                    ));
                    assert!(matches!(
                        words[2],
                        ShellWord::GroupedExpr {
                            expr: Expr::Invoke(_),
                            ..
                        }
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_index_and_add_assign() {
    let parse = parse_source("$items[$i] += 1;");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Assign { op, lhs, .. },
                ..
            } => {
                assert!(matches!(op, AssignOp::AddAssign));
                assert!(matches!(**lhs, Expr::Index { .. }));
            }
            _ => panic!("expected add-assign statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_operator_precedence() {
    let parse = parse_source("$value = 1 + 2 * 3;");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary {
                    op: BinaryOp::Add,
                    rhs,
                    ..
                } => {
                    assert!(matches!(
                        **rhs,
                        Expr::Binary {
                            op: BinaryOp::Mul,
                            ..
                        }
                    ));
                }
                _ => panic!("expected additive expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_ternary_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/ternary-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 3);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => assert!(matches!(**rhs, Expr::Ternary { .. })),
            _ => panic!("expected ternary assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_exponent_float_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/exponent-float-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 3);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => assert!(matches!(
                **rhs,
                Expr::Float { ref text, .. } if text == "1.0e-3"
            )),
            _ => panic!("expected exponent assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::Float { ref text, .. } if text == "1e+3"
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::Float { ref text, .. } if text == "0.0e0"
                    ));
                }
                _ => panic!("expected exponent binary expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[2] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => {
                assert!(matches!(
                    decl.declarators[0].initializer,
                    Some(Expr::ArrayLiteral { .. })
                ));
            }
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_trailing_dot_float_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/trailing-dot-float-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 3);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => assert!(matches!(
                **rhs,
                Expr::Float { ref text, .. } if text == "1000."
            )),
            _ => panic!("expected trailing-dot float assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::Float { ref text, .. } if text == "0."
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::Float { ref text, .. } if text == "1."
                    ));
                }
                _ => panic!("expected binary expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[2] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::ArrayLiteral { elements, .. } => {
                    assert_eq!(elements.len(), 3);
                    assert!(matches!(
                        elements[0],
                        Expr::Float { ref text, .. } if text == "0."
                    ));
                    assert!(matches!(
                        elements[1],
                        Expr::Float { ref text, .. } if text == "1."
                    ));
                    assert!(matches!(
                        elements[2],
                        Expr::Float { ref text, .. } if text == "2."
                    ));
                }
                _ => panic!("expected brace-list assignment"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_for_in_and_for_loop_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/while-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::While { .. })
    ));

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/for-loop-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::For { .. })
    ));

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/for-loop-multi-init-update.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::For {
                init: Some(init),
                condition: Some(_),
                update: Some(update),
                ..
            } => {
                assert_eq!(init.len(), 2);
                assert_eq!(update.len(), 2);
            }
            _ => panic!("expected classic for statement with multi-clause init/update"),
        },
        _ => panic!("expected statement"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/for-in-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::ForIn { .. })
    ));
}

#[test]
fn parses_if_else_and_break_continue_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/if-else-command.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::If { .. })
    ));

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/break-continue.mel"
    ));
    assert!(parse.errors.is_empty());
}

#[test]
fn parses_switch_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/switch-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Switch { clauses, .. } => {
                assert_eq!(clauses.len(), 3);
                assert!(matches!(clauses[0].label, SwitchLabel::Default { .. }));
                assert!(clauses[0].statements.len() == 2);
                assert!(matches!(clauses[1].label, SwitchLabel::Case(_)));
                assert!(clauses[1].statements.is_empty());
                assert!(matches!(clauses[2].label, SwitchLabel::Case(_)));
                assert!(clauses[2].statements.len() == 2);
            }
            _ => panic!("expected switch statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_postfix_update_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/postfix-update.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::PostfixUpdate { op, .. },
                ..
            } => assert!(matches!(op, UpdateOp::Increment)),
            _ => panic!("expected postfix update"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_compound_assign_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/compound-assign-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 3);

    let expected = [
        AssignOp::SubAssign,
        AssignOp::MulAssign,
        AssignOp::DivAssign,
    ];

    for (item, expected_op) in parse.syntax.items.iter().zip(expected) {
        match item {
            Item::Stmt(stmt) => match &**stmt {
                Stmt::Expr {
                    expr: Expr::Assign { op, .. },
                    ..
                } => assert_eq!(*op, expected_op),
                _ => panic!("expected compound assignment"),
            },
            _ => panic!("expected statement"),
        }
    }
}

#[test]
fn parses_prefix_update_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/prefix-update-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::PrefixUpdate { op, .. },
                ..
            } => assert!(matches!(op, UpdateOp::Increment)),
            _ => panic!("expected prefix increment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::PrefixUpdate { op, .. },
                ..
            } => assert!(matches!(op, UpdateOp::Decrement)),
            _ => panic!("expected prefix decrement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_do_while_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/do-while-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::DoWhile { body, .. } => {
                assert!(matches!(&**body, Stmt::Block { .. }));
            }
            _ => panic!("expected do-while statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_variable_declaration_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/var-decl-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => {
                assert!(matches!(decl.ty, TypeName::Int));
                assert_eq!(decl.declarators.len(), 1);
                assert_eq!(decl.declarators[0].name, "$count");
            }
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/global-var-decl.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => {
                assert!(decl.is_global);
                assert!(matches!(decl.ty, TypeName::String));
                assert!(decl.declarators[0].array_size.is_some());
                assert!(matches!(
                    decl.declarators[0].initializer,
                    Some(Expr::ArrayLiteral { .. })
                ));
            }
            _ => panic!("expected global variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/var-decl-multi-array.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => {
                assert_eq!(decl.declarators.len(), 2);
                assert!(matches!(
                    decl.declarators[1].initializer,
                    Some(Expr::ArrayLiteral { .. })
                ));
            }
            _ => panic!("expected multi declarator"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_brace_list_assignment_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/brace-list-assign.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => {
                assert!(matches!(**rhs, Expr::ArrayLiteral { .. }));
            }
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_cast_expression_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/cast-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Cast { ty, expr, .. }) => {
                    assert!(matches!(ty, TypeName::String));
                    assert!(matches!(**expr, Expr::Ident { .. }));
                }
                _ => panic!("expected string cast initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Cast { ty, expr, .. }) => {
                    assert!(matches!(ty, TypeName::Int));
                    assert!(matches!(**expr, Expr::Binary { .. }));
                }
                _ => panic!("expected int cast initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[2] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Cast { ty, expr, .. } => {
                    assert!(matches!(ty, TypeName::String));
                    assert!(matches!(**expr, Expr::Invoke(_)));
                }
                _ => panic!("expected nested string cast"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_path_like_bareword_expression_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/path-like-bareword-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::BareWord { text, .. }) => {
                    assert_eq!(text, "AA_Bar*|mdl|_XXa0|");
                }
                _ => panic!("expected path-like bareword initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_hex_integer_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/hex-int-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(**lhs, Expr::Int { value, .. } if value == 0x8000));
                    assert!(matches!(**rhs, Expr::Int { value, .. } if value == 0x0001));
                }
                _ => panic!("expected hex integer binary expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_caret_operator_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/caret-operator-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Binary { op, .. }) => assert_eq!(op, &BinaryOp::Caret),
                _ => panic!("expected caret binary expression"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_two_element_vector_literal_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/vector-literal-two-elements.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::VectorLiteral { elements, .. }) => assert_eq!(elements.len(), 2),
                _ => panic!("expected vector literal initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_unary_negate_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/unary-negate-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::Unary {
                            op: UnaryOp::Negate,
                            ..
                        }
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::Unary {
                            op: UnaryOp::Negate,
                            ..
                        }
                    ));
                }
                _ => panic!("expected binary negate expression"),
            },
            _ => panic!("expected assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_vector_literal_and_component_fixtures() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/vector-literal-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => {
                assert!(matches!(**rhs, Expr::VectorLiteral { .. }));
            }
            _ => panic!("expected vector assignment"),
        },
        _ => panic!("expected statement"),
    }

    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/vector-component-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::ComponentAccess {
                            component: VectorComponent::X,
                            ..
                        }
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::ComponentAccess {
                            component: VectorComponent::Y,
                            ..
                        }
                    ));
                }
                _ => panic!("expected binary component access"),
            },
            _ => panic!("expected vector component assignment"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_member_access_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/member-access-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => match &**rhs {
                Expr::Binary { lhs, rhs, .. } => {
                    assert!(matches!(
                        **lhs,
                        Expr::MemberAccess { ref member, .. } if member == "foo"
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::MemberAccess { ref member, .. } if member == "bar"
                    ));
                }
                _ => panic!("expected binary member access"),
            },
            _ => panic!("expected member access assignment"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr:
                    Expr::Assign {
                        rhs,
                        op: AssignOp::Assign,
                        ..
                    },
                ..
            } => assert!(matches!(
                **rhs,
                Expr::MemberAccess { ref member, .. } if member == "name"
            )),
            _ => panic!("expected indexed member access"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn reports_missing_index_bracket_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-index-bracket.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected ']' after index expression"
    );
}

#[test]
fn reports_missing_proc_body_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/proc/missing-proc-body.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected proc body block");
}

#[test]
fn reports_missing_proc_param_name_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/proc/missing-proc-param-name.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected '$' before proc parameter name"
    );
}

#[test]
fn reports_missing_compound_assign_rhs_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-compound-assign-rhs.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after operator"
    );
}

#[test]
fn reports_missing_ternary_colon_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-ternary-colon.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected ':' in ternary expression"
    );
}

#[test]
fn reports_missing_prefix_update_operand_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-prefix-update-operand.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after prefix update"
    );
}

#[test]
fn reports_missing_do_while_semi_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-do-while-semi.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected ';' after do-while statement"
    );
}

#[test]
fn reports_missing_for_clause_expr_after_comma_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-for-clause-expr-after-comma.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after ',' in for clause"
    );
}

#[test]
fn reports_missing_var_declarator_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-var-declarator.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected variable declarator");
}

#[test]
fn reports_missing_while_condition_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-while-condition.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected while condition");
}

#[test]
fn reports_missing_while_body_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-while-body.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected while body");
}

#[test]
fn reports_missing_switch_case_value_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-switch-case-value.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected case value");
}

#[test]
fn reports_missing_switch_colon_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-switch-colon.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected ':' after switch label");
}

#[test]
fn reports_missing_unary_negate_operand_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-unary-negate-operand.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after unary operator"
    );
}

#[test]
fn reports_missing_caret_rhs_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-caret-rhs.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after operator"
    );
}

#[test]
fn reports_missing_brace_list_close_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-brace-list-close.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected '}' to close brace list");
}

#[test]
fn reports_missing_cast_operand_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-cast-operand.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected expression after cast");
}

#[test]
fn reports_malformed_path_like_bareword_missing_segment_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/malformed-path-like-bareword-missing-segment.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected expression inside index");
}

#[test]
fn reports_missing_vector_close_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-vector-close.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected '>>' to close vector literal"
    );
}

#[test]
fn reports_missing_member_name_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/missing-member-name.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected member name after '.'");
}

#[test]
fn reports_trailing_dot_float_double_dot_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/expressions/trailing-dot-float-double-dot.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected member name after '.'");
}

#[test]
fn reports_malformed_command_word_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-word.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_missing_closing_backquote_without_command_cascade_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-missing-closing-backquote.mel"
    ));
    assert_eq!(parse.errors.len(), 1);
    assert_eq!(parse.errors[0].message, "expected closing backquote");
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { head, words, .. } => {
                    assert_eq!(head, "optionVar");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::GroupedExpr {
                            expr: Expr::Invoke(_),
                            ..
                        }
                    ));
                    assert!(matches!(
                        words[2],
                        ShellWord::GroupedExpr {
                            expr: Expr::Invoke(_),
                            ..
                        }
                    ));
                }
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn reports_malformed_command_signed_number_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-signed-number.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_spaced_flag_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-spaced-flag.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_single_dot_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-single-dot.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_leading_dot_no_digit_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-leading-dot-no-digit.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_spaced_dotted_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-spaced-dotted-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_spaced_pipe_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-spaced-pipe-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_empty_pipe_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-empty-pipe-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_pipe_followed_by_whitespace() {
    let parse = parse_source("select -r | spine_00;");
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_trailing_pipe_before_semicolon() {
    let parse = parse_source("select -r y_ang| ;");
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_spaced_namespace_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-spaced-namespace-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_leading_colon_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-leading-colon-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_dotted_variable_indexed_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-dotted-variable-indexed-bareword.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "unexpected token in command invocation"
    );
}

#[test]
fn reports_malformed_command_brace_list_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-brace-list.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected '}' to close brace list");
}

#[test]
fn reports_malformed_command_vector_literal_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/malformed-command-vector-literal.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected '>>' to close vector literal"
    );
}

#[test]
fn recovers_missing_statement_parens_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-statement-parens.mel"
    ));
    assert_eq!(parse.errors.len(), 2);
    assert_eq!(
        parse.errors[0].message,
        "expected ')' to close grouped expression"
    );
    assert_eq!(
        parse.errors[1].message,
        "expected ')' to close function invocation"
    );
    assert_eq!(parse.syntax.items.len(), 3);
    assert!(matches!(
        parse.syntax.items[2],
        Item::Stmt(ref stmt) if matches!(
            &**stmt,
            Stmt::Expr {
                expr: Expr::Assign { .. },
                ..
            }
        )
    ));
}

#[test]
fn recovers_missing_statement_semi_fixture() {
    let parse = parse_source(include_str!(
        "../../../tests/corpus/parser/statements/missing-statement-semi-recovery.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected ';' after statement");
    assert_eq!(parse.syntax.items.len(), 2);
    assert!(matches!(
        parse.syntax.items[1],
        Item::Stmt(ref stmt) if matches!(
            &**stmt,
            Stmt::Expr {
                expr: Expr::Assign { .. },
                ..
            }
        )
    ));
}

#[test]
fn allows_trailing_top_level_statement_without_semicolon_in_lenient_mode() {
    let parse = parse_source_with_options(
        include_str!("../../../tests/corpus/parser/statements/trailing-statement-no-semi.mel"),
        ParseOptions {
            mode: ParseMode::AllowTrailingStmtWithoutSemi,
        },
    );
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 1);
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(
            &**stmt,
            Stmt::Expr {
                expr: Expr::Invoke(_),
                ..
            }
        )
    ));
}

#[test]
fn still_requires_semicolon_between_top_level_statements_in_lenient_mode() {
    let parse = parse_source_with_options(
        "$x = 1\n$y = 2;",
        ParseOptions {
            mode: ParseMode::AllowTrailingStmtWithoutSemi,
        },
    );
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected ';' after statement");
}

#[test]
fn still_requires_semicolon_for_nested_statement_in_lenient_mode() {
    let parse = parse_source_with_options(
        "if ($ready) print(\"hello\")",
        ParseOptions {
            mode: ParseMode::AllowTrailingStmtWithoutSemi,
        },
    );
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected ';' after statement");
}

#[test]
fn auto_detects_cp932_and_maps_ranges_to_original_bytes() {
    let source = r#"print "設定";"#;
    let (bytes, _, had_errors) = SHIFT_JIS.encode(source);
    assert!(!had_errors);

    let parse = parse_bytes(bytes.as_ref());
    assert_eq!(parse.source_encoding, SourceEncoding::Cp932);
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => match &words[0] {
                    ShellWord::QuotedString { range, .. } => {
                        assert_eq!(*range, text_range(6, bytes.len() as u32 - 1));
                    }
                    _ => panic!("expected quoted string"),
                },
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn explicit_gbk_decode_preserves_command_surface() {
    let source = r#"print "按钮";"#;
    let (bytes, _, had_errors) = GBK.encode(source);
    assert!(!had_errors);

    let parse = parse_bytes_with_encoding(bytes.as_ref(), SourceEncoding::Gbk);
    assert_eq!(parse.source_encoding, SourceEncoding::Gbk);
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike { words, .. } => match &words[0] {
                    ShellWord::QuotedString { range, .. } => {
                        assert_eq!(*range, text_range(6, bytes.len() as u32 - 1));
                    }
                    _ => panic!("expected quoted string"),
                },
                _ => panic!("expected shell-like invoke"),
            },
            _ => panic!("expected command expression"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn lossy_utf8_single_invalid_byte_keeps_display_ranges_aligned() {
    let parse = parse_bytes(b"print \"A\xffB\";\nprint(;\n");
    assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
    assert_eq!(parse.decode_errors.len(), 1);
    assert_eq!(
        parse.decode_errors[0].message,
        "source is not valid UTF-8; decoded lossily"
    );
    assert_eq!(parse.decode_errors[0].range, text_range(8, 9));

    let decode_span = parse.source_map.display_range(parse.decode_errors[0].range);
    assert_eq!(&parse.source_text[decode_span], "\u{FFFD}");

    assert!(!parse.errors.is_empty());
    let parse_error_span = parse.source_map.display_range(parse.errors[0].range);
    assert_eq!(&parse.source_text[parse_error_span], ";");
}

#[test]
fn lossy_utf8_truncated_sequence_maps_full_invalid_span_to_replacement() {
    let parse = parse_bytes_with_encoding(b"print \"A\xe3\x81\";\nprint(;\n", SourceEncoding::Utf8);
    assert_eq!(parse.source_encoding, SourceEncoding::Utf8);
    assert_eq!(parse.decode_errors.len(), 1);
    assert_eq!(parse.decode_errors[0].range, text_range(8, 10));

    let decode_span = parse.source_map.display_range(parse.decode_errors[0].range);
    assert_eq!(&parse.source_text[decode_span], "\u{FFFD}");

    assert!(!parse.errors.is_empty());
    let parse_error_span = parse.source_map.display_range(parse.errors[0].range);
    assert_eq!(&parse.source_text[parse_error_span], ";");
}

#[test]
fn reports_decimal_integer_literal_overflow() {
    let parse = parse_source("int $value = 9223372036854775808;");
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "integer literal out of range");
    assert_eq!(parse.errors[0].range, text_range(13, 32));

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Int { value, .. }) => assert_eq!(*value, 0),
                _ => panic!("expected integer initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn reports_hex_integer_literal_overflow() {
    let parse = parse_source("int $value = 0x8000000000000000;");
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "integer literal out of range");
    assert_eq!(parse.errors[0].range, text_range(13, 31));

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::Int { value, .. }) => assert_eq!(*value, 0),
                _ => panic!("expected integer initializer"),
            },
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }
}
