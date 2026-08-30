# Agent Harness Integration

Code Mechanic is designed to be discovered once and called repeatedly by coding
agents without prose parsing or a resident daemon.

## Recommended Repository Instruction

Add this to `AGENTS.md` or the equivalent agent entrypoint:

```md
Use `code-mechanic` for supported static-language structure. Run
`code-mechanic --root . index --reconcile` before queries. Prefer `locate` and
bounded `search-body` before requesting complete source with `symbol --raw`.
All refactors preview by default; apply only with the exact fresh plan ID.
Inspect watchers with `code-mechanic watchers list` and prefer cooperative
`watchers stop-all`.
```

Also ignore the derived cache:

```gitignore
.code-mechanic/
```

## Startup Recipe

```sh
command -v code-mechanic
code-mechanic capabilities
code-mechanic --root . index --reconcile
code-mechanic --root . status
code-mechanic --root . diagnostics
```

The compact JSON response from `capabilities` is safe to cache for the process
lifetime. Do not cache locator or source responses across repository changes.

## Retrieval Recipe

1. If only a file is known, call `outline --file PATH`.
2. Resolve the unique definition with `locate NAME [--file PATH]`.
3. Search inside the fresh body with `search-body` and a low `--max-results`.
4. Use the returned line or byte range with the harness's bounded file reader.
5. Request `symbol --raw` only when the complete implementation is necessary.

Example:

```sh
code-mechanic --root . locate dispatch --file src/runtime.rs
code-mechanic --root . search-body dispatch --file src/runtime.rs \
  --pattern 'retry' --ignore-case --max-results 8
```

`search-body --regex` accepts a Rust regular expression and searches each body
line independently. It reports both `matching_lines` and `returned_lines`, plus
`truncated`, so an agent knows whether it received a sample.

## Refactor Recipe

1. Reconcile.
2. Request a preview without `--apply`.
3. Inspect `files_changed`, `replacements` and every occurrence.
4. Pass the returned `plan_id` to the identical command with `--apply`.
5. Run the relevant formatter/compiler/test gate.

Never parse a plan ID out of diagnostic prose; it is a top-level JSON field.
Never reuse a plan after any file in its scope may have changed.

## Process Lifecycle

The default watcher is bounded to 30 seconds with a two-second idle exit. A
harness that wants continuous watching must explicitly pass `--forever` and own
termination. At cleanup:

```sh
code-mechanic watchers list
code-mechanic watchers stop-all
```

Use `--force` only after inspecting the registry; it may signal registered
processes that miss cooperative shutdown.

## Output Contract

- stdout: compact JSON, except `symbol --raw`;
- stderr: compact structured error JSON;
- exit `0`: operation or preview succeeded;
- non-zero: refusal, stale state, invalid input or operational error;
- `--pretty`: human investigation only; compact output minimizes context.

File paths are workspace-relative. Refactors and indexed paths cannot escape
the canonical root through `..`, an absolute path or a symlink.
