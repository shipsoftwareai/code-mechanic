# Refactor Safety

Code Mechanic automates syntax-addressed edits. It refuses work it cannot prove
at that layer, but a clean syntax tree is not a successful compile or a semantic
identity proof.

## Universal Guards

Every shipped refactor has:

- unique-definition selection, with an optional file filter;
- live content-hash validation;
- exact expected source bytes for every replacement;
- overlap detection;
- preview by default;
- apply bound to a freshly recomputed plan ID;
- clean post-edit Tree-sitter parses;
- sibling staging, atomic rename and multi-file rollback; and
- changed-row index refresh.

Formatting, type checking and project tests remain required after apply.

## Operation Matrix

| Operation | Syntax scope | Important boundary |
| --- | --- | --- |
| `rename` | Unique definition, compatible same-language prototypes and same-name AST calls | Name based, not compiler identity based |
| `inject-entry` | Immediately after a braced body opener | Refuses non-braced bodies and duplicate injected text |
| `replace-body` | Interior of one braced body | Preserves signature/braces; does not prove return/type behavior |
| `append-parameter` | Definition, compatible prototypes and parenthesized calls | Excludes Objective-C selectors; refuses variadic-last and C++ default-order violations |

## Language Boundaries

### Rust

Tree-sitter does not resolve traits, macros, `cfg`, re-exports or type-directed
method dispatch. A unique textual definition does not prove every same-name
method call refers to it.

### C, C++ And Objective-C

Preprocessor configurations, linkage, overload resolution, templates,
function-pointer targets and Objective-C selector identity require compiler
knowledge. Objective-C rename covers one contiguous indexed method identifier;
signature mutation is refused.

### Go

Receiver method calls and interface dispatch are indexed syntactically. The tool
does not run package/type analysis or prove interface satisfaction after a
signature change.

### GLSL

Shader include systems, defines, stage configuration and driver/compiler
dialects are outside Tree-sitter's concrete source tree. Compile every affected
shader variant after a change.

## Why File Moves Are Not Shipped Yet

A useful file move must atomically update dependency edges, not merely rename a
path. That means Rust modules and `#[path]`, C-family includes and build inputs,
Go package ownership/imports, and shader include/build references. The planned
implementation starts with a preview-only dependency graph and refuses unknown
edge types before gaining an apply mode.

## Choosing The Right Tool

Use Code Mechanic for exact repetitive source shapes. Use `rust-analyzer`,
Clang tooling, `gopls` or another compiler-backed language server for symbol
identity. Use `ast-grep` for ad-hoc structural patterns that do not need the
persistent index and fresh-plan apply protocol.
