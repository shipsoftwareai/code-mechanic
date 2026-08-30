//! Guarded AST-addressed mechanical refactors.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;

use crate::index::{CodeIndex, ReferenceRecord, SymbolRecord, unique_definition};
use crate::language::{CallReference, FunctionSymbol, Language, SymbolKind, parse};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RefactorReport {
    pub operation: String,
    pub plan_id: String,
    pub applied: bool,
    pub files_changed: usize,
    pub replacements: usize,
    pub changes: Vec<FileChange>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub before_hash: String,
    pub after_hash: String,
    pub replacements: Vec<Occurrence>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Occurrence {
    pub kind: String,
    pub line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub replacement: String,
}

#[derive(Debug)]
struct PlannedFile {
    path: String,
    language: Language,
    original: String,
    updated: String,
    before_hash: String,
    after_hash: String,
    occurrences: Vec<Occurrence>,
}

#[derive(Debug)]
struct RefactorPlan {
    operation: String,
    plan_id: String,
    files: Vec<PlannedFile>,
}

#[derive(Debug, Clone)]
struct Replacement {
    start: usize,
    end: usize,
    text: String,
    expected: String,
    kind: String,
    line: usize,
}

/// Preview or apply a function rename. The target definition must be unique
/// after the optional file filter; only AST definition/prototype/call spans are
/// edited, so comments, strings, and unrelated identifiers remain untouched.
pub fn rename_function(
    index: &CodeIndex,
    from: &str,
    to: &str,
    file: Option<&str>,
    apply: bool,
    expect_plan: Option<&str>,
) -> Result<RefactorReport> {
    validate_identifier(from)?;
    validate_identifier(to)?;
    if from == to {
        bail!("rename source and destination are identical");
    }

    let target = unique_definition(from, index.definitions(from, file)?)?;
    let language = record_language(&target.language)?;
    let all_definitions = index.definitions(from, None)?;
    if file.is_some() && all_definitions.len() > 1 {
        bail!(
            "`{from}` has multiple workspace definitions; file-scoped selection cannot safely decide which unqualified calls belong to {}; refusing workspace rename",
            target.path
        );
    }
    if !index.definitions(to, None)?.is_empty() {
        bail!("destination function `{to}` already has an indexed definition");
    }

    let mut replacements: BTreeMap<String, Vec<(String, Replacement)>> = BTreeMap::new();
    push_symbol_replacement(&mut replacements, &target, from, to, "definition");
    for symbol in index.symbols(from, None)? {
        if symbol.kind == "prototype" && record_language(&symbol.language)? == language {
            push_symbol_replacement(&mut replacements, &symbol, from, to, "prototype");
        }
    }
    for reference in index.references(from)? {
        if record_language(&reference.language)? == language {
            push_reference_replacement(&mut replacements, &reference, from, to);
        }
    }

    let plan = build_plan(index, format!("rename:{from}->{to}"), replacements)?;
    finish_plan(index, &plan, apply, expect_plan)
}

/// Preview or apply code immediately after a function body's opening brace.
pub fn inject_function_entry(
    index: &CodeIndex,
    symbol: &str,
    code: &str,
    file: Option<&str>,
    apply: bool,
    expect_plan: Option<&str>,
) -> Result<RefactorReport> {
    validate_identifier(symbol)?;
    if code.trim().is_empty() {
        bail!("injected code must not be empty");
    }
    if code.contains('\0') {
        bail!("injected code contains a NUL byte");
    }
    let target = unique_definition(symbol, index.definitions(symbol, file)?)?;
    let body_start = target
        .body_start_byte
        .ok_or_else(|| anyhow!("function `{symbol}` has no indexed body"))?;
    let source = index.fresh_source(&target.path, &target.content_hash, target.parse_ok)?;
    let function_source = source
        .get(target.start_byte..target.end_byte)
        .ok_or_else(|| anyhow!("indexed function range is outside {}", target.path))?;
    if function_source.contains(code.trim()) {
        bail!("function `{symbol}` already contains the requested injection");
    }
    if source.as_bytes().get(body_start) != Some(&b'{') {
        bail!("indexed function body for `{symbol}` does not start with `{{`");
    }

    let base_indent = line_indent(&source, target.start_byte);
    let inner_indent = format!("{base_indent}    ");
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let indented_code = code
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join(&format!("{newline}{inner_indent}"));
    let insert_at = body_start + 1;
    let body_is_multiline = source[insert_at..].starts_with(newline);
    let insertion = if body_is_multiline {
        format!("{newline}{inner_indent}{indented_code}")
    } else {
        format!("{newline}{inner_indent}{indented_code}{newline}{base_indent}")
    };
    let line = source[..insert_at]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let mut replacements = BTreeMap::new();
    replacements.insert(
        target.path.clone(),
        vec![(
            target.content_hash,
            Replacement {
                start: insert_at,
                end: insert_at,
                text: insertion,
                expected: String::new(),
                kind: "function_entry".to_owned(),
                line,
            },
        )],
    );
    let plan = build_plan(index, format!("inject-entry:{symbol}"), replacements)?;
    finish_plan(index, &plan, apply, expect_plan)
}

/// Preview or apply replacement of the statements inside one unique braced
/// function body while preserving the function's signature and braces.
pub fn replace_function_body(
    index: &CodeIndex,
    symbol: &str,
    code: &str,
    file: Option<&str>,
    apply: bool,
    expect_plan: Option<&str>,
) -> Result<RefactorReport> {
    validate_identifier(symbol)?;
    if code.contains('\0') {
        bail!("replacement body contains a NUL byte");
    }
    let target = unique_definition(symbol, index.definitions(symbol, file)?)?;
    let body_start = target
        .body_start_byte
        .ok_or_else(|| anyhow!("function `{symbol}` has no indexed body"))?;
    let body_end = target
        .body_end_byte
        .ok_or_else(|| anyhow!("function `{symbol}` has no indexed body"))?;
    let source = index.fresh_source(&target.path, &target.content_hash, target.parse_ok)?;
    if source.as_bytes().get(body_start) != Some(&b'{')
        || source.as_bytes().get(body_end.saturating_sub(1)) != Some(&b'}')
    {
        bail!("replace-body currently requires a braced function body");
    }
    let replace_start = body_start + 1;
    let replace_end = body_end - 1;
    let existing = source
        .get(replace_start..replace_end)
        .ok_or_else(|| anyhow!("indexed function body is outside {}", target.path))?;
    let base_indent = line_indent(&source, target.start_byte);
    let inner_indent = format!("{base_indent}    ");
    let newline = source_newline(&source);
    let replacement = format_code_block(code, &inner_indent, &base_indent, newline);
    if existing == replacement {
        bail!("function `{symbol}` already has the requested body");
    }
    let mut replacements = BTreeMap::new();
    replacements.insert(
        target.path.clone(),
        vec![(
            target.content_hash,
            Replacement {
                start: replace_start,
                end: replace_end,
                text: replacement,
                expected: existing.to_owned(),
                kind: "function_body".to_owned(),
                line: target.start_line,
            },
        )],
    );
    let plan = build_plan(index, format!("replace-body:{symbol}"), replacements)?;
    finish_plan(index, &plan, apply, expect_plan)
}

/// Preview or apply an appended formal parameter and matching call argument.
/// This is syntax-addressed and intentionally excludes Objective-C selectors.
pub fn append_parameter(
    index: &CodeIndex,
    symbol: &str,
    parameter: &str,
    argument: &str,
    file: Option<&str>,
    apply: bool,
    expect_plan: Option<&str>,
) -> Result<RefactorReport> {
    validate_identifier(symbol)?;
    validate_fragment("parameter", parameter)?;
    validate_fragment("argument", argument)?;
    let target = unique_definition(symbol, index.definitions(symbol, file)?)?;
    let language = record_language(&target.language)?;
    if language == Language::ObjectiveC {
        bail!("append-parameter does not support Objective-C selector syntax");
    }
    let all_definitions = index.definitions(symbol, None)?;
    if file.is_some() && all_definitions.len() > 1 {
        bail!(
            "`{symbol}` has multiple workspace definitions; file-scoped selection cannot safely assign call sites"
        );
    }

    let mut replacements: BTreeMap<String, Vec<(String, Replacement)>> = BTreeMap::new();
    push_parameter_replacement(index, &mut replacements, &target, parameter, language)?;
    for prototype in index.symbols(symbol, None)? {
        if prototype.kind == SymbolKind::Prototype.as_str()
            && record_language(&prototype.language)? == language
        {
            push_parameter_replacement(index, &mut replacements, &prototype, parameter, language)?;
        }
    }
    for reference in index.references(symbol)? {
        if record_language(&reference.language)? == language {
            push_argument_replacement(index, &mut replacements, &reference, argument)?;
        }
    }
    let plan = build_plan(
        index,
        format!("append-parameter:{symbol}:{parameter}:{argument}"),
        replacements,
    )?;
    finish_plan(index, &plan, apply, expect_plan)
}

fn push_symbol_replacement(
    grouped: &mut BTreeMap<String, Vec<(String, Replacement)>>,
    symbol: &SymbolRecord,
    from: &str,
    to: &str,
    kind: &str,
) {
    grouped.entry(symbol.path.clone()).or_default().push((
        symbol.content_hash.clone(),
        Replacement {
            start: symbol.name_start_byte,
            end: symbol.name_end_byte,
            text: to.to_owned(),
            expected: from.to_owned(),
            kind: kind.to_owned(),
            line: symbol.start_line,
        },
    ));
    debug_assert_eq!(symbol.name, from);
}

fn push_reference_replacement(
    grouped: &mut BTreeMap<String, Vec<(String, Replacement)>>,
    reference: &ReferenceRecord,
    from: &str,
    to: &str,
) {
    grouped.entry(reference.path.clone()).or_default().push((
        reference.content_hash.clone(),
        Replacement {
            start: reference.start_byte,
            end: reference.end_byte,
            text: to.to_owned(),
            expected: from.to_owned(),
            kind: "call".to_owned(),
            line: reference.line,
        },
    ));
    debug_assert_eq!(reference.name, from);
}

fn push_parameter_replacement(
    index: &CodeIndex,
    grouped: &mut BTreeMap<String, Vec<(String, Replacement)>>,
    record: &SymbolRecord,
    parameter: &str,
    language: Language,
) -> Result<()> {
    let source = index.fresh_source(&record.path, &record.content_hash, record.parse_ok)?;
    let parsed = parse(language, &source)?;
    let function = matching_function(&parsed.functions, record)?;
    let start = function.parameters_start_byte.ok_or_else(|| {
        anyhow!(
            "{}:{} has no simple parameter list",
            record.path,
            record.start_line
        )
    })?;
    let end = function.parameters_end_byte.ok_or_else(|| {
        anyhow!(
            "{}:{} has no simple parameter list",
            record.path,
            record.start_line
        )
    })?;
    let replacement = append_to_list(&source, start, end, parameter, "parameter", Some(language))?;
    grouped
        .entry(record.path.clone())
        .or_default()
        .push((record.content_hash.clone(), replacement));
    Ok(())
}

fn push_argument_replacement(
    index: &CodeIndex,
    grouped: &mut BTreeMap<String, Vec<(String, Replacement)>>,
    record: &ReferenceRecord,
    argument: &str,
) -> Result<()> {
    let language = record_language(&record.language)?;
    let source = index.fresh_source(&record.path, &record.content_hash, record.parse_ok)?;
    let parsed = parse(language, &source)?;
    let call = matching_call(&parsed.calls, record)?;
    let start = call.arguments_start_byte.ok_or_else(|| {
        anyhow!(
            "{}:{} call has no parenthesized argument list",
            record.path,
            record.line
        )
    })?;
    let end = call.arguments_end_byte.ok_or_else(|| {
        anyhow!(
            "{}:{} call has no parenthesized argument list",
            record.path,
            record.line
        )
    })?;
    let replacement = append_to_list(&source, start, end, argument, "argument", None)?;
    grouped
        .entry(record.path.clone())
        .or_default()
        .push((record.content_hash.clone(), replacement));
    Ok(())
}

fn matching_function<'a>(
    functions: &'a [FunctionSymbol],
    record: &SymbolRecord,
) -> Result<&'a FunctionSymbol> {
    functions
        .iter()
        .find(|function| {
            function.name_start_byte == record.name_start_byte
                && function.name_end_byte == record.name_end_byte
                && function.kind.as_str() == record.kind
        })
        .ok_or_else(|| {
            anyhow!(
                "fresh parse did not reproduce indexed function {}:{}",
                record.path,
                record.start_line
            )
        })
}

fn matching_call<'a>(
    calls: &'a [CallReference],
    record: &ReferenceRecord,
) -> Result<&'a CallReference> {
    calls
        .iter()
        .find(|call| {
            call.start_byte == record.start_byte
                && call.end_byte == record.end_byte
                && call.name == record.name
        })
        .ok_or_else(|| {
            anyhow!(
                "fresh parse did not reproduce indexed call {}:{}",
                record.path,
                record.line
            )
        })
}

fn append_to_list(
    source: &str,
    start: usize,
    end: usize,
    value: &str,
    kind: &str,
    parameter_language: Option<Language>,
) -> Result<Replacement> {
    let list = source
        .get(start..end)
        .ok_or_else(|| anyhow!("indexed {kind} list is outside source"))?;
    if !list.starts_with('(') || !list.ends_with(')') {
        bail!("indexed {kind} list is not parenthesized: `{list}`");
    }
    let interior_start = start + 1;
    let interior_end = end - 1;
    let interior = &source[interior_start..interior_end];
    let trimmed = interior.trim();
    if trimmed
        .split(',')
        .any(|existing| existing.trim() == value.trim())
    {
        bail!("{kind} list already contains `{value}`");
    }
    if trimmed.trim_end_matches(',').trim_end().ends_with("...") {
        bail!("cannot append after a variadic {kind}; it must remain last");
    }
    if parameter_language == Some(Language::Cpp) && trimmed.contains('=') && !value.contains('=') {
        bail!("C++ parameters after a defaulted parameter must also have a default");
    }
    if parameter_language
        .is_some_and(|language| matches!(language, Language::C | Language::Cpp | Language::Glsl))
        && trimmed == "void"
    {
        let offset = interior
            .find("void")
            .expect("trimmed interior contains void");
        return Ok(Replacement {
            start: interior_start + offset,
            end: interior_start + offset + 4,
            text: value.to_owned(),
            expected: "void".to_owned(),
            kind: kind.to_owned(),
            line: source[..interior_start + offset].matches('\n').count() + 1,
        });
    }

    let (replace_start, replace_end, text, expected) = if trimmed.is_empty() {
        if interior.contains('\n') {
            let newline = source_newline(source);
            let closing_indent = line_indent(source, interior_end);
            let item_indent = format!("{closing_indent}    ");
            (
                interior_start,
                interior_end,
                format!("{newline}{item_indent}{value}{newline}{closing_indent}"),
                interior.to_owned(),
            )
        } else {
            (
                interior_start,
                interior_end,
                value.to_owned(),
                interior.to_owned(),
            )
        }
    } else if interior.contains('\n') {
        let content_end = interior.trim_end().len();
        let insert_at = interior_start + content_end;
        let newline = source_newline(source);
        let item_indent = multiline_item_indent(interior);
        let separator = if interior[..content_end].trim_end().ends_with(',') {
            ""
        } else {
            ","
        };
        (
            insert_at,
            insert_at,
            format!("{separator}{newline}{item_indent}{value}"),
            String::new(),
        )
    } else {
        (
            interior_end,
            interior_end,
            if trimmed.ends_with(',') {
                format!(" {value}")
            } else {
                format!(", {value}")
            },
            String::new(),
        )
    };
    Ok(Replacement {
        start: replace_start,
        end: replace_end,
        text,
        expected,
        kind: kind.to_owned(),
        line: source[..replace_start].matches('\n').count() + 1,
    })
}

fn build_plan(
    index: &CodeIndex,
    operation: String,
    grouped: BTreeMap<String, Vec<(String, Replacement)>>,
) -> Result<RefactorPlan> {
    let mut files = Vec::with_capacity(grouped.len());
    for (path, entries) in grouped {
        let expected_hashes: BTreeSet<&str> =
            entries.iter().map(|(hash, _)| hash.as_str()).collect();
        if expected_hashes.len() != 1 {
            bail!("index contains inconsistent snapshots for {path}; re-index before refactoring");
        }
        let expected_hash = expected_hashes.iter().next().expect("one expected hash");
        let source = index.fresh_source(&path, expected_hash, true)?;
        let language = Language::for_path(Path::new(&path))
            .ok_or_else(|| anyhow!("unsupported indexed path {path}"))?;
        let mut replacements: Vec<Replacement> =
            entries.into_iter().map(|(_, item)| item).collect();
        replacements.sort_by_key(|item| (item.start, item.end));
        replacements.dedup_by(|left, right| {
            left.start == right.start && left.end == right.end && left.text == right.text
        });
        ensure_non_overlapping(&path, &replacements)?;

        let mut updated = source.clone();
        for replacement in replacements.iter().rev() {
            let existing = updated
                .get(replacement.start..replacement.end)
                .ok_or_else(|| anyhow!("replacement range is outside {path}"))?;
            if existing != replacement.expected {
                bail!(
                    "indexed replacement range {}..{} in {path} contains unexpected source",
                    replacement.start,
                    replacement.end
                );
            }
            updated.replace_range(replacement.start..replacement.end, &replacement.text);
        }
        let parsed = parse(language, &updated)?;
        if parsed.tree.root_node().has_error() {
            bail!("planned refactor introduces Tree-sitter parse errors in {path}");
        }
        let occurrences = replacements
            .into_iter()
            .map(|item| Occurrence {
                kind: item.kind,
                line: item.line,
                start_byte: item.start,
                end_byte: item.end,
                replacement: item.text,
            })
            .collect();
        files.push(PlannedFile {
            path,
            language,
            before_hash: hash_text(&source),
            after_hash: hash_text(&updated),
            original: source,
            updated,
            occurrences,
        });
    }
    if files.is_empty() {
        bail!("refactor produced no AST-addressed replacements");
    }
    let plan_id = compute_plan_id(&operation, &files);
    Ok(RefactorPlan {
        operation,
        plan_id,
        files,
    })
}

fn finish_plan(
    index: &CodeIndex,
    plan: &RefactorPlan,
    apply: bool,
    expect_plan: Option<&str>,
) -> Result<RefactorReport> {
    if apply {
        let expected = expect_plan
            .ok_or_else(|| anyhow!("--apply requires --expect-plan from a fresh preview"))?;
        if expected != plan.plan_id {
            bail!(
                "plan mismatch: expected {expected}, freshly computed {}; preview again",
                plan.plan_id
            );
        }
        apply_files(index.root(), plan)?;
        for file in &plan.files {
            index.refresh_path(Path::new(&file.path))?;
        }
    } else if expect_plan.is_some() {
        bail!("--expect-plan is only valid with --apply");
    }
    Ok(report(plan, apply))
}

fn report(plan: &RefactorPlan, applied: bool) -> RefactorReport {
    let replacements = plan.files.iter().map(|file| file.occurrences.len()).sum();
    RefactorReport {
        operation: plan.operation.clone(),
        plan_id: plan.plan_id.clone(),
        applied,
        files_changed: plan.files.len(),
        replacements,
        changes: plan
            .files
            .iter()
            .map(|file| FileChange {
                path: file.path.clone(),
                before_hash: file.before_hash.clone(),
                after_hash: file.after_hash.clone(),
                replacements: file.occurrences.clone(),
            })
            .collect(),
    }
}

fn apply_files(root: &Path, plan: &RefactorPlan) -> Result<()> {
    let suffix = &plan.plan_id[..16];
    let mut staged = Vec::with_capacity(plan.files.len());
    for file in &plan.files {
        let absolute = root.join(&file.path);
        let live = std::fs::read_to_string(&absolute)
            .with_context(|| format!("re-read {} before apply", absolute.display()))?;
        if hash_text(&live) != file.before_hash || live != file.original {
            cleanup_staged(&staged);
            bail!("{} changed after preview; no files were applied", file.path);
        }
        let temporary = sibling_path(&absolute, &format!("code-mechanic-{suffix}.new"))?;
        let backup = sibling_path(&absolute, &format!("code-mechanic-{suffix}.bak"))?;
        if temporary.exists() || backup.exists() {
            cleanup_staged(&staged);
            bail!(
                "staging path already exists beside {}; refusing overwrite",
                file.path
            );
        }
        let permissions = std::fs::metadata(&absolute)?.permissions();
        let mut handle = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| format!("stage refactor for {}", file.path))?;
        handle.write_all(file.updated.as_bytes())?;
        handle.sync_all()?;
        std::fs::set_permissions(&temporary, permissions)?;
        staged.push((absolute, temporary, backup));
    }

    for (committed, (absolute, temporary, backup)) in staged.iter().enumerate() {
        if let Err(error) =
            std::fs::rename(absolute, backup).and_then(|()| std::fs::rename(temporary, absolute))
        {
            rollback(&staged, committed);
            return Err(error)
                .with_context(|| format!("atomically replace {}", absolute.display()));
        }
    }
    for (_, _, backup) in &staged {
        std::fs::remove_file(backup)
            .with_context(|| format!("remove successful refactor backup {}", backup.display()))?;
    }
    Ok(())
}

fn rollback(staged: &[(PathBuf, PathBuf, PathBuf)], committed: usize) {
    for (index, (absolute, temporary, backup)) in staged.iter().enumerate().rev() {
        if index < committed {
            let _ = std::fs::remove_file(absolute);
            let _ = std::fs::rename(backup, absolute);
        } else if backup.exists() && !absolute.exists() {
            let _ = std::fs::rename(backup, absolute);
        }
        let _ = std::fs::remove_file(temporary);
    }
}

fn cleanup_staged(staged: &[(PathBuf, PathBuf, PathBuf)]) {
    for (_, temporary, _) in staged {
        let _ = std::fs::remove_file(temporary);
    }
}

fn sibling_path(path: &Path, suffix: &str) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("source path has no filename: {}", path.display()))?
        .to_string_lossy();
    Ok(path.with_file_name(format!(".{name}.{suffix}")))
}

fn compute_plan_id(operation: &str, files: &[PlannedFile]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(operation.as_bytes());
    for file in files {
        hasher.update(file.path.as_bytes());
        hasher.update(file.before_hash.as_bytes());
        hasher.update(file.after_hash.as_bytes());
        hasher.update(file.language.as_str().as_bytes());
        for occurrence in &file.occurrences {
            hasher.update(&occurrence.start_byte.to_le_bytes());
            hasher.update(&occurrence.end_byte.to_le_bytes());
            hasher.update(occurrence.replacement.as_bytes());
        }
    }
    hasher.finalize().to_hex().to_string()
}

fn ensure_non_overlapping(path: &str, replacements: &[Replacement]) -> Result<()> {
    for pair in replacements.windows(2) {
        if pair[0].end > pair[1].start {
            bail!("overlapping AST replacements in {path}; refusing plan");
        }
    }
    Ok(())
}

fn record_language(value: &str) -> Result<Language> {
    match value {
        "rust" => Ok(Language::Rust),
        "c" => Ok(Language::C),
        "cpp" => Ok(Language::Cpp),
        "go" => Ok(Language::Go),
        "objective-c" => Ok(Language::ObjectiveC),
        "glsl" => Ok(Language::Glsl),
        _ => bail!("unsupported indexed language `{value}`"),
    }
}

fn validate_identifier(value: &str) -> Result<()> {
    if !is_identifier(value) {
        bail!("`{value}` is not a portable static-language identifier");
    }
    Ok(())
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn line_indent(source: &str, byte: usize) -> String {
    let line_start = source[..byte].rfind('\n').map_or(0, |index| index + 1);
    source[line_start..]
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .collect()
}

fn source_newline(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn multiline_item_indent(interior: &str) -> String {
    interior
        .lines()
        .find(|line| !line.trim().is_empty())
        .map_or_else(
            || "    ".to_owned(),
            |line| {
                line.chars()
                    .take_while(|character| matches!(character, ' ' | '\t'))
                    .collect()
            },
        )
}

fn format_code_block(code: &str, inner_indent: &str, base_indent: &str, newline: &str) -> String {
    let trimmed = code.trim_matches(['\r', '\n']);
    if trimmed.trim().is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    let common_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    let normalized = lines
        .iter()
        .map(|line| line.get(common_indent..).unwrap_or(line).trim_end())
        .collect::<Vec<_>>()
        .join(&format!("{newline}{inner_indent}"));
    format!("{newline}{inner_indent}{normalized}{newline}{base_indent}")
}

fn validate_fragment(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    if value.contains('\0') || value.contains(['\r', '\n']) {
        bail!("{label} must be one line and contain no NUL byte");
    }
    Ok(())
}

fn hash_text(source: &str) -> String {
    blake3::hash(source.as_bytes()).to_hex().to_string()
}
