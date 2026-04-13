# MayaMelParser

`MayaMelParser` is a Rust workspace for parsing and analyzing Autodesk Maya MEL.
It is being built as a foundation for MEL tooling rather than a one-off parser.
The main library entry point is the single crate `maya-mel`, which provides
parsing, diagnostics, semantic analysis, and Maya command metadata through one
dependency.

The library is published as experimental `0.x`. Expect APIs to keep evolving
while parser recovery, semantic coverage, and corpus automation continue to
tighten.

## Getting Started

Add the library:

```bash
cargo add maya-mel
```

Parse source, run semantic analysis, and collect Maya-oriented facts:

```rust
use maya_mel::{analyze, collect_top_level_facts, parse_source};

let parsed = parse_source("global proc hello() {}");
let analysis = analyze(&parsed.syntax, parsed.source_view());
let facts = collect_top_level_facts(&parsed);

assert!(analysis.diagnostics.is_empty());
assert!(!facts.items.is_empty());
```

The crate currently includes:

- parsing for core MEL statement and expression surfaces
- source spans and diagnostics
- semantic analysis for proc visibility and command normalization
- Maya command metadata and top-level fact collection
- a lightweight scan path for fast inspection-oriented workflows

## CLI

`mel-inspect` is the local inspection CLI for parser and analysis output.
Prebuilt binaries are distributed through GitHub Releases. For local work from
this repository:

```bash
cargo install --path crates/mel-cli
```

Current CLI surface:

```text
mel-inspect [OPTIONS] [PATH]
  --inline <SOURCE>
  --lightweight
  --max-bytes <MAX_BYTES>
  --encoding <auto|utf8|cp932|gbk>
```

Example diagnostic output:

![Example `mel-inspect` diagnostic output](docs/images/inspect_example.png)

## Status

The project is already useful for parser and semantic-analysis experiments, but
it is still an experimental `0.x` library. Expect API changes while parser
recovery, semantic coverage, and corpus automation continue to improve.

Current limitations:

- Maya-specific specialization covers selected workflows, not the full language surface
- parser recovery and semantic coverage are still being expanded
- this repository is not aiming to be a formatter, interpreter, or full Maya runtime integration

## Design Notes

MEL looks close to C-family syntax in places, but several language features make
it awkward to model with a parser-only design:

- command syntax and function syntax coexist
- command results can be captured with backquotes
- command flags have command-specific meaning and arity
- proc resolution depends on semantic context rather than parse shape alone

Because of that, this project prefers surface-preserving parse output first and
defers language-specific resolution to later passes.

## Architecture

The workspace is organized around a generic MEL pipeline plus a Maya-specific layer:

```text
source
  -> maya-mel::syntax
  -> maya-mel::lexer
  -> maya-mel::ast
  -> maya-mel::parser
  -> maya-mel::sema
  -> maya-mel::maya
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

## Repository Layout

- `crates/maya-mel`: single public library crate with internal syntax/lexer/ast/parser/sema/maya modules
- `crates/mel-cli`: local inspection CLI for parser, sema, and lightweight output
- `tests/corpus`: public MEL fixtures and snapshot-oriented tests
- `examples`: small sample MEL sources

## Release Notes

See `CHANGELOG.md` for release notes.

## Example MEL

```mel
global proc string[] selected_transforms() {
    string $nodes[] = `ls -sl -type "transform"`;
    string $result[];

    for ($node in $nodes) {
        if (`objExists $node`) {
            $result[size($result)] = $node;
        }
    }

    return $result;
}
```
