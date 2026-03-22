use super::{
    CommandKind, CommandMode, CommandModeMask, CommandRegistry, CommandSchema, CommandSourceKind,
    DiagnosticSeverity, FlagArity, FlagArityByMode, FlagSchema, IdentTarget, ReturnBehavior,
    ValueShape, VariableKind, analyze, analyze_with_registry,
};
use mel_ast::{
    AssignOp, CalleeResolution, Declarator, Expr, InvokeExpr, InvokeSurface, Item, ProcDef,
    ProcParam, ShellWord, SourceFile, Stmt, TypeName, VarDecl,
};
use mel_syntax::text_range;

struct TestRegistry {
    commands: Vec<CommandSchema>,
}

impl CommandRegistry for TestRegistry {
    fn lookup(&self, name: &str) -> Option<CommandSchema> {
        self.commands.iter().find(|info| info.name == name).cloned()
    }
}

fn command_schema(name: &str, kind: CommandKind) -> CommandSchema {
    CommandSchema {
        name: name.to_owned(),
        kind,
        source_kind: CommandSourceKind::Command,
        mode_mask: CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        return_behavior: ReturnBehavior::Unknown,
        flags: Vec::new(),
    }
}

fn uniform_arity(arity: FlagArity) -> FlagArityByMode {
    FlagArityByMode {
        create: arity,
        edit: arity,
        query: arity,
    }
}

fn flag_schema(long_name: &str, short_name: Option<&str>, arity: FlagArity) -> FlagSchema {
    FlagSchema {
        long_name: long_name.to_owned(),
        short_name: short_name.map(str::to_owned),
        mode_mask: CommandModeMask {
            create: true,
            edit: true,
            query: true,
        },
        arity_by_mode: uniform_arity(arity),
        value_shapes: vec![ValueShape::Unknown],
        allows_multiple: false,
    }
}

fn resolved_variable(analysis: &super::Analysis, index: usize) -> Option<&super::VariableSymbol> {
    match analysis.ident_resolutions[index].resolution {
        IdentTarget::Unresolved => None,
        IdentTarget::Variable(symbol_id) => Some(&analysis.variable_symbols[symbol_id.0]),
    }
}

fn warning_messages(analysis: &super::Analysis) -> Vec<&str> {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
        .map(|diagnostic| diagnostic.message.as_str())
        .collect()
}

#[test]
fn function_local_proc_forward_reference_reports_diagnostic() {
    let source = SourceFile {
        items: vec![
            Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::Function {
                        name: "helper".to_owned(),
                        args: Vec::new(),
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 8),
                }),
                range: text_range(0, 9),
            })),
            Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: Vec::new(),
                    range: text_range(17, 19),
                },
                is_global: false,
                range: text_range(10, 19),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].message,
        "local proc \"helper\" is called before its definition"
    );
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Proc("helper".to_owned())
    );
}

#[test]
fn proc_body_traversal_respects_visible_local_proc() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![Stmt::Expr {
                    expr: Expr::Invoke(InvokeExpr {
                        surface: InvokeSurface::Function {
                            name: "helper".to_owned(),
                            args: vec![Expr::Int {
                                value: 1,
                                range: text_range(20, 21),
                            }],
                        },
                        resolution: CalleeResolution::Unresolved,
                        range: text_range(13, 22),
                    }),
                    range: text_range(13, 23),
                }],
                range: text_range(12, 24),
            },
            is_global: false,
            range: text_range(0, 24),
        }))],
    };

    let analysis = analyze(&source);
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Proc("helper".to_owned())
    );
}

#[test]
fn ancestor_scope_local_proc_is_visible_in_nested_block() {
    let source = SourceFile {
        items: vec![
            Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: Vec::new(),
                    range: text_range(12, 14),
                },
                is_global: false,
                range: text_range(0, 14),
            })),
            Item::Stmt(Box::new(Stmt::Block {
                statements: vec![Stmt::Expr {
                    expr: Expr::Invoke(InvokeExpr {
                        surface: InvokeSurface::Function {
                            name: "helper".to_owned(),
                            args: Vec::new(),
                        },
                        resolution: CalleeResolution::Unresolved,
                        range: text_range(17, 25),
                    }),
                    range: text_range(17, 26),
                }],
                range: text_range(15, 27),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Proc("helper".to_owned())
    );
}

#[test]
fn block_local_proc_does_not_leak_to_parent_scope() {
    let source = SourceFile {
        items: vec![
            Item::Stmt(Box::new(Stmt::Block {
                statements: vec![Stmt::Proc {
                    proc_def: Box::new(ProcDef {
                        return_type: None,
                        name: "helper".to_owned(),
                        params: Vec::new(),
                        body: Stmt::Block {
                            statements: Vec::new(),
                            range: text_range(17, 19),
                        },
                        is_global: false,
                        range: text_range(8, 19),
                    }),
                    range: text_range(8, 19),
                }],
                range: text_range(0, 20),
            })),
            Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::Function {
                        name: "helper".to_owned(),
                        args: Vec::new(),
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(21, 29),
                }),
                range: text_range(21, 30),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Unresolved
    );
}

#[test]
fn shell_like_calls_resolve_to_local_proc_without_diagnostic() {
    let source = SourceFile {
        items: vec![
            Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: Vec::new(),
                    range: text_range(9, 11),
                },
                is_global: false,
                range: text_range(0, 11),
            })),
            Item::Stmt(Box::new(Stmt::VarDecl {
                decl: VarDecl {
                    is_global: false,
                    ty: TypeName::String,
                    declarators: vec![Declarator {
                        name: "$selection".to_owned(),
                        array_size: None,
                        initializer: Some(Expr::String {
                            text: "\"pSphere1\"".to_owned(),
                            range: text_range(19, 29),
                        }),
                        range: text_range(12, 29),
                    }],
                    range: text_range(12, 30),
                },
                range: text_range(12, 30),
            })),
            Item::Stmt(Box::new(Stmt::VarDecl {
                decl: VarDecl {
                    is_global: false,
                    ty: TypeName::String,
                    declarators: vec![Declarator {
                        name: "$value".to_owned(),
                        array_size: None,
                        initializer: None,
                        range: text_range(31, 37),
                    }],
                    range: text_range(31, 38),
                },
                range: text_range(31, 38),
            })),
            Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Assign {
                    op: AssignOp::Assign,
                    lhs: Box::new(Expr::Ident {
                        name: "$value".to_owned(),
                        range: text_range(39, 45),
                    }),
                    rhs: Box::new(Expr::Invoke(InvokeExpr {
                        surface: InvokeSurface::ShellLike {
                            head: "helper".to_owned(),
                            words: vec![ShellWord::Variable {
                                expr: Expr::Ident {
                                    name: "$selection".to_owned(),
                                    range: text_range(50, 60),
                                },
                                range: text_range(50, 60),
                            }],
                            captured: true,
                        },
                        resolution: CalleeResolution::Unresolved,
                        range: text_range(46, 61),
                    })),
                    range: text_range(39, 61),
                },
                range: text_range(39, 62),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Proc("helper".to_owned())
    );
}

#[test]
fn shell_like_calls_without_proc_or_registry_remain_unresolved() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "unknown".to_owned(),
                    words: Vec::new(),
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 7),
            }),
            range: text_range(0, 8),
        }))],
    };

    let analysis = analyze(&source);
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Unresolved
    );
}

#[test]
fn shell_like_local_proc_forward_reference_reports_diagnostic() {
    let source = SourceFile {
        items: vec![
            Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "helper".to_owned(),
                        words: vec![ShellWord::NumericLiteral {
                            text: "7".to_owned(),
                            range: text_range(7, 8),
                        }],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 8),
                }),
                range: text_range(0, 9),
            })),
            Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: Vec::new(),
                    range: text_range(17, 19),
                },
                is_global: false,
                range: text_range(10, 19),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].message,
        "local proc \"helper\" is called before its definition"
    );
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Proc("helper".to_owned())
    );
}

#[test]
fn shell_like_global_proc_resolves_without_diagnostic() {
    let source = SourceFile {
        items: vec![
            Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "helper".to_owned(),
                        words: vec![ShellWord::QuotedString {
                            text: "\"value\"".to_owned(),
                            range: text_range(7, 14),
                        }],
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 14),
                }),
                range: text_range(0, 15),
            })),
            Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: Vec::new(),
                    range: text_range(23, 25),
                },
                is_global: true,
                range: text_range(16, 25),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Proc("helper".to_owned())
    );
}

#[test]
fn builtin_command_resolves_with_registry() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::Function {
                    name: "sphere".to_owned(),
                    args: Vec::new(),
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 8),
            }),
            range: text_range(0, 9),
        }))],
    };

    let registry = TestRegistry {
        commands: vec![command_schema("sphere", CommandKind::Builtin)],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::BuiltinCommand("sphere".to_owned())
    );
}

#[test]
fn plugin_command_resolves_with_registry() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "foo".to_owned(),
                    words: Vec::new(),
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 3),
            }),
            range: text_range(0, 4),
        }))],
    };

    let registry = TestRegistry {
        commands: vec![command_schema("foo", CommandKind::Plugin)],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::PluginCommand("foo".to_owned())
    );
}

#[test]
fn proc_resolution_takes_precedence_over_registry_command() {
    let source = SourceFile {
        items: vec![
            Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Invoke(InvokeExpr {
                    surface: InvokeSurface::ShellLike {
                        head: "helper".to_owned(),
                        words: Vec::new(),
                        captured: false,
                    },
                    resolution: CalleeResolution::Unresolved,
                    range: text_range(0, 6),
                }),
                range: text_range(0, 7),
            })),
            Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: Vec::new(),
                    range: text_range(15, 17),
                },
                is_global: true,
                range: text_range(8, 17),
            })),
        ],
    };

    let registry = TestRegistry {
        commands: vec![command_schema("helper", CommandKind::Builtin)],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert!(analysis.diagnostics.is_empty());
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Proc("helper".to_owned())
    );
}

#[test]
fn analyze_without_registry_leaves_builtin_unresolved() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::Function {
                    name: "sphere".to_owned(),
                    args: Vec::new(),
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 8),
            }),
            range: text_range(0, 9),
        }))],
    };

    let analysis = analyze(&source);
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::Unresolved
    );
}

#[test]
fn shell_like_command_normalization_tracks_query_mode_and_invalid_flag_usage() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "frameLayout".to_owned(),
                    words: vec![
                        ShellWord::Flag {
                            text: "-query".to_owned(),
                            range: text_range(12, 18),
                        },
                        ShellWord::Flag {
                            text: "-label".to_owned(),
                            range: text_range(19, 25),
                        },
                        ShellWord::QuotedString {
                            text: "\"title\"".to_owned(),
                            range: text_range(26, 33),
                        },
                    ],
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 33),
            }),
            range: text_range(0, 34),
        }))],
    };

    let mut command = command_schema("frameLayout", CommandKind::Builtin);
    command.mode_mask = CommandModeMask {
        create: true,
        edit: true,
        query: true,
    };
    command.flags = vec![FlagSchema {
        mode_mask: CommandModeMask {
            create: false,
            edit: true,
            query: false,
        },
        value_shapes: vec![ValueShape::String],
        ..flag_schema("label", Some("l"), FlagArity::Exact(1))
    }];
    let registry = TestRegistry {
        commands: vec![command],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert_eq!(
        analysis.invoke_resolutions[0].resolution,
        CalleeResolution::BuiltinCommand("frameLayout".to_owned())
    );
    assert_eq!(analysis.normalized_invokes.len(), 1);
    assert_eq!(analysis.normalized_invokes[0].mode, CommandMode::Query);
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Warning
            && diagnostic.message.contains("not available in query mode")
    }));
}

#[test]
fn shell_like_command_unknown_flag_is_warning() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "frameLayout".to_owned(),
                    words: vec![ShellWord::Flag {
                        text: "-mystery".to_owned(),
                        range: text_range(12, 20),
                    }],
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 20),
            }),
            range: text_range(0, 21),
        }))],
    };

    let registry = TestRegistry {
        commands: vec![command_schema("frameLayout", CommandKind::Builtin)],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Warning
            && diagnostic.message.contains("unknown flag")
    }));
}

#[test]
fn shell_like_command_normalization_reports_mode_conflict() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "frameLayout".to_owned(),
                    words: vec![
                        ShellWord::Flag {
                            text: "-edit".to_owned(),
                            range: text_range(12, 17),
                        },
                        ShellWord::Flag {
                            text: "-query".to_owned(),
                            range: text_range(18, 24),
                        },
                    ],
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 24),
            }),
            range: text_range(0, 25),
        }))],
    };

    let mut command = command_schema("frameLayout", CommandKind::Builtin);
    command.mode_mask = CommandModeMask {
        create: true,
        edit: true,
        query: true,
    };
    let registry = TestRegistry {
        commands: vec![command],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert_eq!(analysis.normalized_invokes[0].mode, CommandMode::Unknown);
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("combine create/edit/query mode flags")
    }));
}

#[test]
fn shell_like_command_query_mode_uses_query_specific_flag_arity() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "frameLayout".to_owned(),
                    words: vec![
                        ShellWord::Flag {
                            text: "-query".to_owned(),
                            range: text_range(12, 18),
                        },
                        ShellWord::Flag {
                            text: "-label".to_owned(),
                            range: text_range(19, 25),
                        },
                        ShellWord::QuotedString {
                            text: "\"title\"".to_owned(),
                            range: text_range(26, 33),
                        },
                    ],
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 33),
            }),
            range: text_range(0, 34),
        }))],
    };

    let mut command = command_schema("frameLayout", CommandKind::Builtin);
    command.flags = vec![FlagSchema {
        arity_by_mode: FlagArityByMode {
            create: FlagArity::Exact(1),
            edit: FlagArity::Exact(1),
            query: FlagArity::None,
        },
        value_shapes: vec![ValueShape::String],
        ..flag_schema("label", Some("l"), FlagArity::Exact(1))
    }];
    let registry = TestRegistry {
        commands: vec![command],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert!(analysis.diagnostics.is_empty());
    let items = &analysis.normalized_invokes[0].items;
    assert!(matches!(
        &items[1],
        super::NormalizedCommandItem::Flag(super::NormalizedFlag {
            source_range,
            canonical_name: Some(name),
            args,
            ..
        }) if *source_range == text_range(19, 25) && name == "label" && args.is_empty()
    ));
    assert!(matches!(
        &items[2],
        super::NormalizedCommandItem::Positional(super::PositionalArg {
            word: ShellWord::QuotedString { text, .. },
            ..
        }) if text == "\"title\""
    ));
}

#[test]
fn shell_like_command_range_arity_allows_optional_second_arg_to_be_omitted() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "frameLayout".to_owned(),
                    words: vec![
                        ShellWord::Flag {
                            text: "-label".to_owned(),
                            range: text_range(12, 18),
                        },
                        ShellWord::QuotedString {
                            text: "\"title\"".to_owned(),
                            range: text_range(19, 26),
                        },
                    ],
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 26),
            }),
            range: text_range(0, 27),
        }))],
    };

    let mut command = command_schema("frameLayout", CommandKind::Builtin);
    command.flags = vec![FlagSchema {
        arity_by_mode: FlagArityByMode {
            create: FlagArity::Range { min: 1, max: 2 },
            edit: FlagArity::Range { min: 1, max: 2 },
            query: FlagArity::None,
        },
        value_shapes: vec![ValueShape::String, ValueShape::String],
        ..flag_schema("label", Some("l"), FlagArity::Exact(1))
    }];
    let registry = TestRegistry {
        commands: vec![command],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert!(analysis.diagnostics.is_empty());
    let items = &analysis.normalized_invokes[0].items;
    assert!(matches!(
        &items[0],
        super::NormalizedCommandItem::Flag(super::NormalizedFlag {
            canonical_name: Some(name),
            args,
            ..
        }) if name == "label" && args.len() == 1
    ));
}

#[test]
fn shell_like_command_range_arity_allows_optional_second_arg_to_be_present() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "frameLayout".to_owned(),
                    words: vec![
                        ShellWord::Flag {
                            text: "-label".to_owned(),
                            range: text_range(12, 18),
                        },
                        ShellWord::QuotedString {
                            text: "\"title\"".to_owned(),
                            range: text_range(19, 26),
                        },
                        ShellWord::QuotedString {
                            text: "\"tooltip\"".to_owned(),
                            range: text_range(27, 36),
                        },
                    ],
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 36),
            }),
            range: text_range(0, 37),
        }))],
    };

    let mut command = command_schema("frameLayout", CommandKind::Builtin);
    command.flags = vec![FlagSchema {
        arity_by_mode: FlagArityByMode {
            create: FlagArity::Range { min: 1, max: 2 },
            edit: FlagArity::Range { min: 1, max: 2 },
            query: FlagArity::None,
        },
        value_shapes: vec![ValueShape::String, ValueShape::String],
        ..flag_schema("label", Some("l"), FlagArity::Exact(1))
    }];
    let registry = TestRegistry {
        commands: vec![command],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert!(analysis.diagnostics.is_empty());
    let items = &analysis.normalized_invokes[0].items;
    assert!(matches!(
        &items[0],
        super::NormalizedCommandItem::Flag(super::NormalizedFlag {
            canonical_name: Some(name),
            args,
            ..
        }) if name == "label" && args.len() == 2
    ));
}

#[test]
fn shell_like_command_range_arity_reports_missing_required_argument() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "frameLayout".to_owned(),
                    words: vec![ShellWord::Flag {
                        text: "-label".to_owned(),
                        range: text_range(12, 18),
                    }],
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 18),
            }),
            range: text_range(0, 19),
        }))],
    };

    let mut command = command_schema("frameLayout", CommandKind::Builtin);
    command.flags = vec![FlagSchema {
        arity_by_mode: FlagArityByMode {
            create: FlagArity::Range { min: 1, max: 2 },
            edit: FlagArity::Range { min: 1, max: 2 },
            query: FlagArity::None,
        },
        value_shapes: vec![ValueShape::String, ValueShape::String],
        ..flag_schema("label", Some("l"), FlagArity::Exact(1))
    }];
    let registry = TestRegistry {
        commands: vec![command],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error
            && diagnostic.message.contains("expects 1 to 2 argument(s)")
    }));
}

#[test]
fn shell_like_command_without_mode_flag_reports_unavailable_create_mode() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Invoke(InvokeExpr {
                surface: InvokeSurface::ShellLike {
                    head: "queryOnly".to_owned(),
                    words: vec![ShellWord::BareWord {
                        text: "node1".to_owned(),
                        range: text_range(10, 15),
                    }],
                    captured: false,
                },
                resolution: CalleeResolution::Unresolved,
                range: text_range(0, 15),
            }),
            range: text_range(0, 16),
        }))],
    };

    let mut command = command_schema("queryOnly", CommandKind::Builtin);
    command.mode_mask = CommandModeMask {
        create: false,
        edit: false,
        query: true,
    };
    let registry = TestRegistry {
        commands: vec![command],
    };

    let analysis = analyze_with_registry(&source, &registry);
    assert_eq!(analysis.normalized_invokes[0].mode, CommandMode::Create);
    assert!(analysis.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("command \"queryOnly\" is not available in create mode")
    }));
}

#[test]
fn proc_params_resolve_inside_proc_body() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: vec![ProcParam {
                ty: TypeName::String,
                name: "$name".to_owned(),
                is_array: false,
                range: text_range(12, 24),
            }],
            body: Stmt::Block {
                statements: vec![Stmt::Expr {
                    expr: Expr::Ident {
                        name: "$name".to_owned(),
                        range: text_range(29, 34),
                    },
                    range: text_range(29, 35),
                }],
                range: text_range(26, 36),
            },
            is_global: false,
            range: text_range(0, 36),
        }))],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.variable_symbols.len(), 1);
    assert_eq!(analysis.variable_symbols[0].kind, VariableKind::Parameter);
    assert_eq!(
        resolved_variable(&analysis, 0).map(|symbol| symbol.name.as_str()),
        Some("$name")
    );
}

#[test]
fn local_variables_become_visible_after_declaration() {
    let source = SourceFile {
        items: vec![
            Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Ident {
                    name: "$value".to_owned(),
                    range: text_range(0, 6),
                },
                range: text_range(0, 7),
            })),
            Item::Stmt(Box::new(Stmt::VarDecl {
                decl: VarDecl {
                    is_global: false,
                    ty: TypeName::Int,
                    declarators: vec![Declarator {
                        name: "$value".to_owned(),
                        array_size: None,
                        initializer: Some(Expr::Int {
                            value: 1,
                            range: text_range(18, 19),
                        }),
                        range: text_range(12, 19),
                    }],
                    range: text_range(8, 20),
                },
                range: text_range(8, 20),
            })),
            Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Ident {
                    name: "$value".to_owned(),
                    range: text_range(21, 27),
                },
                range: text_range(21, 28),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.variable_symbols.len(), 1);
    assert_eq!(analysis.ident_resolutions.len(), 2);
    assert!(resolved_variable(&analysis, 0).is_none());
    assert_eq!(
        resolved_variable(&analysis, 1).map(|symbol| symbol.name.as_str()),
        Some("$value")
    );
}

#[test]
fn local_variables_shadow_globals_inside_proc_scope() {
    let source = SourceFile {
        items: vec![
            Item::Stmt(Box::new(Stmt::VarDecl {
                decl: VarDecl {
                    is_global: true,
                    ty: TypeName::String,
                    declarators: vec![Declarator {
                        name: "$value".to_owned(),
                        array_size: None,
                        initializer: None,
                        range: text_range(0, 20),
                    }],
                    range: text_range(0, 21),
                },
                range: text_range(0, 21),
            })),
            Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$value".to_owned(),
                                    array_size: None,
                                    initializer: Some(Expr::Int {
                                        value: 1,
                                        range: text_range(42, 43),
                                    }),
                                    range: text_range(36, 43),
                                }],
                                range: text_range(32, 44),
                            },
                            range: text_range(32, 44),
                        },
                        Stmt::Expr {
                            expr: Expr::Ident {
                                name: "$value".to_owned(),
                                range: text_range(45, 51),
                            },
                            range: text_range(45, 52),
                        },
                    ],
                    range: text_range(30, 53),
                },
                is_global: false,
                range: text_range(22, 53),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.variable_symbols.len(), 2);
    let resolved = resolved_variable(&analysis, 0).expect("local variable should resolve");
    assert_eq!(resolved.kind, VariableKind::Local);
    assert_eq!(resolved.name, "$value");
}

#[test]
fn block_local_variable_does_not_leak_to_parent_scope() {
    let source = SourceFile {
        items: vec![
            Item::Stmt(Box::new(Stmt::Block {
                statements: vec![
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Int,
                            declarators: vec![Declarator {
                                name: "$value".to_owned(),
                                array_size: None,
                                initializer: Some(Expr::Int {
                                    value: 1,
                                    range: text_range(13, 14),
                                }),
                                range: text_range(7, 14),
                            }],
                            range: text_range(3, 15),
                        },
                        range: text_range(3, 15),
                    },
                    Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(16, 22),
                        },
                        range: text_range(16, 23),
                    },
                ],
                range: text_range(0, 24),
            })),
            Item::Stmt(Box::new(Stmt::Expr {
                expr: Expr::Ident {
                    name: "$value".to_owned(),
                    range: text_range(25, 31),
                },
                range: text_range(25, 32),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert_eq!(
        resolved_variable(&analysis, 0).map(|symbol| symbol.kind),
        Some(VariableKind::Local)
    );
    assert!(resolved_variable(&analysis, 1).is_none());
}

#[test]
fn local_variable_read_before_first_explicit_write_is_warning() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Int,
                            declarators: vec![Declarator {
                                name: "$value".to_owned(),
                                array_size: None,
                                initializer: None,
                                range: text_range(17, 23),
                            }],
                            range: text_range(13, 24),
                        },
                        range: text_range(13, 24),
                    },
                    Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(25, 31),
                        },
                        range: text_range(25, 32),
                    },
                ],
                range: text_range(12, 33),
            },
            is_global: false,
            range: text_range(0, 33),
        }))],
    };

    let analysis = analyze(&source);
    let warnings = warning_messages(&analysis);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0],
        "local variable \"$value\" is read before its first explicit write; MEL would use a default value here"
    );
}

#[test]
fn initialized_local_variable_does_not_warn() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Int,
                            declarators: vec![Declarator {
                                name: "$value".to_owned(),
                                array_size: None,
                                initializer: Some(Expr::Int {
                                    value: 1,
                                    range: text_range(25, 26),
                                }),
                                range: text_range(17, 26),
                            }],
                            range: text_range(13, 27),
                        },
                        range: text_range(13, 27),
                    },
                    Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(28, 34),
                        },
                        range: text_range(28, 35),
                    },
                ],
                range: text_range(12, 36),
            },
            is_global: false,
            range: text_range(0, 36),
        }))],
    };

    let analysis = analyze(&source);
    assert!(warning_messages(&analysis).is_empty());
}

#[test]
fn proc_param_read_does_not_warn() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: vec![ProcParam {
                ty: TypeName::String,
                name: "$name".to_owned(),
                is_array: false,
                range: text_range(12, 24),
            }],
            body: Stmt::Block {
                statements: vec![Stmt::Expr {
                    expr: Expr::Ident {
                        name: "$name".to_owned(),
                        range: text_range(29, 34),
                    },
                    range: text_range(29, 35),
                }],
                range: text_range(26, 36),
            },
            is_global: false,
            range: text_range(0, 36),
        }))],
    };

    let analysis = analyze(&source);
    assert!(warning_messages(&analysis).is_empty());
}

#[test]
fn if_without_else_keeps_maybe_unwritten_state() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Int,
                            declarators: vec![Declarator {
                                name: "$value".to_owned(),
                                array_size: None,
                                initializer: None,
                                range: text_range(17, 23),
                            }],
                            range: text_range(13, 24),
                        },
                        range: text_range(13, 24),
                    },
                    Stmt::If {
                        condition: Expr::Int {
                            value: 1,
                            range: text_range(29, 30),
                        },
                        then_branch: Box::new(Stmt::Block {
                            statements: vec![Stmt::Expr {
                                expr: Expr::Assign {
                                    op: AssignOp::Assign,
                                    lhs: Box::new(Expr::Ident {
                                        name: "$value".to_owned(),
                                        range: text_range(36, 42),
                                    }),
                                    rhs: Box::new(Expr::Int {
                                        value: 1,
                                        range: text_range(45, 46),
                                    }),
                                    range: text_range(36, 46),
                                },
                                range: text_range(36, 47),
                            }],
                            range: text_range(32, 49),
                        }),
                        else_branch: None,
                        range: text_range(25, 49),
                    },
                    Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(50, 56),
                        },
                        range: text_range(50, 57),
                    },
                ],
                range: text_range(12, 58),
            },
            is_global: false,
            range: text_range(0, 58),
        }))],
    };

    let analysis = analyze(&source);
    assert!(warning_messages(&analysis).iter().any(|message| {
            *message
                == "local variable \"$value\" is read before its first explicit write; MEL would use a default value here"
        }));
}

#[test]
fn if_else_assigning_both_branches_does_not_warn() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Int,
                            declarators: vec![Declarator {
                                name: "$value".to_owned(),
                                array_size: None,
                                initializer: None,
                                range: text_range(17, 23),
                            }],
                            range: text_range(13, 24),
                        },
                        range: text_range(13, 24),
                    },
                    Stmt::If {
                        condition: Expr::Int {
                            value: 1,
                            range: text_range(29, 30),
                        },
                        then_branch: Box::new(Stmt::Block {
                            statements: vec![Stmt::Expr {
                                expr: Expr::Assign {
                                    op: AssignOp::Assign,
                                    lhs: Box::new(Expr::Ident {
                                        name: "$value".to_owned(),
                                        range: text_range(36, 42),
                                    }),
                                    rhs: Box::new(Expr::Int {
                                        value: 1,
                                        range: text_range(45, 46),
                                    }),
                                    range: text_range(36, 46),
                                },
                                range: text_range(36, 47),
                            }],
                            range: text_range(32, 49),
                        }),
                        else_branch: Some(Box::new(Stmt::Block {
                            statements: vec![Stmt::Expr {
                                expr: Expr::Assign {
                                    op: AssignOp::Assign,
                                    lhs: Box::new(Expr::Ident {
                                        name: "$value".to_owned(),
                                        range: text_range(57, 63),
                                    }),
                                    rhs: Box::new(Expr::Int {
                                        value: 2,
                                        range: text_range(66, 67),
                                    }),
                                    range: text_range(57, 67),
                                },
                                range: text_range(57, 68),
                            }],
                            range: text_range(53, 70),
                        })),
                        range: text_range(25, 70),
                    },
                    Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(71, 77),
                        },
                        range: text_range(71, 78),
                    },
                ],
                range: text_range(12, 79),
            },
            is_global: false,
            range: text_range(0, 79),
        }))],
    };

    let analysis = analyze(&source);
    assert!(warning_messages(&analysis).is_empty());
}

#[test]
fn while_loop_write_only_does_not_make_post_read_definite() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Int,
                            declarators: vec![Declarator {
                                name: "$value".to_owned(),
                                array_size: None,
                                initializer: None,
                                range: text_range(17, 23),
                            }],
                            range: text_range(13, 24),
                        },
                        range: text_range(13, 24),
                    },
                    Stmt::While {
                        condition: Expr::Int {
                            value: 1,
                            range: text_range(32, 33),
                        },
                        body: Box::new(Stmt::Block {
                            statements: vec![Stmt::Expr {
                                expr: Expr::Assign {
                                    op: AssignOp::Assign,
                                    lhs: Box::new(Expr::Ident {
                                        name: "$value".to_owned(),
                                        range: text_range(39, 45),
                                    }),
                                    rhs: Box::new(Expr::Int {
                                        value: 1,
                                        range: text_range(48, 49),
                                    }),
                                    range: text_range(39, 49),
                                },
                                range: text_range(39, 50),
                            }],
                            range: text_range(35, 52),
                        }),
                        range: text_range(25, 52),
                    },
                    Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(53, 59),
                        },
                        range: text_range(53, 60),
                    },
                ],
                range: text_range(12, 61),
            },
            is_global: false,
            range: text_range(0, 61),
        }))],
    };

    let analysis = analyze(&source);
    assert!(warning_messages(&analysis).iter().any(|message| {
            *message
                == "local variable \"$value\" is read before its first explicit write; MEL would use a default value here"
        }));
}

#[test]
fn do_while_unconditional_write_allows_post_read() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Int,
                            declarators: vec![Declarator {
                                name: "$value".to_owned(),
                                array_size: None,
                                initializer: None,
                                range: text_range(17, 23),
                            }],
                            range: text_range(13, 24),
                        },
                        range: text_range(13, 24),
                    },
                    Stmt::DoWhile {
                        body: Box::new(Stmt::Block {
                            statements: vec![Stmt::Expr {
                                expr: Expr::Assign {
                                    op: AssignOp::Assign,
                                    lhs: Box::new(Expr::Ident {
                                        name: "$value".to_owned(),
                                        range: text_range(31, 37),
                                    }),
                                    rhs: Box::new(Expr::Int {
                                        value: 1,
                                        range: text_range(40, 41),
                                    }),
                                    range: text_range(31, 41),
                                },
                                range: text_range(31, 42),
                            }],
                            range: text_range(27, 44),
                        }),
                        condition: Expr::Int {
                            value: 0,
                            range: text_range(51, 52),
                        },
                        range: text_range(25, 53),
                    },
                    Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$value".to_owned(),
                            range: text_range(54, 60),
                        },
                        range: text_range(54, 61),
                    },
                ],
                range: text_range(12, 62),
            },
            is_global: false,
            range: text_range(0, 62),
        }))],
    };

    let analysis = analyze(&source);
    assert!(warning_messages(&analysis).is_empty());
}

#[test]
fn arrays_and_matrices_are_treated_as_initialized() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Int,
                            declarators: vec![Declarator {
                                name: "$items".to_owned(),
                                array_size: Some(None),
                                initializer: None,
                                range: text_range(17, 24),
                            }],
                            range: text_range(13, 25),
                        },
                        range: text_range(13, 25),
                    },
                    Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$items".to_owned(),
                            range: text_range(26, 32),
                        },
                        range: text_range(26, 33),
                    },
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Matrix,
                            declarators: vec![Declarator {
                                name: "$matrix".to_owned(),
                                array_size: None,
                                initializer: None,
                                range: text_range(34, 41),
                            }],
                            range: text_range(34, 42),
                        },
                        range: text_range(34, 42),
                    },
                    Stmt::Expr {
                        expr: Expr::Ident {
                            name: "$matrix".to_owned(),
                            range: text_range(43, 50),
                        },
                        range: text_range(43, 51),
                    },
                ],
                range: text_range(12, 52),
            },
            is_global: false,
            range: text_range(0, 52),
        }))],
    };

    let analysis = analyze(&source);
    assert!(warning_messages(&analysis).is_empty());
}

#[test]
fn compound_assignment_before_write_is_warning() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![
                    Stmt::VarDecl {
                        decl: VarDecl {
                            is_global: false,
                            ty: TypeName::Int,
                            declarators: vec![Declarator {
                                name: "$value".to_owned(),
                                array_size: None,
                                initializer: None,
                                range: text_range(17, 23),
                            }],
                            range: text_range(13, 24),
                        },
                        range: text_range(13, 24),
                    },
                    Stmt::Expr {
                        expr: Expr::Assign {
                            op: AssignOp::AddAssign,
                            lhs: Box::new(Expr::Ident {
                                name: "$value".to_owned(),
                                range: text_range(25, 31),
                            }),
                            rhs: Box::new(Expr::Int {
                                value: 1,
                                range: text_range(35, 36),
                            }),
                            range: text_range(25, 36),
                        },
                        range: text_range(25, 37),
                    },
                ],
                range: text_range(12, 38),
            },
            is_global: false,
            range: text_range(0, 38),
        }))],
    };

    let analysis = analyze(&source);
    assert!(warning_messages(&analysis).iter().any(|message| {
            *message
                == "local variable \"$value\" is read before its first explicit write; MEL would use a default value here"
        }));
}

#[test]
fn local_shadowing_warnings_cover_parameter_local_and_global() {
    let source = SourceFile {
        items: vec![
            Item::Stmt(Box::new(Stmt::VarDecl {
                decl: VarDecl {
                    is_global: true,
                    ty: TypeName::String,
                    declarators: vec![Declarator {
                        name: "$global".to_owned(),
                        array_size: None,
                        initializer: None,
                        range: text_range(0, 20),
                    }],
                    range: text_range(0, 21),
                },
                range: text_range(0, 21),
            })),
            Item::Proc(Box::new(ProcDef {
                return_type: None,
                name: "helper".to_owned(),
                params: vec![ProcParam {
                    ty: TypeName::String,
                    name: "$param".to_owned(),
                    is_array: false,
                    range: text_range(34, 47),
                }],
                body: Stmt::Block {
                    statements: vec![
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$param".to_owned(),
                                    array_size: None,
                                    initializer: Some(Expr::Int {
                                        value: 1,
                                        range: text_range(59, 60),
                                    }),
                                    range: text_range(51, 60),
                                }],
                                range: text_range(48, 61),
                            },
                            range: text_range(48, 61),
                        },
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$local".to_owned(),
                                    array_size: None,
                                    initializer: Some(Expr::Int {
                                        value: 1,
                                        range: text_range(72, 73),
                                    }),
                                    range: text_range(64, 73),
                                }],
                                range: text_range(62, 74),
                            },
                            range: text_range(62, 74),
                        },
                        Stmt::Block {
                            statements: vec![Stmt::VarDecl {
                                decl: VarDecl {
                                    is_global: false,
                                    ty: TypeName::Int,
                                    declarators: vec![Declarator {
                                        name: "$local".to_owned(),
                                        array_size: None,
                                        initializer: Some(Expr::Int {
                                            value: 2,
                                            range: text_range(87, 88),
                                        }),
                                        range: text_range(79, 88),
                                    }],
                                    range: text_range(75, 89),
                                },
                                range: text_range(75, 89),
                            }],
                            range: text_range(74, 90),
                        },
                        Stmt::VarDecl {
                            decl: VarDecl {
                                is_global: false,
                                ty: TypeName::Int,
                                declarators: vec![Declarator {
                                    name: "$global".to_owned(),
                                    array_size: None,
                                    initializer: Some(Expr::Int {
                                        value: 3,
                                        range: text_range(102, 103),
                                    }),
                                    range: text_range(93, 103),
                                }],
                                range: text_range(91, 104),
                            },
                            range: text_range(91, 104),
                        },
                    ],
                    range: text_range(47, 105),
                },
                is_global: false,
                range: text_range(22, 105),
            })),
        ],
    };

    let analysis = analyze(&source);
    let warnings = warning_messages(&analysis);
    assert!(warnings.contains(&"local variable \"$param\" shadows visible parameter variable"));
    assert!(warnings.contains(&"local variable \"$local\" shadows visible local variable"));
    assert!(warnings.contains(&"local variable \"$global\" shadows visible global variable"));
}

#[test]
fn unresolved_variable_is_reported_as_warning() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::Expr {
            expr: Expr::Ident {
                name: "$missing".to_owned(),
                range: text_range(0, 8),
            },
            range: text_range(0, 9),
        }))],
    };

    let analysis = analyze(&source);
    let warnings = warning_messages(&analysis);
    assert!(warnings.contains(&"unresolved variable \"$missing\""));
    assert!(resolved_variable(&analysis, 0).is_none());
}

#[test]
fn unresolved_variable_plain_assignment_target_is_not_reported() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![Stmt::Expr {
                    expr: Expr::Assign {
                        op: AssignOp::Assign,
                        lhs: Box::new(Expr::Ident {
                            name: "$missing".to_owned(),
                            range: text_range(13, 21),
                        }),
                        rhs: Box::new(Expr::Int {
                            value: 1,
                            range: text_range(24, 25),
                        }),
                        range: text_range(13, 25),
                    },
                    range: text_range(13, 26),
                }],
                range: text_range(12, 27),
            },
            is_global: false,
            range: text_range(0, 27),
        }))],
    };

    let analysis = analyze(&source);
    let unresolved_count = warning_messages(&analysis)
        .into_iter()
        .filter(|message| *message == "unresolved variable \"$missing\"")
        .count();
    assert_eq!(unresolved_count, 0);
    assert!(resolved_variable(&analysis, 0).is_none());
}

#[test]
fn unresolved_variable_compound_assignment_target_is_reported() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![Stmt::Expr {
                    expr: Expr::Assign {
                        op: AssignOp::AddAssign,
                        lhs: Box::new(Expr::Ident {
                            name: "$missing".to_owned(),
                            range: text_range(13, 21),
                        }),
                        rhs: Box::new(Expr::Int {
                            value: 1,
                            range: text_range(25, 26),
                        }),
                        range: text_range(13, 26),
                    },
                    range: text_range(13, 27),
                }],
                range: text_range(12, 28),
            },
            is_global: false,
            range: text_range(0, 28),
        }))],
    };

    let analysis = analyze(&source);
    let unresolved_count = warning_messages(&analysis)
        .into_iter()
        .filter(|message| *message == "unresolved variable \"$missing\"")
        .count();
    assert_eq!(unresolved_count, 1);
}

#[test]
fn void_proc_returning_value_reports_diagnostic() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: None,
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![Stmt::Return {
                    expr: Some(Expr::Int {
                        value: 1,
                        range: text_range(16, 17),
                    }),
                    range: text_range(9, 18),
                }],
                range: text_range(8, 19),
            },
            is_global: false,
            range: text_range(0, 19),
        }))],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].message,
        "proc \"helper\" has no return type but returns a value"
    );
}

#[test]
fn typed_proc_without_value_return_reports_diagnostic() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: Some(mel_ast::ProcReturnType {
                ty: TypeName::Int,
                is_array: false,
                range: text_range(5, 8),
            }),
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![Stmt::Return {
                    expr: None,
                    range: text_range(16, 23),
                }],
                range: text_range(15, 24),
            },
            is_global: false,
            range: text_range(0, 24),
        }))],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].message,
        "proc \"helper\" declares a return type but never returns a value"
    );
}

#[test]
fn typed_proc_value_return_in_nested_proc_does_not_satisfy_outer_proc() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: Some(mel_ast::ProcReturnType {
                ty: TypeName::Int,
                is_array: false,
                range: text_range(5, 8),
            }),
            name: "outer".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![Stmt::Proc {
                    proc_def: Box::new(ProcDef {
                        return_type: Some(mel_ast::ProcReturnType {
                            ty: TypeName::Int,
                            is_array: false,
                            range: text_range(24, 27),
                        }),
                        name: "inner".to_owned(),
                        params: Vec::new(),
                        body: Stmt::Block {
                            statements: vec![Stmt::Return {
                                expr: Some(Expr::Int {
                                    value: 1,
                                    range: text_range(42, 43),
                                }),
                                range: text_range(35, 44),
                            }],
                            range: text_range(34, 45),
                        },
                        is_global: false,
                        range: text_range(19, 45),
                    }),
                    range: text_range(19, 45),
                }],
                range: text_range(14, 46),
            },
            is_global: false,
            range: text_range(0, 46),
        }))],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].message,
        "proc \"outer\" declares a return type but never returns a value"
    );
}

#[test]
fn typed_proc_return_type_mismatch_reports_diagnostic() {
    let source = SourceFile {
        items: vec![Item::Proc(Box::new(ProcDef {
            return_type: Some(mel_ast::ProcReturnType {
                ty: TypeName::Int,
                is_array: false,
                range: text_range(5, 8),
            }),
            name: "helper".to_owned(),
            params: Vec::new(),
            body: Stmt::Block {
                statements: vec![Stmt::Return {
                    expr: Some(Expr::String {
                        text: "\"bad\"".to_owned(),
                        range: text_range(16, 21),
                    }),
                    range: text_range(9, 22),
                }],
                range: text_range(8, 23),
            },
            is_global: false,
            range: text_range(0, 23),
        }))],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].message,
        "proc \"helper\" returns String but declares Int"
    );
}

#[test]
fn var_initializer_type_mismatch_reports_diagnostic() {
    let source = SourceFile {
        items: vec![Item::Stmt(Box::new(Stmt::VarDecl {
            decl: VarDecl {
                is_global: false,
                ty: TypeName::String,
                declarators: vec![Declarator {
                    name: "$value".to_owned(),
                    array_size: None,
                    initializer: Some(Expr::Int {
                        value: 1,
                        range: text_range(16, 17),
                    }),
                    range: text_range(8, 17),
                }],
                range: text_range(0, 18),
            },
            range: text_range(0, 18),
        }))],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].message,
        "variable \"$value\" has declared type String but initializer is Int"
    );
}

#[test]
fn proc_invoke_return_type_flows_into_initializer_check() {
    let source = SourceFile {
        items: vec![
            Item::Proc(Box::new(ProcDef {
                return_type: Some(mel_ast::ProcReturnType {
                    ty: TypeName::String,
                    is_array: false,
                    range: text_range(5, 11),
                }),
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Return {
                        expr: Some(Expr::String {
                            text: "\"bad\"".to_owned(),
                            range: text_range(24, 29),
                        }),
                        range: text_range(17, 30),
                    }],
                    range: text_range(16, 31),
                },
                is_global: false,
                range: text_range(0, 31),
            })),
            Item::Stmt(Box::new(Stmt::VarDecl {
                decl: VarDecl {
                    is_global: false,
                    ty: TypeName::Int,
                    declarators: vec![Declarator {
                        name: "$value".to_owned(),
                        array_size: None,
                        initializer: Some(Expr::Invoke(InvokeExpr {
                            surface: InvokeSurface::Function {
                                name: "helper".to_owned(),
                                args: Vec::new(),
                            },
                            resolution: CalleeResolution::Unresolved,
                            range: text_range(44, 52),
                        })),
                        range: text_range(36, 52),
                    }],
                    range: text_range(32, 53),
                },
                range: text_range(32, 53),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].message,
        "variable \"$value\" has declared type Int but initializer is String"
    );
}

#[test]
fn proc_invoke_return_type_flows_into_return_check() {
    let source = SourceFile {
        items: vec![
            Item::Proc(Box::new(ProcDef {
                return_type: Some(mel_ast::ProcReturnType {
                    ty: TypeName::String,
                    is_array: false,
                    range: text_range(5, 11),
                }),
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Return {
                        expr: Some(Expr::String {
                            text: "\"bad\"".to_owned(),
                            range: text_range(24, 29),
                        }),
                        range: text_range(17, 30),
                    }],
                    range: text_range(16, 31),
                },
                is_global: false,
                range: text_range(0, 31),
            })),
            Item::Proc(Box::new(ProcDef {
                return_type: Some(mel_ast::ProcReturnType {
                    ty: TypeName::Int,
                    is_array: false,
                    range: text_range(37, 40),
                }),
                name: "outer".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Return {
                        expr: Some(Expr::Invoke(InvokeExpr {
                            surface: InvokeSurface::Function {
                                name: "helper".to_owned(),
                                args: Vec::new(),
                            },
                            resolution: CalleeResolution::Unresolved,
                            range: text_range(56, 64),
                        })),
                        range: text_range(49, 65),
                    }],
                    range: text_range(48, 66),
                },
                is_global: false,
                range: text_range(32, 66),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(
        analysis.diagnostics[0].message,
        "proc \"outer\" returns String but declares Int"
    );
}

#[test]
fn proc_invoke_return_type_allows_matching_initializer() {
    let source = SourceFile {
        items: vec![
            Item::Proc(Box::new(ProcDef {
                return_type: Some(mel_ast::ProcReturnType {
                    ty: TypeName::Int,
                    is_array: false,
                    range: text_range(5, 8),
                }),
                name: "helper".to_owned(),
                params: Vec::new(),
                body: Stmt::Block {
                    statements: vec![Stmt::Return {
                        expr: Some(Expr::Int {
                            value: 1,
                            range: text_range(21, 22),
                        }),
                        range: text_range(14, 23),
                    }],
                    range: text_range(13, 24),
                },
                is_global: false,
                range: text_range(0, 24),
            })),
            Item::Stmt(Box::new(Stmt::VarDecl {
                decl: VarDecl {
                    is_global: false,
                    ty: TypeName::Int,
                    declarators: vec![Declarator {
                        name: "$value".to_owned(),
                        array_size: None,
                        initializer: Some(Expr::Invoke(InvokeExpr {
                            surface: InvokeSurface::Function {
                                name: "helper".to_owned(),
                                args: Vec::new(),
                            },
                            resolution: CalleeResolution::Unresolved,
                            range: text_range(37, 45),
                        })),
                        range: text_range(29, 45),
                    }],
                    range: text_range(25, 46),
                },
                range: text_range(25, 46),
            })),
        ],
    };

    let analysis = analyze(&source);
    assert!(analysis.diagnostics.is_empty());
}
