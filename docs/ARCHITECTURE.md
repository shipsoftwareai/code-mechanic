# Architecture

Code Mechanic is a foreground CLI around five deliberately small layers:

```text
filesystem / watcher events
          │
          ▼
 language adapters ──► normalized functions, prototypes and calls
          │
          ▼
 disposable SQLite WAL index
          │
          ├──► locator / outline / body search / exact source
          │
          └──► hash-bound refactor plan ──► parse gate ──► atomic apply
```

## Language Adapters

Tree-sitter provides the parser runtime and one concrete grammar per language.
The common adapter maps grammar-specific nodes into:

- function or method definitions;
- compatible declarations/prototypes;
- body, parameter and identifier byte ranges; and
- direct call or method/message name ranges.

Tree-sitter is language aware but not a compiler semantic model. It does not
resolve overloads, traits, interfaces, macro expansion, build configurations or
runtime dispatch. The common schema intentionally remains small so future
compiler/LSP identity adapters can augment it without replacing the storage and
plan protocol.

## Persistent Index

The index stores file fingerprints, parse status, functions/prototypes and call
references in SQLite WAL mode. Reconciliation compares size and nanosecond
mtime, hashes changed candidates, refreshes new files and deletes vanished
paths. Forced reconciliation hashes every supported candidate.

The database is derived data. It can always be deleted and rebuilt:

```sh
rm .code-mechanic/index.sqlite*
code-mechanic index
```

Every content-bearing query re-hashes its target before returning. A watcher is
an optimization, never the source of freshness authority.

## Locator Protocol

`locate` returns three spans:

- `function`: the complete definition;
- `signature`: function start through body start; and
- `body`: the grammar's body node.

Byte ranges are half-open `[start, end)` UTF-8 offsets. Lines are inclusive and
one-based. The short `snapshot` is diagnostic metadata, not permission to
write. Refactor apply requires the full freshly recomputed plan ID.

## Refactor Plans

A plan contains exact path, content hash, byte ranges, expected source text and
replacement text. Planning:

1. selects one unambiguous definition;
2. revalidates every indexed file hash;
3. rejects overlapping edits;
4. applies edits in reverse byte order in memory;
5. reparses every resulting file with its language grammar; and
6. hashes the complete operation and results into a plan ID.

Apply recomputes the plan, compares the supplied ID, stages sibling files with
preserved permissions, atomically renames them, rolls back a partial multi-file
commit and refreshes successful index rows.

## Watcher Lifecycle

One recursive native watcher covers a root. Events are debounced into file-level
refreshes; periodic and overflow reconciliation close missed-event gaps. Every
watcher registers root, database, PID, bounds and a heartbeat in a mode-restricted
per-user SQLite registry.

Normal return, errors, Ctrl-C, termination signals and cooperative stop all
explicitly unwatch and deregister. Drop cleanup is the final fallback.

## Crate Layout

| Module | Responsibility |
| --- | --- |
| `language` | Tree-sitter grammars and normalized facts |
| `index` | SQLite schema, scans, refresh, freshness and queries |
| `query` | Locator-first spans and bounded body search |
| `refactor` | Plan construction, validation, staging and apply |
| `watcher` | Bounded recursive event loop and reconciliation |
| `watch_registry` | Cross-root watcher inspection and teardown |
| `benchmark` | Exact-answer tokenizer and latency evidence |
