use super::*;

#[test]
fn parses_ternary_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/ternary-basic.mel"
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
        "../../../../tests/corpus/parser/expressions/exponent-float-basic.mel"
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
                Expr::Float { ref text, .. } if parse.source_slice(*text) == "1.0e-3"
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
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "1e+3"
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "0.0e0"
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
        "../../../../tests/corpus/parser/expressions/trailing-dot-float-basic.mel"
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
                Expr::Float { ref text, .. } if parse.source_slice(*text) == "1000."
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
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "0."
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "1."
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
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "0."
                    ));
                    assert!(matches!(
                        elements[1],
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "1."
                    ));
                    assert!(matches!(
                        elements[2],
                        Expr::Float { ref text, .. } if parse.source_slice(*text) == "2."
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
        "../../../../tests/corpus/parser/statements/while-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::While { .. })
    ));

    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/for-loop-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::For { .. })
    ));

    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/for-loop-multi-init-update.mel"
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
        "../../../../tests/corpus/parser/statements/for-in-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::ForIn { .. })
    ));
}

#[test]
fn parses_for_in_and_for_with_shared_prefix_tokens() {
    let parse = parse_source(
        "for ($item in some_array) {\n    print $item;\n}\nfor ($i = 0; $i < 3; ++$i) {\n    print $i;\n}\n",
    );
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::ForIn { binding, .. } => match &**binding {
                Expr::Ident {
                    name_range: _,
                    range: _,
                } => {}
                _ => panic!("expected variable binding"),
            },
            _ => panic!("expected for-in statement"),
        },
        _ => panic!("expected statement"),
    }

    match &parse.syntax.items[1] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::For {
                init,
                condition,
                update,
                ..
            } => {
                assert!(init.is_some());
                assert!(condition.is_some());
                assert!(update.is_some());
            }
            _ => panic!("expected classic for statement"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn parses_if_else_and_break_continue_fixtures() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/if-else-command.mel"
    ));
    assert!(parse.errors.is_empty());
    assert!(matches!(
        parse.syntax.items[0],
        Item::Stmt(ref stmt) if matches!(&**stmt, Stmt::If { .. })
    ));

    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/break-continue.mel"
    ));
    assert!(parse.errors.is_empty());
}

#[test]
fn parses_switch_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/switch-basic.mel"
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
        "../../../../tests/corpus/parser/statements/postfix-update.mel"
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
        "../../../../tests/corpus/parser/expressions/compound-assign-basic.mel"
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
        "../../../../tests/corpus/parser/expressions/prefix-update-basic.mel"
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
        "../../../../tests/corpus/parser/statements/do-while-basic.mel"
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
        "../../../../tests/corpus/parser/statements/var-decl-basic.mel"
    ));
    assert!(parse.errors.is_empty());
    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => {
                assert!(matches!(decl.ty, TypeName::Int));
                assert_eq!(decl.declarators.len(), 1);
                assert_eq!(parse.source_slice(decl.declarators[0].name_range), "$count");
            }
            _ => panic!("expected variable declaration"),
        },
        _ => panic!("expected statement"),
    }

    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/statements/global-var-decl.mel"
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
        "../../../../tests/corpus/parser/statements/var-decl-multi-array.mel"
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
        "../../../../tests/corpus/parser/expressions/brace-list-assign.mel"
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
        "../../../../tests/corpus/parser/expressions/cast-basic.mel"
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
        "../../../../tests/corpus/parser/expressions/path-like-bareword-basic.mel"
    ));
    assert!(parse.errors.is_empty());

    match &parse.syntax.items[0] {
        Item::Stmt(stmt) => match &**stmt {
            Stmt::VarDecl { decl, .. } => match decl.declarators[0].initializer.as_ref() {
                Some(Expr::BareWord { text, .. }) => {
                    assert_eq!(parse.source_slice(*text), "AA_Bar*|mdl|_XXa0|");
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
        "../../../../tests/corpus/parser/expressions/hex-int-basic.mel"
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
        "../../../../tests/corpus/parser/expressions/caret-operator-basic.mel"
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
        "../../../../tests/corpus/parser/expressions/vector-literal-two-elements.mel"
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
        "../../../../tests/corpus/parser/expressions/unary-negate-basic.mel"
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
        "../../../../tests/corpus/parser/expressions/vector-literal-basic.mel"
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
        "../../../../tests/corpus/parser/expressions/vector-component-basic.mel"
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
        "../../../../tests/corpus/parser/expressions/member-access-basic.mel"
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
                        Expr::MemberAccess { ref member, .. } if parse.source_slice(*member) == "foo"
                    ));
                    assert!(matches!(
                        **rhs,
                        Expr::MemberAccess { ref member, .. } if parse.source_slice(*member) == "bar"
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
                Expr::MemberAccess { ref member, .. } if parse.source_slice(*member) == "name"
            )),
            _ => panic!("expected indexed member access"),
        },
        _ => panic!("expected statement"),
    }
}

#[test]
fn reports_missing_index_bracket_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/expressions/missing-index-bracket.mel"
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
        "../../../../tests/corpus/parser/proc/missing-proc-body.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(parse.errors[0].message, "expected proc body block");
}

#[test]
fn reports_missing_proc_param_name_fixture() {
    let parse = parse_source(include_str!(
        "../../../../tests/corpus/parser/proc/missing-proc-param-name.mel"
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
        "../../../../tests/corpus/parser/expressions/missing-compound-assign-rhs.mel"
    ));
    assert!(!parse.errors.is_empty());
    assert_eq!(
        parse.errors[0].message,
        "expected expression after operator"
    );
}

#[test]
fn parses_inline_path_like_bareword_expression_with_trailing_pipe() {
    let parse = parse_source("string $name = AA_Bar*|mdl|_XXa0|;");
    assert!(parse.errors.is_empty());

    let Item::Stmt(stmt) = &parse.syntax.items[0] else {
        panic!("expected statement");
    };
    let Stmt::VarDecl { decl, .. } = &**stmt else {
        panic!("expected variable declaration");
    };
    let Some(Expr::BareWord { text, .. }) = decl.declarators[0].initializer.as_ref() else {
        panic!("expected bareword initializer");
    };
    assert_eq!(parse.source_slice(*text), "AA_Bar*|mdl|_XXa0|");
}
