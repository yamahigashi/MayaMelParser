# MayaMelParser

`MayaMelParser` is a Rust workspace for parsing and analyzing Autodesk Maya MEL.
It is being built as a foundation for MEL tooling rather than a one-off parser:
syntax, diagnostics, semantic analysis, and Maya-specific command metadata are
kept as separate layers so the workspace can support regression testing and
future editor-facing tooling.

## Current Status

The workspace is now solid enough for parser, semantic, and corpus-oriented
experimentation, but internal crate APIs are still evolving as the architecture
is tightened.

Today the workspace already includes:

- shared syntax primitives and span types
- a lexer with trivia retention and lexical diagnostics
- a parser for core MEL statement and expression surfaces, including a lightweight scan path
- typed AST structures used as the current parse output
- generic semantic analysis for proc visibility, command normalization, and diagnostics
- a Maya-specific metadata layer for builtin command registries and top-level command facts
- a small local CLI for inspecting parse, diagnostics, and lightweight summaries

The implementation is under active development, but the current crate layout and
CLI workflow are already useful for day-to-day parser and sema iteration.

## Getting Started

`mel-inspect` is the local inspection CLI for parser and analysis output.

```bash
mel-inspect examples/basic.mel
mel-inspect --inline '`ls -sl`;'
mel-inspect --lightweight my-corpus
mel-inspect --encoding cp932 my-corpus
```

Build from source:

```bash
cargo install --path crates/mel-cli
```

Current CLI surface:

```text
mel-inspect [OPTIONS] [PATH]
  --inline <SOURCE>
  --lightweight
  --encoding <auto|utf8|cp932|gbk>
```

Example diagnostic output:

![Example `mel-inspect` diagnostic output](docs/images/inspect_example.png)

## Architecture

The workspace is organized around a generic MEL pipeline plus a Maya-specific layer:

```text
source
  -> mel-syntax
  -> mel-lexer
  -> mel-ast
  -> mel-parser
  -> mel-sema
  -> mel-maya
  -> mel-cli
```

The current implementation is intentionally AST-first. Parsing preserves MEL
surface structure and does not try to fully resolve meaning too early. In
particular:

- parser output keeps command-style and function-style invocation surfaces
- command, proc, and plugin-command resolution belongs to semantic analysis
- Maya builtin catalogs and command specialization belong to the Maya layer
- spans are carried through syntax and diagnostics
- error recovery is treated as a first-class parser concern

A lossless CST may be added later if formatter, source-to-source rewrite, or
incremental editor workflows require it, but it is not part of the current
workspace surface.

## Workspace Layout

- `crates/mel-syntax`: shared span, token, and syntax primitives
- `crates/mel-lexer`: tokenization and lexical diagnostics
- `crates/mel-ast`: typed AST shapes used by parser and sema
- `crates/mel-parser`: parsing entry points, recovery, source decoding, and lightweight scanning
- `crates/mel-sema`: generic semantic analysis, diagnostics, and registry-backed command normalization
- `crates/mel-maya`: Maya builtin command metadata and top-level command collection helpers
- `crates/mel-cli`: local inspection CLI for parser, sema, and lightweight output
- `tests/corpus`: public MEL fixtures and snapshot-oriented tests
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

## Current Limitations

- The workspace is usable for parser/sema experimentation, but internal crate APIs may still change.
- Parser recovery, semantic coverage, and corpus automation are incomplete.
- Maya-specific command specialization exists for selected workflows, not the full language surface.
- This repository is not aiming to be a formatter, interpreter, or complete Maya runtime integration.

## Example MEL

```mel
global proc string hello(string $name) {
    print ("hello " + $name);
}

`ls -sl`;
```
