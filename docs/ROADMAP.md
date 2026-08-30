# Roadmap

Code Mechanic optimizes for high-value, mechanically provable work rather than
the largest language badge count. A feature ships only with fresh-index
behavior, preview/apply plan binding, easy-to-complex fixtures, refusal tests and
measured agent output.

## Current Baseline

Rust, C, C++, Go, Objective-C, GLSL and Kotlin share:

- recursive indexing and forced whole-root reconciliation;
- diagnostics for every refused parse;
- outline, locator, bounded body search, exact source and call references;
- guarded function rename;
- braced function-entry injection and body replacement;
- formal-parameter plus call-argument append where syntax makes it safe; and
- watcher lifecycle and equivalent-answer token benchmarks.

Language-specific syntax is not flattened into false parity. Objective-C
selectors, Kotlin trailing lambdas and expression bodies, C++ default ordering,
variadics and compiler-semantic identity retain explicit refusal boundaries.

## Value-Ordered Work

| Priority | Capability | Why agents benefit | Acceptance boundary |
| --- | --- | --- | --- |
| P0 | Automatic query/refactor freshness preflight | Removes the need for agents to remember a reconcile command and makes watchers purely an optimization | New, changed, renamed and deleted files are reflected before every answer without a resident daemon |
| P1 | Transactional file move/rename | Replaces a risky path edit plus broad search across many files | Preview dependency edges, refuse unknown edges, atomically move and rewrite, roll back on any failure |
| P1 | Rich signature migration | Handles the repetitive edit agents most often perform after rename | Add/remove/reorder/rename parameters; update positional/named/default/trailing-lambda call shapes; compiler/typecheck gate adapters |
| P1 | Import/include/module repair | Makes file and symbol moves useful rather than cosmetic | Rust modules/use, C-family includes, Go packages/imports, Kotlin packages/imports/source sets and GLSL include conventions |
| P1 | Confidence-bearing relationship graph | Replaces iterative cross-file exploration for impact, caller and dependency questions | Typed import/containment/implements/inherits/type-use/test/call edges; provenance and confidence on every resolved edge; mutations accept only exact or compiler-confirmed identity |
| P2 | Type/member rename | Covers classes, structs, enums, interfaces, fields and constants | Declaration-kind-aware plans with collision and ambiguous-dispatch refusal |
| P2 | Extract helper/function | Reduces large-body token work and makes a common cleanup repeatable | Exact statement range, inputs/outputs preview, formatting hook and semantic verification requirement |
| P2 | Declarative multi-operation plans | Lets an agent request one reviewed transaction instead of serial plans that stale each other | One hash-bound plan, ordered non-overlapping edits and all-or-nothing apply |
| P2 | Recipe-based mechanical creation | Replaces large repetitive model outputs with a small intent plus deterministic local expansion | Named/versioned reviewed recipes, AST-bound insertion points, preview/apply binding, formatter/compiler gates and ambiguity refusal |
| P2 | Compact agent protocol | Avoids shell-output parsing and permits direct harness integration | Versioned JSON/stdin core first; an optional local-only MCP facade must preserve bounded output and explicit mutation plans |
| P3 | Compiler/LSP identity adapters | Safely crosses overloads, traits, interfaces, extensions and generated code | Optional rust-analyzer, Clang, gopls and Kotlin semantic providers augment—not replace—the offline AST core |

## File Move Sequence

File moves come before more exotic refactors because they are common, costly in
tokens and easy to get subtly incomplete.

1. Ship a preview-only dependency graph with exact path occurrences.
2. Support same-language moves whose dependency edges are completely known.
3. Add atomic apply with destination collision, case-only rename and rollback
   tests across macOS, Linux and Windows.
4. Add build-manifest adapters one ecosystem at a time; unknown build edges
   continue to refuse the plan.

For Kotlin, a move must understand the package declaration, imports, `.kt` and
`.kts`, Gradle source-set roots and case-sensitive JVM path conventions before
apply is enabled.

## Signature Migration Sequence

1. Add/remove a trailing parameter for purely positional calls.
2. Rename parameters and update Kotlin/other named arguments.
3. Reorder parameters with mixed positional and named calls.
4. Change type/default fragments with language-specific ordering checks.
5. Model Kotlin trailing lambdas and function-type parameters explicitly.
6. Offer compiler-backed verification adapters for identity and type safety.

## Mechanical Creation Without Constraining The Agent

Code creation is worthwhile when the desired structure is already known:
bindings, adapters, trait/interface implementations, builders, schema plumbing,
registries, serialization and repetitive tests. The agent should submit a small
intent to a named, versioned recipe distilled from an accepted repository
pattern; Code Mechanic should expand it locally and return only paths, hashes,
diagnostics and a compact diff summary.

Recipes must never become an implicit style oracle or a substitute for model
judgment. Novel business logic, algorithms and nuanced error handling remain
agent-authored. Every recipe requires explicit parameters, AST-bound insertion
points, a hash-bound preview/apply plan, formatting and project verification,
and refusal when the target or pattern is ambiguous. A repository may opt in to
recipes individually and review the source pattern from which each was derived.

The benchmark contract counts the complete interaction: agent request, tool
result, generated-code review and any correction turns. It compares that total
with direct model-generated code for identical accepted output. Initial output
savings alone are not a win if a rigid recipe creates more review or repair
work.

## Retrieval Evaluation

Fixture byte counts prove that compact structural answers can be smaller, but
they do not prove that an agent completes real work correctly. The next
evaluation layer will compare Code Mechanic-assisted and conventional
file-exploration agents on the same repositories, model, prompts and reference
answers. Retained evidence must include total input/output tokens, tool calls,
wall time, answer quality and any corrective turns. Categories should cover
symbol discovery, caller ranking, call paths, change impact, exact source
retrieval and exhaustive text search so the result exposes where structure
loses as clearly as where it wins.

The intended retrieval contract is hybrid: use high-confidence typed graph
edges to localize cross-file structure, exact source locators for the required
declaration/body, and ordinary text search within that bounded region when the
question depends on comments, macros or line-level detail. Fuzzy or heuristic
edges may assist exploration when labelled with confidence and provenance, but
must never authorize a mechanical edit.

## Public Tool Trust

Because an agent-invoked binary can read and mutate source without a human at
every call, release integrity is a product feature. Evolve the current checksum
release into reproducible provenance/attestations, an SBOM, dependency and
binary audits, filesystem-containment tests, adversarial structured-input tests
and proof that bounded watcher sessions leave no resident process. Network
access remains absent from normal indexing, query and refactor paths.

## Language Expansion

Operations and correctness come before breadth. After the P0/P1 work, candidate
languages are evaluated in this order:

1. Java, because its static model and JVM/Kotlin adjacency reuse move/import and
   signature work.
2. C#, for similar static refactor value and strong compiler/LSP verification.
3. Swift, once member/extension identity and platform grammar coverage have a
   reliable test corpus.
4. TypeScript, when compiler-backed identity is available; syntax-only name
   matching is too weak for its structural type and module patterns.

No language is accepted with extension detection alone. It needs a maintained
Tree-sitter grammar, clean cross-platform Rust build, structural extraction,
safe/refused operation matrix, realistic fixture corpus and benchmark evidence.
