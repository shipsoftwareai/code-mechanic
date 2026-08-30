use std::fmt::Write as _;
use std::path::Path;
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use code_mechanic::benchmark::{self, BenchmarkCase};
use code_mechanic::index::CodeIndex;
use code_mechanic::query::{self, BodySearchOptions};
use code_mechanic::refactor::{
    append_parameter, inject_function_entry, rename_function, replace_function_body,
};
use code_mechanic::watch_registry::WatcherRegistry;
use code_mechanic::watcher::{self, WatchConfig};
use tempfile::TempDir;

fn index_for(root: &Path) -> CodeIndex {
    CodeIndex::open(root, &root.join(".index/code-mechanic.sqlite")).unwrap()
}

fn write(root: &Path, relative: &str, source: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

#[test]
fn persistent_index_returns_exact_rust_and_c_structure() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "src/lib.rs",
        r#"
fn alpha() -> i32 { beta() }
fn beta() -> i32 { 7 }
// beta() is prose, not a call.
const TEXT: &str = "beta()";
"#,
    );
    write(
        workspace.path(),
        "native/helper.c",
        r"
int helper(int value);
static int helper(int value) { return value + 1; }
int run(void) { return helper(4); }
",
    );
    let index = index_for(workspace.path());
    let summary = index.rebuild().unwrap();
    assert_eq!(summary.files, 2);
    assert_eq!(summary.parse_failures, 0);
    assert_eq!(index.references("beta").unwrap().len(), 1);
    assert_eq!(index.references("helper").unwrap().len(), 1);
    assert_eq!(index.symbols("helper", None).unwrap().len(), 2);
    let alpha = index.read_symbol("alpha", None).unwrap();
    assert_eq!(alpha.path, "src/lib.rs");
    assert_eq!(alpha.source, "fn alpha() -> i32 { beta() }");

    drop(index);
    let reopened = index_for(workspace.path());
    assert_eq!(reopened.read_symbol("run", None).unwrap().language, "c");
}

#[test]
fn locator_returns_exact_spans_and_body_search_is_scoped_bounded_and_absolute() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "src/query.rs",
        r"fn targeted(
    signature_only: usize,
) -> usize {
    let work = signature_only + 1;
    WORK(work);
    work
}
",
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();

    let source = std::fs::read_to_string(workspace.path().join("src/query.rs")).unwrap();
    let exact = index.read_symbol("targeted", None).unwrap();
    let location = query::locate(&index, "targeted", None).unwrap();
    assert_eq!(
        &source[location.function.bytes[0]..location.function.bytes[1]],
        exact.source
    );
    assert_eq!(
        &source[location.signature.bytes[0]..location.signature.bytes[1]],
        "fn targeted(\n    signature_only: usize,\n) -> usize "
    );
    assert_eq!(
        &source[location.body.bytes[0]..location.body.bytes[1]],
        "{\n    let work = signature_only + 1;\n    WORK(work);\n    work\n}"
    );
    assert_eq!(location.function.lines, [1, 7]);
    assert_eq!(location.signature.lines, [1, 3]);
    assert_eq!(location.body.lines, [3, 7]);
    assert_eq!(location.snapshot.len(), 16);

    let signature_only = query::search_body(
        &index,
        "targeted",
        None,
        &BodySearchOptions {
            pattern: "usize",
            regex: false,
            ignore_case: false,
            max_results: 20,
        },
    )
    .unwrap();
    assert_eq!(signature_only.matching_lines, 0);

    let bounded = query::search_body(
        &index,
        "targeted",
        None,
        &BodySearchOptions {
            pattern: r"(?i)work",
            regex: true,
            ignore_case: false,
            max_results: 2,
        },
    )
    .unwrap();
    assert_eq!(bounded.matching_lines, 3);
    assert_eq!(bounded.returned_lines, 2);
    assert!(bounded.truncated);
    for matched in &bounded.matches {
        assert!(matched.line >= location.body.lines[0]);
        assert!(matched.line <= location.body.lines[1]);
        assert_eq!(&source[matched.bytes[0]..matched.bytes[1]], matched.text);
        for range in &matched.match_bytes {
            assert_eq!(source[range[0]..range[1]].to_ascii_lowercase(), "work");
        }
    }
}

#[test]
fn body_search_handles_c_crlf_lines_without_leaking_carriage_returns() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "native/query.c",
        "int c_target(int needle) {\r\n    int found = needle + 1;\r\n    return found;\r\n}\r\n",
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let report = query::search_body(
        &index,
        "c_target",
        None,
        &BodySearchOptions {
            pattern: "found",
            regex: false,
            ignore_case: false,
            max_results: 10,
        },
    )
    .unwrap();
    assert_eq!(report.matching_lines, 2);
    assert_eq!(report.matches[0].line, 2);
    assert!(
        report
            .matches
            .iter()
            .all(|matched| !matched.text.contains('\r'))
    );
}

#[test]
fn static_language_index_locates_go_cpp_objective_c_glsl_and_kotlin_bodies() {
    let workspace = TempDir::new().unwrap();
    let fixtures = [
        (
            "cmd/tool/main.go",
            "package main\nfunc GoTarget(value int) int { return value + 1 }\n",
            "GoTarget",
            "go",
        ),
        (
            "native/tool.cpp",
            "int cppTarget(int value) { return value + 1; }\n",
            "cppTarget",
            "cpp",
        ),
        (
            "native/tool.m",
            "@implementation Worker\n- (void)objcTarget { return; }\n@end\n",
            "objcTarget",
            "objective-c",
        ),
        (
            "shaders/tool.frag",
            "float glslTarget(float value) { return value + 1.0; }\n",
            "glslTarget",
            "glsl",
        ),
        (
            "src/main/kotlin/Tool.kt",
            "fun kotlinTarget(value: Int): Int { return value + 1 }\n",
            "kotlinTarget",
            "kotlin",
        ),
    ];
    for (path, source, _, _) in fixtures {
        write(workspace.path(), path, source);
    }
    let index = index_for(workspace.path());
    let summary = index.rebuild().unwrap();
    assert_eq!(summary.files, 5);
    assert_eq!(summary.parse_failures, 0);
    for (path, _, symbol, language) in fixtures {
        let location = query::locate(&index, symbol, Some(path)).unwrap();
        assert_eq!(location.language, language);
        assert_eq!(location.body.bytes[0] + 1, location.signature.bytes[1] + 1);
        assert!(location.body.lines[1] >= location.body.lines[0]);
    }
}

#[test]
fn replace_body_is_plan_bound_and_preserves_go_signature() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "cmd/tool/main.go",
        "package main\nfunc calculate(value int) int {\n    return value + 1\n}\nfunc caller() int { return calculate(4) }\n",
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let preview = replace_function_body(
        &index,
        "calculate",
        "result := value * 2\nreturn result",
        None,
        false,
        None,
    )
    .unwrap();
    assert_eq!(preview.replacements, 1);
    replace_function_body(
        &index,
        "calculate",
        "result := value * 2\nreturn result",
        None,
        true,
        Some(&preview.plan_id),
    )
    .unwrap();
    let source = std::fs::read_to_string(workspace.path().join("cmd/tool/main.go")).unwrap();
    assert!(source.contains("func calculate(value int) int {"));
    assert!(source.contains("    result := value * 2\n    return result"));
    assert!(source.contains("return calculate(4)"));
}

#[test]
fn append_parameter_updates_static_definitions_prototypes_and_calls() {
    let workspace = TempDir::new().unwrap();
    let cases = [
        (
            "src/lib.rs",
            "fn rust_target(value: i32) -> i32 { value }\nfn rust_call() -> i32 { rust_target(1) }\n",
            "rust_target",
            "flag: bool",
            "true",
            2,
            "rust_target(value: i32, flag: bool)",
            "rust_target(1, true)",
        ),
        (
            "native/tool.c",
            "int c_target(void);\nint c_target(void) { return 1; }\nint c_call(void) { return c_target(); }\n",
            "c_target",
            "int flag",
            "1",
            3,
            "c_target(int flag)",
            "c_target(1)",
        ),
        (
            "native/tool.cpp",
            "int cppTarget(int value);\nint cppTarget(int value) { return value; }\nint cppCall() { return cppTarget(1); }\n",
            "cppTarget",
            "bool flag",
            "true",
            3,
            "cppTarget(int value, bool flag)",
            "cppTarget(1, true)",
        ),
        (
            "cmd/tool/main.go",
            "package main\nfunc GoTarget(value int) int { return value }\nfunc GoCall() int { return GoTarget(1) }\n",
            "GoTarget",
            "flag bool",
            "true",
            2,
            "GoTarget(value int, flag bool)",
            "GoTarget(1, true)",
        ),
        (
            "shaders/tool.frag",
            "float glslTarget(float value);\nfloat glslTarget(float value) { return value; }\nvoid glslCall() { float value = glslTarget(1.0); }\n",
            "glslTarget",
            "bool flag",
            "true",
            3,
            "glslTarget(float value, bool flag)",
            "glslTarget(1.0, true)",
        ),
        (
            "src/main/kotlin/Tool.kt",
            "fun kotlinTarget(value: Int): Int { return value }\nfun kotlinCall(): Int { return kotlinTarget(1) }\n",
            "kotlinTarget",
            "flag: Boolean",
            "true",
            2,
            "kotlinTarget(value: Int, flag: Boolean)",
            "kotlinTarget(1, true)",
        ),
    ];
    for (path, source, ..) in cases {
        write(workspace.path(), path, source);
    }
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    for (path, _, symbol, parameter, argument, replacements, signature, call) in cases {
        let preview =
            append_parameter(&index, symbol, parameter, argument, Some(path), false, None).unwrap();
        assert_eq!(preview.replacements, replacements, "{symbol}");
        append_parameter(
            &index,
            symbol,
            parameter,
            argument,
            Some(path),
            true,
            Some(&preview.plan_id),
        )
        .unwrap();
        let source = std::fs::read_to_string(workspace.path().join(path)).unwrap();
        assert!(source.contains(signature), "{source}");
        assert!(source.contains(call), "{source}");
    }
}

#[test]
fn append_parameter_refuses_objective_c_selectors_and_cpp_default_ordering() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "native/tool.m",
        "@implementation Worker\n- (void)consume:(int)value { return; }\n@end\n",
    );
    write(
        workspace.path(),
        "native/tool.cpp",
        "int configured(int value = 1) { return value; }\n",
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    assert!(
        append_parameter(&index, "consume", "int flag", "1", None, false, None)
            .unwrap_err()
            .to_string()
            .contains("selector")
    );
    assert!(
        append_parameter(&index, "configured", "bool flag", "true", None, false, None,)
            .unwrap_err()
            .to_string()
            .contains("default")
    );
}

#[test]
fn kotlin_append_parameter_refuses_unsafe_named_and_trailing_lambda_calls() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "src/main/kotlin/Calls.kt",
        r"
fun namedTarget(value: Int): Int { return value }
fun namedCaller(): Int { return namedTarget(value = 1) }

fun trailingTarget(value: Int, transform: (Int) -> Int): Int { return transform(value) }
fun trailingCaller(): Int { return trailingTarget(1) { it + 1 } }
",
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();

    let named = append_parameter(
        &index,
        "namedTarget",
        "enabled: Boolean",
        "true",
        None,
        false,
        None,
    )
    .unwrap_err();
    assert!(
        named
            .to_string()
            .contains("require the appended argument to be named")
    );

    let trailing = append_parameter(
        &index,
        "trailingTarget",
        "enabled: Boolean",
        "true",
        None,
        false,
        None,
    )
    .unwrap_err();
    assert!(trailing.to_string().contains("parenthesized argument list"));
}

#[test]
fn rust_rename_is_previewed_plan_bound_and_ast_only() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "src/lib.rs",
        r#"
fn old_name(value: i32) -> i32 { value + 1 }
fn first() -> i32 { old_name(1) }
fn second() -> i32 { crate::old_name(2) }
// old_name() must remain in this comment.
const TEXT: &str = "old_name() must remain in this string";
"#,
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();

    let preview = rename_function(&index, "old_name", "new_name", None, false, None).unwrap();
    assert!(!preview.applied);
    assert_eq!(preview.files_changed, 1);
    assert_eq!(preview.replacements, 3);
    let wrong = rename_function(
        &index,
        "old_name",
        "new_name",
        None,
        true,
        Some("wrong-plan"),
    )
    .unwrap_err();
    assert!(wrong.to_string().contains("plan mismatch"));

    let applied = rename_function(
        &index,
        "old_name",
        "new_name",
        None,
        true,
        Some(&preview.plan_id),
    )
    .unwrap();
    assert!(applied.applied);
    let source = std::fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(source.contains("fn new_name"));
    assert_eq!(source.matches("new_name(").count(), 3);
    assert!(source.contains("// old_name() must remain"));
    assert!(source.contains("\"old_name() must remain"));
    assert_eq!(index.definitions("new_name", None).unwrap().len(), 1);
    assert_eq!(index.references("new_name").unwrap().len(), 2);
}

#[test]
fn c_rename_updates_prototype_definition_and_call_but_not_comments() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "native/work.c",
        r"
int old_name(int value);
int old_name(int value) { return value + 1; }
int caller(void) { return old_name(3); }
// old_name(4) remains documentary text.
",
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let preview = rename_function(&index, "old_name", "new_name", None, false, None).unwrap();
    assert_eq!(preview.replacements, 3);
    rename_function(
        &index,
        "old_name",
        "new_name",
        None,
        true,
        Some(&preview.plan_id),
    )
    .unwrap();
    let source = std::fs::read_to_string(workspace.path().join("native/work.c")).unwrap();
    assert_eq!(source.matches("new_name(").count(), 3);
    assert!(source.contains("// old_name(4)"));
}

#[test]
fn apply_refuses_a_stale_preview_without_writing() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "src/lib.rs",
        "fn target() {}\nfn caller() { target(); }\n",
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let preview = rename_function(&index, "target", "renamed", None, false, None).unwrap();
    std::fs::write(
        workspace.path().join("src/lib.rs"),
        "fn target() {}\nfn caller() { target(); }\n// concurrent edit\n",
    )
    .unwrap();
    let locate_error = query::locate(&index, "target", None).unwrap_err();
    assert!(locate_error.to_string().contains("stale index"));
    let error = rename_function(
        &index,
        "target",
        "renamed",
        None,
        true,
        Some(&preview.plan_id),
    )
    .unwrap_err();
    assert!(error.to_string().contains("stale index"));
    let source = std::fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(source.contains("fn target"));
    assert!(!source.contains("fn renamed"));
}

#[test]
fn function_entry_injection_handles_rust_and_c_and_is_idempotent() {
    let workspace = TempDir::new().unwrap();
    write(
        workspace.path(),
        "src/lib.rs",
        "fn rust_target() {\n    work();\n}\nfn work() {}\n",
    );
    write(
        workspace.path(),
        "native/work.c",
        "int c_target(void) { return 2; }\n",
    );
    let index = index_for(workspace.path());
    index.rebuild().unwrap();

    let rust_preview =
        inject_function_entry(&index, "rust_target", "trace_entry();", None, false, None).unwrap();
    inject_function_entry(
        &index,
        "rust_target",
        "trace_entry();",
        None,
        true,
        Some(&rust_preview.plan_id),
    )
    .unwrap();
    let rust_source = std::fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(rust_source.contains("{\n    trace_entry();\n    work();"));
    assert!(
        inject_function_entry(&index, "rust_target", "trace_entry();", None, false, None,)
            .unwrap_err()
            .to_string()
            .contains("already contains")
    );

    let c_preview =
        inject_function_entry(&index, "c_target", "observe_entry();", None, false, None).unwrap();
    inject_function_entry(
        &index,
        "c_target",
        "observe_entry();",
        None,
        true,
        Some(&c_preview.plan_id),
    )
    .unwrap();
    let c_source = std::fs::read_to_string(workspace.path().join("native/work.c")).unwrap();
    assert!(c_source.contains("{\n    observe_entry();\n return 2; }"));
}

#[test]
fn ambiguous_definition_is_rejected_before_a_plan_exists() {
    let workspace = TempDir::new().unwrap();
    write(workspace.path(), "src/a.rs", "fn duplicate() {}\n");
    write(workspace.path(), "src/b.rs", "fn duplicate() {}\n");
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let error = rename_function(&index, "duplicate", "renamed", None, false, None).unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
}

#[test]
fn bounded_watcher_refreshes_then_explicitly_unwatches() {
    let workspace = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    write(workspace.path(), "src/lib.rs", "fn before() {}\n");
    let index = CodeIndex::open(workspace.path(), &state.path().join("index.sqlite")).unwrap();
    index.rebuild().unwrap();
    let watched_index = index.clone();
    let config = WatchConfig {
        duration: Some(Duration::from_secs(5)),
        idle_exit: Some(Duration::from_secs(2)),
        debounce: Duration::from_millis(50),
        reconcile_interval: Duration::from_secs(1),
        allow_unbounded: false,
        registry_path: Some(state.path().join("watchers.sqlite")),
    };
    let handle = std::thread::spawn(move || {
        let stop = Arc::new(AtomicBool::new(false));
        let result = watcher::run(&watched_index, &config, stop.as_ref());
        if let Err(error) = &result {
            eprintln!("watcher failed before contract completion: {error:#}");
        }
        result
    });
    std::thread::sleep(Duration::from_millis(300));
    write(workspace.path(), "src/lib.rs", "fn after() {}\n");
    let report = handle.join().unwrap().unwrap();
    assert!(report.unwatched);
    assert_eq!(report.reason, "idle");
    assert!(report.paths_refreshed >= 1);
    assert_eq!(index.definitions("after", None).unwrap().len(), 1);
    assert!(index.definitions("before", None).unwrap().is_empty());
}

#[test]
fn watcher_registry_lists_metadata_tracks_create_rename_delete_and_stops_all() {
    let workspace = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    write(workspace.path(), "src/stable.rs", "fn stable() {}\n");
    let index = CodeIndex::open(workspace.path(), &state.path().join("index.sqlite")).unwrap();
    index.rebuild().unwrap();
    let registry_path = state.path().join("watchers.sqlite");
    let registry = WatcherRegistry::open_at(&registry_path).unwrap();
    let watched_index = index.clone();
    let config = WatchConfig {
        // This test performs three sequential filesystem transitions. Keep the
        // watcher lifetime comfortably above their individual bounded waits,
        // especially on a cold Windows CI runner.
        duration: Some(Duration::from_secs(30)),
        idle_exit: None,
        debounce: Duration::from_millis(30),
        reconcile_interval: Duration::from_secs(2),
        allow_unbounded: false,
        registry_path: Some(registry_path),
    };
    let handle = std::thread::spawn(move || {
        let stop = Arc::new(AtomicBool::new(false));
        let result = watcher::run(&watched_index, &config, stop.as_ref());
        if let Err(error) = &result {
            eprintln!("registry watcher failed before contract completion: {error:#}");
        }
        result
    });

    wait_until("watcher registration", || {
        registry.list().is_ok_and(|list| list.active == 1)
    });
    let listed = registry.list().unwrap();
    let watcher = &listed.watchers[0];
    assert_eq!(
        watcher.root,
        workspace
            .path()
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    );
    assert_eq!(watcher.pid, std::process::id());
    assert_eq!(watcher.status, "active");
    assert!(watcher.deadline_unix_ms.is_some());

    write(workspace.path(), "src/created.rs", "fn created() {}\n");
    wait_until("created file indexed", || {
        index
            .definitions("created", Some("src/created.rs"))
            .is_ok_and(|items| items.len() == 1)
    });
    std::fs::rename(
        workspace.path().join("src/created.rs"),
        workspace.path().join("src/moved.rs"),
    )
    .unwrap();
    wait_until("renamed file reindexed", || {
        index
            .definitions("created", Some("src/moved.rs"))
            .is_ok_and(|items| items.len() == 1)
            && index
                .definitions("created", Some("src/created.rs"))
                .is_ok_and(|items| items.is_empty())
    });
    std::fs::remove_file(workspace.path().join("src/moved.rs")).unwrap();
    wait_until("deleted file removed from index", || {
        index
            .definitions("created", None)
            .is_ok_and(|items| items.is_empty())
    });

    write(
        workspace.path(),
        "src/nested/Watched.kt",
        "fun kotlinWatched(): Int = 1\n",
    );
    wait_until("Kotlin file in created directory indexed", || {
        index
            .definitions("kotlinWatched", None)
            .is_ok_and(|items| items.len() == 1)
    });
    std::fs::remove_dir_all(workspace.path().join("src/nested")).unwrap();
    wait_until("removed directory purged from index", || {
        index
            .definitions("kotlinWatched", None)
            .is_ok_and(|items| items.is_empty())
    });

    let stop = registry.stop_all(false, Duration::from_millis(0)).unwrap();
    assert_eq!(stop.requested, 1);
    let report = handle.join().unwrap().unwrap();
    assert_eq!(report.reason, "registry_stop");
    assert!(report.unwatched);
    assert_eq!(registry.list().unwrap().active, 0);
    assert!(registry.list().unwrap().watchers.is_empty());
}

#[cfg(unix)]
#[test]
fn forced_stop_all_signals_a_separate_watcher_process_and_it_cleans_up() {
    let workspace = TempDir::new().unwrap();
    write(workspace.path(), "src/lib.rs", "fn watched() {}\n");
    let state = workspace.path().join("state");
    let registry_path = state.join("watchers.sqlite");
    let registry = WatcherRegistry::open_at(&registry_path).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_code-mechanic"))
        .env("CODE_MECHANIC_STATE_DIR", &state)
        .args([
            "--root",
            workspace.path().to_str().unwrap(),
            "--database",
            ".index/code-mechanic.sqlite",
            "watch",
            "--duration-seconds",
            "10",
            "--until-idle-seconds",
            "0",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_until("separate watcher registration", || {
        registry.list().is_ok_and(|list| list.active == 1)
    });
    let report = registry.stop_all(true, Duration::ZERO).unwrap();
    assert_eq!(report.requested, 1);
    assert_eq!(report.force_signalled, 1);

    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(5) {
            child.kill().unwrap();
            panic!("forced watcher did not terminate before timeout");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success());
    wait_until("forced watcher registry cleanup", || {
        registry.list().is_ok_and(|list| list.watchers.is_empty())
    });
}

#[test]
fn benchmark_requires_equivalent_answer_and_proves_local_token_reduction() {
    let workspace = TempDir::new().unwrap();
    let mut source = String::new();
    for value in 0..80 {
        writeln!(source, "fn pre_{value}() -> usize {{ {value} }}").unwrap();
    }
    source.push_str("fn measured_target() -> usize {\n    42\n}\n");
    for value in 0..80 {
        writeln!(
            source,
            "fn caller_{value}() -> usize {{ measured_target() + {value} }}"
        )
        .unwrap();
    }
    write(workspace.path(), "src/bench.rs", &source);
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let report = benchmark::run(
        &index,
        &[BenchmarkCase {
            symbol: "measured_target".to_owned(),
            file: Some("src/bench.rs".to_owned()),
        }],
        3,
        120,
        50.0,
    )
    .unwrap();
    assert!(report.passed, "{report:#?}");
    assert!(report.cases[0].answer_equivalent);
    assert!(report.cases[0].locator_exact);
    assert!(report.cases[0].token_reduction_pct >= 50.0);
}

#[test]
fn benchmark_refuses_to_claim_large_savings_when_the_exact_body_dominates() {
    let workspace = TempDir::new().unwrap();
    let mut source = String::from("fn very_large_target() -> usize {\n    let mut total = 0;\n");
    for value in 0..180 {
        writeln!(source, "    total += {value};").unwrap();
    }
    source.push_str("    total\n}\n");
    write(workspace.path(), "src/large.rs", &source);
    let index = index_for(workspace.path());
    index.rebuild().unwrap();
    let report = benchmark::run(
        &index,
        &[BenchmarkCase {
            symbol: "very_large_target".to_owned(),
            file: Some("src/large.rs".to_owned()),
        }],
        2,
        20,
        50.0,
    )
    .unwrap();
    assert!(report.cases[0].answer_equivalent);
    assert!(report.cases[0].locator_exact);
    assert!(!report.passed);
    assert!(report.cases[0].token_reduction_pct < 50.0);
    assert!(report.cases[0].locator_vs_full_source_reduction_pct >= 80.0);
}

fn wait_until(label: &str, mut condition: impl FnMut() -> bool) {
    let started = std::time::Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{label} did not become true before timeout"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
