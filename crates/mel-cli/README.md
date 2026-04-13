# mel-cli

`mel-cli` packages the `mel-inspect` binary for local inspection of Autodesk
Maya MEL parse output, diagnostics, and lightweight summaries.

## Distribution

Prebuilt `mel-inspect` binaries are intended to be distributed through GitHub
Releases. For local development inside this repository:

```bash
cargo install --path crates/mel-cli
```

## Usage

```bash
mel-inspect examples/basic.mel
mel-inspect --max-bytes 1048576 examples/basic.mel
mel-inspect --inline '`ls -sl`;'
mel-inspect --lightweight my-corpus
```

`--max-bytes` caps source bytes and scales the other parser budgets
proportionally from their defaults.

## Stability

This crate is a workspace tool and is not published to crates.io. CLI flags and
output format may change as the underlying library evolves.
