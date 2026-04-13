#![forbid(unsafe_code)]
#![deny(rustdoc::bare_urls, rustdoc::broken_intra_doc_links)]
//! Parse and analyze Autodesk Maya MEL from a single crate.
//!
//! `maya-mel` is the public library entry point for this workspace. It keeps
//! the common MEL workflow available from one dependency while leaving
//! lower-level syntax, parsing, and Maya-specific layers available as public
//! modules when you need tighter control.
//!
//! # Quick Start
//!
//! ```rust
//! use maya_mel::{analyze, collect_top_level_facts, parse_source};
//!
//! let parsed = parse_source("global proc hello() {}");
//! let analysis = analyze(&parsed.syntax, parsed.source_view());
//! let facts = collect_top_level_facts(&parsed);
//!
//! assert!(analysis.diagnostics.is_empty());
//! assert!(!facts.items.is_empty());
//! ```
//!
//! ```rust
//! use maya_mel::{MayaCommandRegistry, collect_top_level_facts_with_registry, parse_source};
//!
//! let parsed = parse_source("createNode transform -n \"root\";");
//! let facts = collect_top_level_facts_with_registry(&parsed, &MayaCommandRegistry::new());
//!
//! assert_eq!(facts.items.len(), 1);
//! ```
//!
//! # Common Workflows
//!
//! - Use [`parse_source`] or [`parse_file`] to build a typed MEL syntax tree.
//! - Use [`analyze`] to resolve generic MEL semantics and collect diagnostics.
//! - Use [`collect_top_level_facts`] to gather Maya-specific command facts.
//! - Use [`MayaCommandRegistry`] with [`analyze_with_registry`] or
//!   [`collect_top_level_facts_with_registry`] when builtin Maya command metadata matters.
//! - Use [`parser`], [`sema`], or [`maya`] directly for advanced workflows.
//!
//! # Module Guide
//!
//! - [`parser`] exposes full and lightweight parse entry points.
//! - [`sema`] exposes generic semantic analysis at the module root, with
//!   advanced command contracts under [`sema::command_schema`] and command
//!   normalization data under [`sema::command_norm`].
//! - [`maya`] exposes Maya-specific fact collection at the module root, with
//!   detailed fact model types under [`maya::model`].
//! - [`ast`], [`syntax`], and [`mod@lexer`] expose lower-level structures.
//!
//! There is intentionally no crate prelude. The crate root stays small and
//! covers the common parse/analyze/fact-collection workflow, while explicit
//! module paths carry the advanced surfaces.
//!
//! # Stability
//!
//! This crate is published as experimental `0.x`. Root-level APIs are intended
//! to cover the common workflow, while advanced surfaces may continue to move
//! between releases.

extern crate self as mel_ast;
extern crate self as mel_lexer;
extern crate self as mel_maya;
extern crate self as mel_parser;
extern crate self as mel_sema;
extern crate self as mel_syntax;

/// Typed MEL syntax tree structures returned by the parser.
pub mod ast;
/// MEL tokenization utilities and lexer entry points.
pub mod lexer;
/// Maya-specific metadata, registries, and top-level fact collection.
pub mod maya;
/// Full and lightweight MEL parsing entry points.
pub mod parser;
/// Generic semantic analysis and command normalization.
pub mod sema;
/// Shared spans, tokens, and source mapping primitives.
pub mod syntax;

pub(crate) use maya::{model, normalize, registry, specialize, validate};
pub(crate) use parser::decode;
pub(crate) use sema::{command_norm, command_schema, scope};

pub(crate) use ast::*;
pub(crate) use lexer::*;
pub(crate) use parser::*;
pub(crate) use sema::{command_norm::*, command_schema::*, *};
pub(crate) use syntax::*;

#[doc(inline)]
pub use maya::model::MayaTopLevelFacts;
#[doc(inline)]
pub use maya::{
    MayaCommandRegistry, collect_top_level_facts, collect_top_level_facts_shared,
    collect_top_level_facts_shared_with_registry, collect_top_level_facts_with_registry,
};
#[doc(inline)]
pub use parser::{
    DecodeDiagnostic, Parse, ParseBudgets, ParseError, ParseMode, ParseOptions, SourceEncoding,
    parse_bytes, parse_bytes_with_encoding, parse_file, parse_file_with_encoding,
    parse_file_with_encoding_and_options, parse_file_with_options, parse_source,
    parse_source_with_options,
};
#[doc(inline)]
pub use sema::{
    Analysis, Diagnostic, DiagnosticFilter, DiagnosticLabel, DiagnosticSeverity, analyze,
    analyze_diagnostics_with_registry, analyze_diagnostics_with_registry_filtered,
    analyze_with_registry,
};
