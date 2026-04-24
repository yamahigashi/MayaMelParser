# Changelog

All notable changes to this project will be documented in this file.

## [0.1.3] - 2026-04-24

- Add byte-native lightweight scanning for non-UTF-8 `.ma` inputs so scan paths avoid whole-file decoded text and large source offset maps.
- Keep lightweight scan ranges in original input byte offsets and make selective Maya extraction decode only the local values it needs.
- Make the byte scanner encoding-aware for CP932/Shift-JIS style multibyte sequences so trail bytes are not misread as ASCII syntax.
- Refactor `mel-inspect` around explicit `parse`, `scan`, `selective`, and `corpus --engine full|scan|selective` commands.
- Route `mel-inspect scan`, `selective`, and legacy `--lightweight` through no-retain summary sinks instead of materializing `LightParse` and Maya top-level facts.
- Allow global CLI options such as `--max-bytes`, `--encoding`, and `--diagnostic-level` before or after subcommands.

## [0.1.2] - 2026-04-13

- Add `--max-bytes` to `mel-inspect` and thread parser budget controls through file-based CLI parsing paths.
- Extend file-based full and lightweight parse APIs with explicit options-bearing entry points instead of forcing defaults.
- Tighten the `maya-mel` public API surface so the crate root emphasizes the common workflow and keeps lower-level details under module paths.
- Add a lightweight scan callback summary API for no-retain streaming workflows, including source encoding, decode diagnostics, and parse errors.
- Refresh the top-level README to lead with capabilities, practical workflows, CLI usage, and current limits.

## [0.1.1] - 2026-04-12

- Add parser budget controls for full and lightweight parse entry points, including limits for bytes, tokens, statements, nesting depth, and literal size.
- Fail fast on parse budget overruns and report them through the existing parser diagnostic surfaces.
- Report unterminated block comments in the lightweight parser, including the EOF case covered by a regression test.
- Expand public API documentation examples for parsing, semantic analysis, and Maya top-level fact collection.

## [0.1.0] - 2026-04-05

- Collapse the public library surface into the single `maya-mel` crate.
- Keep syntax, lexer, AST, parser, sema, and Maya-specific layers as internal modules.
- Make `mel-cli` a workspace-only tool and prepare GitHub Releases artifact packaging for `mel-inspect`.
- Update CI and publish validation for the new single-crate release model.
