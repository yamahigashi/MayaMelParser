# Corpus fixtures

このディレクトリには将来的に MEL の fixture を置きます。

公開 fixture はこの `tests/corpus/` に、`mel-reference` や一般化した最小 MEL 例から手書きで追加します。

推奨構成:

```text
tests/corpus/
  lexer/
  parser/
  sema/
```

1 つの grammar 変更につき、できるだけ次を揃えてください。

- positive case
- negative case
- expected diagnostics または snapshot
- snapshot は代表ケース 5〜10 件程度に絞る

現在の parser fixture 例:

- `parser/proc/basic-global-proc.mel`
- `parser/proc/array-return-proc.mel`
- `parser/proc/local-array-param-proc.mel`
- `parser/proc/missing-proc-body.mel`
- `parser/proc/missing-proc-param-name.mel`
- `parser/statements/if-block-assignment.mel`
- `parser/statements/missing-block-semicolon.mel`
- `parser/statements/command-bareword.mel`
- `parser/statements/command-flags.mel`
- `parser/statements/command-grouped-args.mel`
- `parser/statements/malformed-command-word.mel`
- `parser/statements/missing-statement-parens.mel`
- `parser/statements/missing-statement-semi-recovery.mel`
- `parser/statements/for-loop-basic.mel`
- `parser/statements/for-in-basic.mel`
- `parser/statements/while-basic.mel`
- `parser/statements/missing-while-condition.mel`
- `parser/statements/missing-while-body.mel`
- `parser/statements/do-while-basic.mel`
- `parser/statements/missing-do-while-semi.mel`
- `parser/statements/switch-basic.mel`
- `parser/statements/missing-switch-case-value.mel`
- `parser/statements/missing-switch-colon.mel`
- `parser/statements/break-continue.mel`
- `parser/statements/var-decl-basic.mel`
- `parser/statements/global-var-decl.mel`
- `parser/statements/var-decl-multi-array.mel`
- `parser/statements/missing-var-declarator.mel`
- `parser/expressions/index-add-assign.mel`
- `parser/expressions/brace-list-assign.mel`
- `parser/expressions/exponent-float-basic.mel`
- `parser/expressions/ternary-basic.mel`
- `parser/expressions/vector-literal-basic.mel`
- `parser/expressions/vector-component-basic.mel`
- `parser/expressions/member-access-basic.mel`
- `parser/expressions/unary-negate-basic.mel`
- `parser/expressions/compound-assign-basic.mel`
- `parser/expressions/missing-compound-assign-rhs.mel`
- `parser/expressions/prefix-update-basic.mel`
- `parser/expressions/missing-prefix-update-operand.mel`
- `parser/expressions/missing-ternary-colon.mel`
- `parser/expressions/missing-unary-negate-operand.mel`
- `parser/expressions/missing-vector-close.mel`
- `parser/expressions/missing-member-name.mel`
- `parser/expressions/missing-brace-list-close.mel`
- `parser/expressions/missing-index-bracket.mel`

現在の sema fixture 例:

- `sema/proc/local-forward-reference.mel`
- `sema/proc/local-shell-unresolved.mel`
- `sema/proc/local-visible-call.mel`
- `sema/lint/read-before-write-and-shadowing.mel`
- `sema/lint/unresolved-variable.mel`

現在の diagnostics snapshot 対象:

- `lexer/strings/unterminated-string.mel`
- `lexer/symbols/unknown-char.mel`
- `parser/expressions/missing-ternary-colon.mel`
- `parser/proc/missing-proc-param-name.mel`
- `sema/proc/local-forward-reference.mel`
- `sema/proc/local-shell-unresolved.mel`
- `sema/lint/read-before-write-and-shadowing.mel`
- `sema/lint/unresolved-variable.mel`
