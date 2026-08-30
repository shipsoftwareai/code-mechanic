# Code Mechanic Repository Guide

This file is the fresh-context entrypoint for every coding-agent task in this
repository. Code Mechanic is a conservative Rust CLI: source files are
authoritative, the SQLite index is disposable, and a refused edit is preferable
to an unsafe one.

## Start Here

1. Run `git status --short` and preserve pre-existing changes.
2. Read [README.md](README.md), then the focused authority for the work:
   [architecture](docs/ARCHITECTURE.md), [refactor safety](docs/REFACTOR-SAFETY.md),
   [agent integration](docs/AGENT-HARNESS.md), or
   [benchmarks](docs/BENCHMARKS.md).
3. Inspect [.agent-cube/harness.json](.agent-cube/harness.json) instead of
   inventing a build or test command. Each action has one executable script and
   a bounded timeout.
4. Use the narrow relevant action while iterating. Run
   `scripts/agent-cube/verify.sh` before a commit or broader green claim.
5. Run `git diff --check`, inspect untracked files, and stop any watcher started
   by the task before handing the checkout back.

## Layout And Ownership

| Path | Responsibility |
| --- | --- |
| `src/language.rs` | Tree-sitter grammars and normalized structural facts |
| `src/index.rs` | SQLite schema, indexing, reconciliation and freshness |
| `src/query.rs` | Locator-first retrieval and bounded body search |
| `src/refactor.rs` | Hash-bound plans, parse gates, atomic apply and rollback |
| `src/watcher.rs` | Recursive bounded filesystem watching and reconciliation |
| `src/watch_registry.rs` | Cross-root watcher inspection and cooperative teardown |
| `src/benchmark.rs` | Exact-answer token and latency evidence |
| `tests/` | Public contracts and easy-to-complex multi-language examples |
| `fixtures/benchmark/` | Retained benchmark/smoke workspace |
| `scripts/agent-cube/` | Canonical development and verification actions |

Keep grammar-specific extraction in `language`, persistence in `index`, query
formatting in `query`, and writes in `refactor`. Do not make the watcher a second
source of indexing truth.

## Core Invariants

- Tree-sitter is language aware, but not a compiler semantic model. Refuse work
  that needs overload, trait, interface, macro or build-configuration identity.
- Index only clean parses. Never turn parse failures into apparently complete
  structural results.
- A watcher is recursive and useful for latency, but asynchronous. Correctness
  must survive missed, coalesced and delayed events through root reconciliation.
- Every content-bearing result validates live source. Every write previews by
  default and applies only with the exact fresh plan ID.
- Writes stay below the canonical root, reject symlink escapes and overlapping
  edits, reparse all results, preserve permissions, and roll back partial
  multi-file commits.
- Machine output is compact JSON on stdout; structured failures go to stderr
  with a non-zero exit. Avoid prose that agents must scrape.
- Watchers are foreground, bounded by default, registered with useful metadata,
  explicitly unwatched on exit, and inspectable/stoppable across roots.
- Benchmarks require exact-answer equivalence and report regressions honestly;
  do not optimize the headline by weakening the requested answer.

## Development Actions

Run the scripts directly or through the action names in the harness registry:

```sh
scripts/agent-cube/build.sh
scripts/agent-cube/test.sh
scripts/agent-cube/lint.sh
scripts/agent-cube/refactor-smoke.sh
scripts/agent-cube/watcher-smoke.sh
scripts/agent-cube/benchmark-smoke.sh
scripts/agent-cube/kotlin-smoke.sh
scripts/agent-cube/agent-contract-smoke.sh
scripts/agent-cube/verify.sh
```

The pinned toolchain owns formatting and lint behavior. Tests must remain
deterministic, silent by default and independent of network services.

When adding a language, include easy and complex definitions, declarations,
bodies and calls; exact UTF-8 byte/line assertions; false-positive controls;
refactor support/refusal boundaries; and real-code parse coverage. An extension
mapping without grammar-specific tests is incomplete.

When adding or changing an operator-facing harness script, register it exactly
once in `.agent-cube/harness.json` and keep the registry guard green.

## Agent Use Of Code Mechanic While Developing It

The checked-out source may differ from an installed release, so use the freshly
built debug binary for self-hosting:

```sh
cargo build --locked
target/debug/code-mechanic index --root . --reconcile --force-hash
target/debug/code-mechanic locate watch_loop --root . --file src/watcher.rs
```

Force a whole-root reconcile immediately before each structural query or
refactor preview. Treat a running watcher only as a speed optimization. Preview
all refactors, inspect every occurrence, and apply only with the exact fresh
plan ID. Prefer `locate` and bounded `search-body` before `symbol --raw`.

Before handoff:

```sh
target/debug/code-mechanic watchers list
target/debug/code-mechanic watchers stop-all
```

Do not publish a crate, create a GitHub release, modify the Homebrew tap, or push
unless the user explicitly requests that external state change.
