# maya-mel

`maya-mel` is the single public entry point for parsing and analyzing Autodesk
Maya MEL in Rust.

The crate root intentionally stays small and covers the common workflow:
parse, analyze, and Maya top-level fact collection. Advanced APIs remain under
explicit module paths instead of a crate prelude.

## Scope

- parse MEL source into typed syntax
- run generic semantic analysis
- collect Maya-specific top-level command facts

## Example

```rust
use maya_mel::{analyze, collect_top_level_facts, parse_source};

let parsed = parse_source("global proc hello() {}");
let analysis = analyze(&parsed.syntax, parsed.source_view());
let facts = collect_top_level_facts(&parsed);

assert!(analysis.diagnostics.is_empty());
assert!(!facts.items.is_empty());
```

## Advanced Modules

- `maya_mel::parser`: lightweight and shared parse entry points
- `maya_mel::sema::command_schema`: custom command registries and schema types
- `maya_mel::sema::command_norm`: normalized command invoke structures
- `maya_mel::maya::model`: detailed Maya fact model types
- `maya_mel::syntax`, `maya_mel::lexer`, `maya_mel::ast`: low-level structures

## Stability

This crate is published as experimental `0.x`. Public APIs may change while the
parser and semantic layers continue to evolve.
