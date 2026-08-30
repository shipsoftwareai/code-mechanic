//! Easy-to-complex syntax examples that exercise byte addressing and guards.

use std::fmt::Write as _;
use std::path::Path;

use code_mechanic::benchmark::{self, BenchmarkCase};
use code_mechanic::index::CodeIndex;
use code_mechanic::language::{Language, SymbolKind, parse};
use code_mechanic::refactor::{inject_function_entry, rename_function};
use tempfile::TempDir;

fn write(root: &Path, relative: &str, source: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

fn index_for(root: &Path) -> CodeIndex {
    CodeIndex::open(root, &root.join(".index/code-mechanic.sqlite")).unwrap()
}

#[test]
fn rust_parser_examples_progress_from_free_function_to_generic_async_method() {
    let cases = [
        (
            "easy free function",
            "fn target() {}\nfn caller() { target(); }\n",
            &["target", "caller"][..],
            &["target"][..],
        ),
        (
            "qualified and turbofish calls",
            r"
fn target<T: Default>(value: T) -> T { value }
fn caller() { let _ = crate::nested::target::<u32>(1); }
",
            &["target", "caller"],
            &["target"],
        ),
        (
            "attributed async generic method",
            r"
struct Worker;
impl Worker {
    #[inline]
    async fn target<'a, T>(&self, value: &'a T) -> usize
    where T: Send + Sync
    { consume(value).await }
    async fn caller(&self) { let _ = self.target(&7).await; }
}
",
            &["target", "caller"],
            &["consume", "target"],
        ),
        (
            "unsafe extern function",
            r#"
unsafe extern "C" fn target(value: *const u8) -> usize { value as usize }
fn caller(pointer: *const u8) { let _ = unsafe { target(pointer) }; }
"#,
            &["target", "caller"],
            &["target"],
        ),
    ];

    for (label, source, expected_defs, expected_calls) in cases {
        let parsed = parse(Language::Rust, source).unwrap();
        assert!(!parsed.tree.root_node().has_error(), "{label}");
        let defs: Vec<_> = parsed
            .functions
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        let calls: Vec<_> = parsed.calls.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(defs, expected_defs, "{label}");
        assert_eq!(calls, expected_calls, "{label}");
    }
}

#[test]
fn c_parser_examples_cover_headers_pointer_returns_fields_and_function_pointers() {
    let cases = [
        (
            "prototype and definition",
            "int target(int value);\nint target(int value) { return value; }\n",
            vec![
                ("target", SymbolKind::Prototype),
                ("target", SymbolKind::Definition),
            ],
            Vec::<&str>::new(),
        ),
        (
            "pointer return and multiline declarator",
            r#"
char *
target(
    int value,
    const char *label
) { return value > 0 ? (char *)label : 0; }
char *caller(void) { return target(1, "ok"); }
"#,
            vec![
                ("target", SymbolKind::Definition),
                ("caller", SymbolKind::Definition),
            ],
            vec!["target"],
        ),
        (
            "field function pointer call",
            r"
struct Api { int (*target)(int); };
int caller(struct Api api) { return api.target(3); }
",
            vec![("caller", SymbolKind::Definition)],
            vec!["target"],
        ),
        (
            "function pointer variable is not a prototype",
            "int (*target)(int);\nint caller(void) { return target(2); }\n",
            vec![("caller", SymbolKind::Definition)],
            vec!["target"],
        ),
    ];

    for (label, source, expected_defs, expected_calls) in cases {
        let parsed = parse(Language::C, source).unwrap();
        assert!(!parsed.tree.root_node().has_error(), "{label}");
        let defs: Vec<_> = parsed
            .functions
            .iter()
            .map(|item| (item.name.as_str(), item.kind))
            .collect();
        let calls: Vec<_> = parsed.calls.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(defs, expected_defs, "{label}");
        assert_eq!(calls, expected_calls, "{label}");
    }
}

#[test]
fn unicode_before_target_does_not_corrupt_byte_addressed_rename() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "src/lib.rs",
        r#"
const LABEL: &str = "Grüße from λ";
fn target() -> &'static str { LABEL }
fn caller() -> &'static str { target() }
"#,
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let preview = rename_function(&index, "target", "renamed", None, false, None).unwrap();
    rename_function(
        &index,
        "target",
        "renamed",
        None,
        true,
        Some(&preview.plan_id),
    )
    .unwrap();
    let source = std::fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(source.contains("Grüße from λ"));
    assert!(source.contains("fn renamed()"));
    assert!(source.contains("{ renamed() }"));
}

#[test]
fn compact_crlf_c_body_accepts_multiline_entry_injection() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "native/work.c",
        "int target(void) { return 1; }\r\n",
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let preview =
        inject_function_entry(&index, "target", "first();\nsecond();", None, false, None).unwrap();
    inject_function_entry(
        &index,
        "target",
        "first();\nsecond();",
        None,
        true,
        Some(&preview.plan_id),
    )
    .unwrap();
    let source = std::fs::read_to_string(workspace.path().join("native/work.c")).unwrap();
    assert!(source.contains("{\r\n    first();\r\n    second();\r\n return 1; }"));
    assert!(!source.replace("\r\n", "").contains('\n'));
    assert!(
        !parse(Language::C, &source)
            .unwrap()
            .tree
            .root_node()
            .has_error()
    );
}

#[test]
fn destination_collision_and_parse_error_sources_are_refused() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "src/lib.rs",
        "fn source() {}\nfn destination() {}\n",
    );
    write(
        workspace.path(),
        "src/broken.rs",
        "fn broken( { this is not Rust\n",
    );
    let index = index_for(workspace.path());
    let summary = index.rebuild().unwrap();
    assert_eq!(summary.parse_failures, 1);
    let diagnostics = index.diagnostics().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].path, "src/broken.rs");
    assert_eq!(diagnostics[0].language, "rust");
    let collision =
        rename_function(&index, "source", "destination", None, false, None).unwrap_err();
    assert!(collision.to_string().contains("already has"));
    let broken =
        inject_function_entry(&index, "broken", "observe();", None, false, None).unwrap_err();
    assert!(broken.to_string().contains("no indexed function"));
}

#[test]
fn multi_case_benchmark_reports_clear_per_case_and_aggregate_savings() {
    let workspace = TempDir::new().unwrap();
    let rust_easy = padded_source(
        "fn rust_easy() -> usize { 1 }\nfn use_rust_easy() -> usize { rust_easy() }\n",
        "rust_pad",
    );
    let rust_complex = padded_source(
        r"
async fn rust_complex<T: Send>(value: T) -> T { value }
async fn use_rust_complex() { let _ = rust_complex::<u32>(7).await; }
",
        "complex_pad",
    );
    let c_complex = padded_c_source(
        r"
int c_complex(int value);
int c_complex(int value) { return value + 1; }
int use_c_complex(void) { return c_complex(4); }
",
        "c_pad",
    );
    write(workspace.path(), "src/easy.rs", &rust_easy);
    write(workspace.path(), "src/complex.rs", &rust_complex);
    write(workspace.path(), "native/complex.c", &c_complex);
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let report = benchmark::run(
        &index,
        &[
            BenchmarkCase {
                symbol: "rust_easy".to_owned(),
                file: Some("src/easy.rs".to_owned()),
            },
            BenchmarkCase {
                symbol: "rust_complex".to_owned(),
                file: Some("src/complex.rs".to_owned()),
            },
            BenchmarkCase {
                symbol: "c_complex".to_owned(),
                file: Some("native/complex.c".to_owned()),
            },
        ],
        3,
        100,
        50.0,
    )
    .unwrap();
    assert!(report.passed, "{report:#?}");
    assert_eq!(report.aggregate.cases_passed, 3);
    assert_eq!(report.aggregate.cases_total, 3);
    assert!(report.aggregate.baseline_tokens > report.aggregate.indexed_tokens);
    assert!(report.aggregate.token_reduction_pct >= 50.0);
    assert!(report.cases.iter().all(|case| case.answer_equivalent));
}

fn padded_source(core: &str, prefix: &str) -> String {
    let mut source = String::new();
    for index in 0..70 {
        writeln!(
            source,
            "fn {prefix}_before_{index}() -> usize {{ {index} }}"
        )
        .unwrap();
    }
    source.push_str(core);
    for index in 0..70 {
        writeln!(source, "fn {prefix}_after_{index}() -> usize {{ {index} }}").unwrap();
    }
    source
}

fn padded_c_source(core: &str, prefix: &str) -> String {
    let mut source = String::new();
    for index in 0..70 {
        writeln!(
            source,
            "int {prefix}_before_{index}(void) {{ return {index}; }}"
        )
        .unwrap();
    }
    source.push_str(core);
    for index in 0..70 {
        writeln!(
            source,
            "int {prefix}_after_{index}(void) {{ return {index}; }}"
        )
        .unwrap();
    }
    source
}
