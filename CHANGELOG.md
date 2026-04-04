# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-04-05

- Collapse the public library surface into the single `maya-mel` crate.
- Keep syntax, lexer, AST, parser, sema, and Maya-specific layers as internal modules.
- Make `mel-cli` a workspace-only tool and prepare GitHub Releases artifact packaging for `mel-inspect`.
- Update CI and publish validation for the new single-crate release model.
