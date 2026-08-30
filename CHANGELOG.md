# Changelog

All notable changes to Code Mechanic are documented here. The project follows
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-08-30

### Added

- Persistent SQLite WAL AST index for Rust, C, C++, Go, Objective-C and GLSL.
- Locator-first function, signature and body spans with bounded body search.
- Exact function retrieval and AST-confirmed call references.
- Preview/apply function rename, entry injection, body replacement and appended
  parameter/call-argument migration.
- Content-hash freshness checks, ambiguity refusal, post-edit parse gates,
  staged atomic writes and rollback.
- Bounded native filesystem watcher with a cross-root lifecycle registry,
  cooperative stop, force stop and stale-registration pruning.
- Local tokenizer benchmark with answer-equivalence and honest small-function
  results.
- Homebrew tap, multi-platform CI and release archives.

[Unreleased]: https://github.com/shipsoftwareai/code-mechanic/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/shipsoftwareai/code-mechanic/releases/tag/v0.1.0
