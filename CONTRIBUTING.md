# Contributing

Thanks for helping make structural tooling safer and cheaper for coding agents.

## Development

Install the pinned Rust toolchain, then run:

```sh
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
scripts/smoke.sh
```

Tests should stay silent, deterministic and independent of network services.
Every write-capable feature needs preview/apply coverage, stale-plan refusal and
a clean-tree post-edit assertion.

## Adding A Language

A language adapter is accepted only when the repository contains a maintained
Tree-sitter grammar and the contribution includes:

1. extension detection and grammar loading;
2. easy and complex definitions, declarations, bodies and call fixtures;
3. exact UTF-8 byte-range and line-range assertions;
4. comments/strings or other false-positive controls;
5. explicit refactor support and refusal boundaries; and
6. a real-code coverage and parse-failure report.

Adding an extension without grammar-specific extraction tests is not enough.

## Pull Requests

Keep changes focused. Explain the unsafe textual alternative being replaced,
the AST evidence used, refusal cases, tests and any benchmark result. Do not
weaken a refusal merely to increase apparent coverage.

By contributing, you agree that your contribution is licensed under either MIT
or Apache-2.0, at the user's option.
