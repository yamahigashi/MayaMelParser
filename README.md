# MayaMelParser

`MayaMelParser` is a Rust workspace for parsing and analyzing Autodesk Maya MEL.
The main library is [`maya-mel`](https://crates.io/crates/maya-mel), an
experimental `0.x` crate for typed parsing, diagnostics, semantic analysis, and
Maya-oriented top-level command facts.

Use it when you need to inspect MEL source, build diagnostics, or extract
structured facts from Maya scripts. Do not treat it as a formatter,
interpreter, or full Maya runtime integration.

## What You Get Today

- typed MEL parsing for core statement and expression surfaces
- source spans, decode diagnostics, lex/parse diagnostics, and semantic diagnostics
- semantic analysis for proc visibility, name resolution, and command normalization
- Maya-oriented top-level fact collection backed by command metadata
- lightweight parse/scan surfaces for inspection-oriented or large-input workflows
- `mel-inspect`, a local CLI for single-file inspection and corpus summaries

## Quick Start

Add the library:

```bash
cargo add maya-mel
```

Parse source and run generic semantic analysis:

```rust
use maya_mel::{analyze, parse_source};

let parsed = parse_source("global proc hello() { print(\"hi\"); }");
let analysis = analyze(&parsed.syntax, parsed.source_view());

assert!(parsed.errors.is_empty());
assert!(analysis.diagnostics.is_empty());
```

Collect Maya-oriented facts when builtin command metadata matters:

```rust
use maya_mel::{
    MayaCommandRegistry, collect_top_level_facts_with_registry, parse_source,
};

let parsed = parse_source("createNode transform -n \"root\";");
let facts =
    collect_top_level_facts_with_registry(&parsed, &MayaCommandRegistry::new());

assert_eq!(facts.items.len(), 1);
```

Common entry points:

- `parse_source` / `parse_file` for full typed parsing
- `analyze` / `analyze_with_registry` for semantic diagnostics
- `collect_top_level_facts` / `collect_top_level_facts_with_registry` for Maya-oriented extraction
- `maya_mel::parser::parse_light_*` and `scan_light_*` for lightweight workflows

## CLI

Install the local inspection CLI from this repository:

```bash
cargo install --path crates/mel-cli
```

Current CLI surface:

```text
mel-inspect [OPTIONS] [PATH]
  --encoding <auto|utf8|cp932|gbk>
  --diagnostic-level <all|error|none>
  --max-bytes <MAX_BYTES>
  --lightweight
  --inline <SOURCE>
```

Inspect one file:

```bash
mel-inspect examples/basic.mel
```

Inspect with the lightweight path:

```bash
mel-inspect --lightweight examples/basic.mel
```

Pass inline source:

```bash
mel-inspect --inline 'createNode transform -n "root";'
```

If `PATH` is a directory, `mel-inspect` prints a corpus-style summary across
the discovered MEL files.

Example diagnostic output:

![Example `mel-inspect` diagnostic output](docs/images/inspect_example.png)

## Limits

- coverage is useful today, but still incomplete for the full MEL language surface
- parser recovery and semantic coverage are still expanding
- Maya-specific specialization is selective rather than exhaustive
- this repository is not targeting formatting, interpretation, or direct Maya runtime integration

## Architecture

The current pipeline is AST-first and keeps parsing separate from semantic
resolution:

```text
source
  -> syntax
  -> lexer
  -> ast
  -> parser
  -> sema
  -> maya
  -> mel-cli
```

Important design choices:

- parser output preserves invocation surface instead of resolving commands too early
- command, proc, and plugin-command resolution belong to semantic analysis
- spans are carried through syntax and diagnostics
- a lightweight path exists alongside the full parse surface for large inputs

Design details live in [.agents/docs/architecture.md](.agents/docs/architecture.md).

## Repository Layout

- `crates/maya-mel`: public library crate
- `crates/mel-cli`: local inspection CLI
- `tests/corpus`: public MEL fixtures
- `examples`: sample MEL sources

## Releases

See [CHANGELOG.md](CHANGELOG.md) for release notes.
