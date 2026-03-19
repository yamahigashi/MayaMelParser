# MayaMelParser

`MayaMelParser` is a Rust workspace for parsing and analyzing Autodesk Maya MEL.

The project is aimed at building a solid foundation for MEL tooling rather than
just accepting source text. The intended pipeline separates syntax handling from
later semantic resolution so the workspace can support diagnostics, recovery,
corpus-based regression testing, and future editor-facing tooling.

## Current Status

This repository is still early-stage and the internal APIs are not stable.

Today the workspace already includes:

- shared syntax primitives and span types
- a lexer with trivia retention and lexical diagnostics
- a parser for core MEL statement and expression surfaces
- typed AST structures
- an initial semantic pass for proc visibility diagnostics
- a small local CLI for inspecting parse and diagnostic output

The implementation is under active development, and crate boundaries are being
treated as part of the long-term architecture.

## Architecture

The workspace is organized around this pipeline:

```text
source
  -> mel-syntax
  -> mel-lexer
  -> mel-cst
  -> mel-ast
  -> mel-parser
  -> mel-sema
  -> mel-cli
```

The main design rule is that parsing preserves MEL surface structure and does
not try to fully resolve meaning too early. In particular:

- parser output keeps command-style and function-style invocation surfaces
- command, proc, and plugin-command resolution belongs to semantic analysis
- spans are carried through syntax and diagnostics
- error recovery is treated as a first-class parser concern

## Workspace Layout

- `crates/mel-syntax`: shared span, token, and syntax primitives
- `crates/mel-lexer`: tokenization and lexical diagnostics
- `crates/mel-cst`: lossless concrete syntax layer scaffold
- `crates/mel-ast`: typed AST shapes used by parser and sema
- `crates/mel-parser`: parsing entry points, recovery, and source decoding
- `crates/mel-sema`: early semantic analysis and diagnostics
- `crates/mel-cli`: local inspection CLI for parser and sema output
- `tests/corpus`: public MEL fixtures and future snapshot-oriented tests
- `tests/private-corpus`: local-only regression inputs used during development
- `examples`: small sample MEL sources

## MEL-Specific Design Principles

MEL looks close to C-family syntax in places, but several language features make
it awkward to model with a conventional parser-only design:

- command syntax and function syntax coexist
- command results can be captured with backquotes
- command flags have command-specific meaning and arity
- proc resolution depends on semantic context rather than parse shape alone

Because of that, this project prefers surface-preserving parse output first and
defers language-specific resolution to later passes.

## Example MEL

```mel
global proc string hello(string $name) {
    print ("hello " + $name);
}

`ls -sl`;
```
