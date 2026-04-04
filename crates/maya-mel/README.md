# maya-mel

`maya-mel` is the single public entry point for parsing and analyzing Autodesk
Maya MEL in Rust.

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

## Stability

This crate is published as experimental `0.x`. Public APIs may change while the
parser and semantic layers continue to evolve.
