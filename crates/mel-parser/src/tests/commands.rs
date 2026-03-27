use super::*;

#[test]
fn parses_command_bareword_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/command-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "print");
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
        "../../../../tests/corpus/parser/statements/command-dotdot-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "setParent");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == ".."
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
        "../../../../tests/corpus/parser/statements/command-dotdot-flag-arg.mel"
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
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == ".."
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
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "setParent");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == ".."
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
                        ShellWord::QuotedString { ref text, .. } if parse.source_slice(*text) == "\"..\""
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
                InvokeSurface::Function {
                    head_range, args, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "doItDRA");
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
fn parses_function_stmt_across_line_break_and_following_keyword() {
    let parse = parse_source("foo(\n    \"arg\"\n);\nif ($ready) {\n    print $ready;\n}\n");
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::Function {
                    head_range, args, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "foo");
                    assert_eq!(args.len(), 1);
                }
                _ => panic!("expected function invoke"),
            },
            _ => panic!("expected expression statement"),
        },
        _ => panic!("expected statement"),
    }

    assert!(matches!(
        parse.syntax.items[1],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::If { .. })
    ));
}

#[test]
fn parses_function_stmt_spaced_lparen_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/function-stmt-spaced-lparen.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::Function {
                    head_range, args, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "tmBuildSet");
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
        "../../../../tests/corpus/parser/statements/command-leading-grouped-arg.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "renameAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
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
        "../../../../tests/corpus/parser/statements/command-numeric-arg.mel"
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
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "0"
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
        "../../../../tests/corpus/parser/statements/command-signed-numeric-arg.mel"
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
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "-10"
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
        "../../../../tests/corpus/parser/statements/command-leading-dot-float.mel"
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
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == ".7"
                    ));
                    assert!(matches!(words[2], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[3],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == ".001"
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
        "../../../../tests/corpus/parser/statements/command-trailing-dot-float.mel"
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
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "-1000."
                    ));
                    assert!(matches!(words[3], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[4],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "1000."
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
        "../../../../tests/corpus/parser/expressions/grouped-subtraction-call.mel"
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
        "../../../../tests/corpus/parser/statements/command-spaced-flag.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::ShellLike {
                        head_range, words, ..
                    } => {
                        assert_eq!(parse.source_slice(*head_range), "optionVar");
                        assert!(matches!(
                            words[0],
                            ShellWord::Flag { ref text, .. } if parse.source_slice(*text) == "- q"
                        ));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "LayoutPreviewResolution"
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
        "../../../../tests/corpus/parser/statements/command-multiline-grouped-args.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "connectAttr");
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
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "setAttr");
                    assert!(matches!(words[0], ShellWord::GroupedExpr { .. }));
                    assert!(
                        matches!(words[1], ShellWord::Flag { ref text, .. } if parse.source_slice(*text) == "-type")
                    );
                    assert!(matches!(
                        words[2],
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == "double3"
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
        "../../../../tests/corpus/parser/statements/command-point-constraint-brace-list.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "applyPointConstraintArgs");
                    assert!(matches!(
                        words[0],
                        ShellWord::NumericLiteral { ref text, .. } if parse.source_slice(*text) == "2"
                    ));
                    assert!(matches!(
                        words[1],
                        ShellWord::BraceList {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::ArrayLiteral { elements, .. } if elements.len() == 10)
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
        "../../../../tests/corpus/parser/statements/command-orient-constraint-brace-list.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "applyOrientConstraintArgs");
                    match &words[1] {
                        ShellWord::BraceList { expr, .. } => match &**expr {
                            Expr::ArrayLiteral { elements, .. } => {
                                assert!(matches!(
                                    elements[0],
                                    Expr::String { ref text, .. } if parse.source_slice(*text) == "\"1\""
                                ));
                                assert!(matches!(
                                    elements[7],
                                    Expr::String { ref text, .. } if parse.source_slice(*text) == "\"8\""
                                ));
                                assert!(matches!(
                                    elements[8],
                                    Expr::String { ref text, .. } if parse.source_slice(*text) == "\"\""
                                ));
                            }
                            _ => panic!("expected array literal"),
                        },
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
        "../../../../tests/corpus/parser/statements/command-capture-vector-literal.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::ShellLike {
                        head_range,
                        words,
                        captured,
                        ..
                    } => {
                        assert_eq!(parse.source_slice(*head_range), "hsv_to_rgb");
                        assert!(*captured);
                        assert!(matches!(
                            words[0],
                            ShellWord::VectorLiteral {
                                ref expr,
                                ..
                            } if matches!(&**expr, Expr::VectorLiteral { elements, .. } if elements.len() == 3)
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
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "text");
                    assert!(matches!(
                        words[2],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::ComponentAccess { .. })
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
        "../../../../tests/corpus/parser/statements/command-dotted-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "setDrivenKeyframe");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "N_arm_01.rotateX"
                    ));
                    assert!(matches!(
                        words[2],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "N_arm_01_H.rotateX"
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
        "../../../../tests/corpus/parser/statements/command-dotted-indexed-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "connectAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "foo.worldMatrix[0]"
                    ));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "bar.inputWorldMatrix"
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
        "../../../../tests/corpus/parser/statements/command-dotted-variable-indexed-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "connectAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
                    ));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "LayerRegistry.layerSlot[$index]"
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
        "../../../../tests/corpus/parser/statements/command-dotted-global-attr.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "getAttr");
                    assert!(matches!(
                        words[0],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "defaultRenderGlobals.hyperShadeBinList"
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
        "../../../../tests/corpus/parser/statements/command-pipe-dag-path.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "select");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "Null|Spine_00|Tail_00"
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
        "../../../../tests/corpus/parser/statements/command-pipe-wildcard-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "select");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == "*|_x005"
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
        "../../../../tests/corpus/parser/statements/command-absolute-plug-path.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "defaultNavigation");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(
                        matches!(words[1], ShellWord::BareWord { ref text, .. } if parse.source_slice(*text) == "shaderNodePreview1")
                    );
                    assert!(matches!(words[2], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[3],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "|geoPreview1|geoPreviewShape1.instObjGroups[0]"
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
        "../../../../tests/corpus/parser/statements/command-namespace-pipe-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "select");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::BareWord { ref text, .. }
                            if parse.source_slice(*text) == "ns:root|ns:spine|ns:ctrl"
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
        "../../../../tests/corpus/parser/statements/command-leading-colon-bareword.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Invoke(invoke)) => match &invoke.surface {
                    InvokeSurface::ShellLike {
                        head_range, words, ..
                    } => {
                        assert_eq!(parse.source_slice(*head_range), "camera");
                        assert!(matches!(words[0], ShellWord::Flag { .. }));
                        assert!(matches!(
                            words[1],
                            ShellWord::BareWord { ref text, .. }
                                if parse.source_slice(*text) == ":previewViewportCamera"
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
        "../../../../tests/corpus/parser/statements/command-grouped-args.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::Expr {
                expr: Expr::Invoke(invoke),
                ..
            } => match &invoke.surface {
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "iconTextButton");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(words[1], ShellWord::QuotedString { .. }));
                    assert!(matches!(
                        words[5],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
                    ));
                    assert!(matches!(
                        words[7],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
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
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "menuItem");
                    assert!(matches!(
                        words[1],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Binary { .. })
                    ));
                    assert!(matches!(
                        words[3],
                        ShellWord::Variable {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::MemberAccess { member, .. } if parse.source_slice(*member) == "name")
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
        "../../../../tests/corpus/parser/statements/command-capture-grouped-function-call.mel"
    ));
    assert!(parse.errors.is_empty());
    assert_eq!(parse.syntax.items.len(), 2);

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match &decl.declarators[0].initializer {
                Some(Expr::Unary { expr, .. }) => match &**expr {
                    Expr::Invoke(invoke) => match &invoke.surface {
                        InvokeSurface::ShellLike {
                            head_range,
                            words,
                            captured,
                            ..
                        } => {
                            assert_eq!(parse.source_slice(*head_range), "optionVar");
                            assert!(*captured);
                            assert!(matches!(words[0], ShellWord::Flag { .. }));
                            assert!(matches!(
                                words[1],
                                ShellWord::GroupedExpr {
                                    ref expr,
                                    ..
                                } if matches!(&**expr, Expr::Invoke(_))
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
                InvokeSurface::ShellLike {
                    head_range, words, ..
                } => {
                    assert_eq!(parse.source_slice(*head_range), "optionVar");
                    assert!(matches!(words[0], ShellWord::Flag { .. }));
                    assert!(matches!(
                        words[1],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Invoke(_))
                    ));
                    assert!(matches!(
                        words[2],
                        ShellWord::GroupedExpr {
                            ref expr,
                            ..
                        } if matches!(&**expr, Expr::Invoke(_))
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
fn parses_shell_variable_member_index_chain_inline() {
    let parse = parse_source("setAttr $node.attr[$index] 1;");
    assert!(parse.errors.is_empty());

    let Item::Stmt(stmt) = &parse.syntax.items[0] else {
        panic!("expected command statement");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke expression");
    };
    let InvokeSurface::ShellLike { words, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    let ShellWord::Variable { expr, .. } = &words[0] else {
        panic!("expected variable shell word");
    };

    match &**expr {
        Expr::Index { target, index, .. } => {
            assert!(matches!(
                &**target,
                Expr::MemberAccess { member, .. } if parse.source_slice(*member) == "attr"
            ));
            assert!(matches!(
                &**index,
                Expr::Ident { name_range, .. } if parse.source_slice(*name_range) == "$index"
            ));
        }
        _ => panic!("expected indexed member access"),
    }
}

#[test]
fn parses_shell_path_like_bareword_with_namespace_member_and_index() {
    let parse = parse_source("select ns:node.attr[3];");
    assert!(parse.errors.is_empty());

    let Item::Stmt(stmt) = &parse.syntax.items[0] else {
        panic!("expected command statement");
    };
    let Stmt::Expr {
        expr: Expr::Invoke(invoke),
        ..
    } = &**stmt
    else {
        panic!("expected invoke expression");
    };
    let InvokeSurface::ShellLike { words, .. } = &invoke.surface else {
        panic!("expected shell-like invoke");
    };
    let ShellWord::BareWord { text, .. } = &words[0] else {
        panic!("expected bareword shell word");
    };

    assert_eq!(parse.source_slice(*text), "ns:node.attr[3]");
}
