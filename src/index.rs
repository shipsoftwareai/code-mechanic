//! Persistent `SQLite` index over the common Rust/C structural schema.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::Metadata;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, anyhow, bail};
use ignore::WalkBuilder;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::canonical_root;
use crate::language::{Language, SymbolKind, parse};

const SCHEMA_VERSION: i64 = 1;

/// Persistent index entrypoint. Connections are deliberately short-lived so
/// agent CLI invocations never need a resident daemon.
#[derive(Debug, Clone)]
pub struct CodeIndex {
    root: PathBuf,
    database: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IndexSummary {
    pub root: String,
    pub database: String,
    pub files: usize,
    pub symbols: usize,
    pub references: usize,
    pub parse_failures: usize,
    pub reparsed: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SymbolRecord {
    pub path: String,
    pub language: String,
    pub name: String,
    pub kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub name_start_byte: usize,
    pub name_end_byte: usize,
    pub body_start_byte: Option<usize>,
    pub body_end_byte: Option<usize>,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(skip_serializing)]
    pub content_hash: String,
    #[serde(skip_serializing)]
    pub parse_ok: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReferenceRecord {
    pub path: String,
    pub language: String,
    pub name: String,
    pub kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub line: usize,
    #[serde(skip_serializing)]
    pub content_hash: String,
    #[serde(skip_serializing)]
    pub parse_ok: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SymbolSource {
    pub path: String,
    pub language: String,
    pub name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content_hash: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ParseDiagnostic {
    pub path: String,
    pub language: String,
    pub error: String,
}

#[derive(Debug)]
struct ParsedFile {
    relative: String,
    language: Language,
    bytes: usize,
    modified_ns: i64,
    hash: String,
    parse_ok: bool,
    error: Option<String>,
    functions: Vec<crate::language::FunctionSymbol>,
    calls: Vec<crate::language::CallReference>,
}

#[derive(Debug, Clone)]
struct FileFingerprint {
    bytes: usize,
    modified_ns: i64,
    hash: String,
}

impl CodeIndex {
    pub fn open(root: &Path, database: &Path) -> Result<Self> {
        let root = canonical_root(root)?;
        let database = if database.is_absolute() {
            database.to_path_buf()
        } else {
            root.join(database)
        };
        if let Some(parent) = database.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create index directory {}", parent.display()))?;
        }
        let index = Self { root, database };
        let connection = index.connection()?;
        initialise_schema(&connection)?;
        Ok(index)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn database(&self) -> &Path {
        &self.database
    }

    /// Rebuild the complete derived index in one `SQLite` transaction.
    pub fn rebuild(&self) -> Result<IndexSummary> {
        let paths = self.source_paths()?;
        let parsed = paths
            .iter()
            .map(|path| self.parse_file(path))
            .collect::<Result<Vec<_>>>()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().context("begin index rebuild")?;
        transaction.execute_batch("DELETE FROM refs; DELETE FROM symbols; DELETE FROM files;")?;
        for file in &parsed {
            write_parsed_file(&transaction, file)?;
        }
        transaction.commit().context("commit index rebuild")?;
        self.summary(parsed.len(), 0)
    }

    /// Reconcile the persistent index with disk. `force_hash` is used after
    /// watcher overflow; normal periodic reconciliation hashes only files whose
    /// size or mtime changed.
    pub fn reconcile(&self, force_hash: bool) -> Result<IndexSummary> {
        let current_paths = self.source_paths()?;
        let stored = self.file_fingerprints()?;
        let mut current_rel = BTreeSet::new();
        let mut changed = Vec::new();

        for path in &current_paths {
            // The walker may yield a path that is renamed before this loop
            // reaches it. Preserve its safe lexical identity so a later
            // NotFound can be treated as removal rather than killing a watcher.
            let relative = self.relative_string_lexical(path)?;
            let metadata = match std::fs::metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("read metadata for {}", path.display()));
                }
            };
            current_rel.insert(relative.clone());
            let bytes =
                usize::try_from(metadata.len()).context("source file length exceeds usize")?;
            let modified_ns = modified_ns(&metadata);
            let needs_hash = force_hash
                || stored
                    .get(&relative)
                    .is_none_or(|old| old.bytes != bytes || old.modified_ns != modified_ns);
            if !needs_hash {
                continue;
            }
            if force_hash {
                let bytes_on_disk = match std::fs::read(path) {
                    Ok(bytes) => bytes,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        current_rel.remove(&relative);
                        continue;
                    }
                    Err(error) => {
                        return Err(error)
                            .with_context(|| format!("read source {}", path.display()));
                    }
                };
                let hash = content_hash(&bytes_on_disk);
                if stored.get(&relative).is_some_and(|old| old.hash == hash) {
                    continue;
                }
            }
            match self.parse_file(path) {
                Ok(parsed) => changed.push(parsed),
                Err(error) if is_not_found(&error) => {
                    current_rel.remove(&relative);
                }
                Err(error) => return Err(error),
            }
        }

        let removed: Vec<String> = stored
            .keys()
            .filter(|path| !current_rel.contains(*path))
            .cloned()
            .collect();
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .context("begin index reconciliation")?;
        for relative in &removed {
            transaction.execute("DELETE FROM files WHERE path = ?1", [relative])?;
        }
        for file in &changed {
            write_parsed_file(&transaction, file)?;
        }
        transaction
            .commit()
            .context("commit index reconciliation")?;
        self.summary(changed.len(), removed.len())
    }

    /// Re-index exactly one event path. Unsupported, ignored, missing, and
    /// out-of-root paths remove any stale row rather than being followed.
    pub fn refresh_path(&self, path: &Path) -> Result<()> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let Ok(relative) = self.relative_string_lexical(&absolute) else {
            return Ok(());
        };
        let should_index =
            absolute.is_file() && Language::for_path(&absolute).is_some() && !is_symlink(&absolute);
        let parsed = if should_index {
            match self.parse_file(&absolute) {
                Ok(parsed) => Some(parsed),
                Err(error) if is_not_found(&error) => None,
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let mut connection = self.connection()?;
        let transaction = connection.transaction().context("begin path refresh")?;
        if let Some(parsed) = parsed {
            write_parsed_file(&transaction, &parsed)?;
        } else {
            transaction.execute("DELETE FROM files WHERE path = ?1", [&relative])?;
        }
        transaction.commit().context("commit path refresh")?;
        Ok(())
    }

    pub fn summary(&self, reparsed: usize, removed: usize) -> Result<IndexSummary> {
        let connection = self.connection()?;
        let files = count(&connection, "files")?;
        let symbols = count(&connection, "symbols")?;
        let references = count(&connection, "refs")?;
        let parse_failures_raw: i64 = connection
            .query_row("SELECT COUNT(*) FROM files WHERE parse_ok = 0", [], |row| {
                row.get(0)
            })
            .context("count parse failures")?;
        let parse_failures = usize::try_from(parse_failures_raw).context("negative parse count")?;
        Ok(IndexSummary {
            root: self.root.display().to_string(),
            database: self.database.display().to_string(),
            files,
            symbols,
            references,
            parse_failures,
            reparsed,
            removed,
        })
    }

    pub fn diagnostics(&self) -> Result<Vec<ParseDiagnostic>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT path, language, COALESCE(error, 'unknown parse failure') \
             FROM files WHERE parse_ok = 0 ORDER BY path",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ParseDiagnostic {
                path: row.get(0)?,
                language: row.get(1)?,
                error: row.get(2)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("list parse diagnostics")
    }

    pub fn definitions(&self, name: &str, file: Option<&str>) -> Result<Vec<SymbolRecord>> {
        let mut records = self.symbols(name, file)?;
        records.retain(|record| record.kind == SymbolKind::Definition.as_str());
        Ok(records)
    }

    pub fn symbols(&self, name: &str, file: Option<&str>) -> Result<Vec<SymbolRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT s.path, f.language, s.name, s.kind, s.start_byte, s.end_byte, \
             s.name_start_byte, s.name_end_byte, s.body_start_byte, s.body_end_byte, \
             s.start_line, s.end_line, f.content_hash, f.parse_ok \
             FROM symbols s JOIN files f ON f.path = s.path \
             WHERE s.name = ?1 AND (?2 IS NULL OR s.path = ?2) \
             ORDER BY s.path, s.start_byte",
        )?;
        let rows = statement.query_map(params![name, file], symbol_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read indexed symbols")
    }

    pub fn outline(&self, file: &str) -> Result<Vec<SymbolRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT s.path, f.language, s.name, s.kind, s.start_byte, s.end_byte, \
             s.name_start_byte, s.name_end_byte, s.body_start_byte, s.body_end_byte, \
             s.start_line, s.end_line, f.content_hash, f.parse_ok \
             FROM symbols s JOIN files f ON f.path = s.path \
             WHERE s.path = ?1 ORDER BY s.start_byte",
        )?;
        let rows = statement.query_map([file], symbol_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read file outline")
    }

    pub fn references(&self, name: &str) -> Result<Vec<ReferenceRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT r.path, f.language, r.name, r.kind, r.start_byte, r.end_byte, r.line, \
             f.content_hash, f.parse_ok FROM refs r JOIN files f ON f.path = r.path \
             WHERE r.name = ?1 ORDER BY r.path, r.start_byte",
        )?;
        let rows = statement.query_map([name], |row| {
            Ok(ReferenceRecord {
                path: row.get(0)?,
                language: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                start_byte: row_usize(row, 4)?,
                end_byte: row_usize(row, 5)?,
                line: row_usize(row, 6)?,
                content_hash: row.get(7)?,
                parse_ok: row.get(8)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("read indexed references")
    }

    pub fn read_symbol(&self, name: &str, file: Option<&str>) -> Result<SymbolSource> {
        let definitions = self.definitions(name, file)?;
        let target = unique_definition(name, definitions)?;
        let source = self.fresh_source(&target.path, &target.content_hash, target.parse_ok)?;
        let body = source
            .get(target.start_byte..target.end_byte)
            .ok_or_else(|| anyhow!("indexed byte range is outside {}", target.path))?;
        Ok(SymbolSource {
            path: target.path,
            language: target.language,
            name: target.name,
            kind: target.kind,
            start_line: target.start_line,
            end_line: target.end_line,
            content_hash: target.content_hash,
            source: body.to_owned(),
        })
    }

    pub fn fresh_source(
        &self,
        relative: &str,
        expected_hash: &str,
        parse_ok: bool,
    ) -> Result<String> {
        if !parse_ok {
            bail!("{relative} has Tree-sitter parse errors; refusing structural use");
        }
        let absolute = self.safe_absolute(relative)?;
        let bytes = std::fs::read(&absolute)
            .with_context(|| format!("read indexed source {}", absolute.display()))?;
        let live_hash = content_hash(&bytes);
        if live_hash != expected_hash {
            bail!(
                "stale index for {relative}: indexed {expected_hash}, live {live_hash}; run `code-mechanic index` or keep a bounded watcher active"
            );
        }
        String::from_utf8(bytes).with_context(|| format!("source is not UTF-8: {relative}"))
    }

    pub fn indexed_files(&self) -> Result<Vec<String>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT path FROM files ORDER BY path")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("list indexed files")
    }

    pub fn file_hash(&self, relative: &str) -> Result<Option<String>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT content_hash FROM files WHERE path = ?1",
                [relative],
                |row| row.get(0),
            )
            .optional()
            .context("read indexed file hash")
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.database)
            .with_context(|| format!("open SQLite index {}", self.database.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
        )?;
        Ok(connection)
    }

    fn source_paths(&self) -> Result<Vec<PathBuf>> {
        let mut builder = WalkBuilder::new(&self.root);
        builder
            .follow_links(false)
            .standard_filters(true)
            .hidden(false);
        let mut paths = Vec::new();
        for entry in builder.build() {
            let entry = entry.context("walk workspace source tree")?;
            let path = entry.path();
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && Language::for_path(path).is_some()
            {
                paths.push(path.to_path_buf());
            }
        }
        paths.sort();
        Ok(paths)
    }

    fn parse_file(&self, absolute: &Path) -> Result<ParsedFile> {
        let relative = self.relative_string(absolute)?;
        let language = Language::for_path(absolute)
            .ok_or_else(|| anyhow!("unsupported source extension: {}", absolute.display()))?;
        let bytes = std::fs::read(absolute)
            .with_context(|| format!("read source {}", absolute.display()))?;
        let metadata = std::fs::metadata(absolute)
            .with_context(|| format!("read source metadata {}", absolute.display()))?;
        let hash = content_hash(&bytes);
        let source = match std::str::from_utf8(&bytes) {
            Ok(source) => source,
            Err(error) => {
                return Ok(ParsedFile {
                    relative,
                    language,
                    bytes: bytes.len(),
                    modified_ns: modified_ns(&metadata),
                    hash,
                    parse_ok: false,
                    error: Some(format!("non-UTF-8 source: {error}")),
                    functions: Vec::new(),
                    calls: Vec::new(),
                });
            }
        };
        let parsed = parse(language, source)?;
        let parse_ok = !parsed.tree.root_node().has_error();
        Ok(ParsedFile {
            relative,
            language,
            bytes: bytes.len(),
            modified_ns: modified_ns(&metadata),
            hash,
            parse_ok,
            error: (!parse_ok).then(|| "Tree-sitter parse contains error nodes".to_owned()),
            functions: if parse_ok {
                parsed.functions
            } else {
                Vec::new()
            },
            calls: if parse_ok { parsed.calls } else { Vec::new() },
        })
    }

    fn file_fingerprints(&self) -> Result<BTreeMap<String, FileFingerprint>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT path, bytes, modified_ns, content_hash FROM files ORDER BY path")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get(0)?,
                FileFingerprint {
                    bytes: row_usize(row, 1)?,
                    modified_ns: row.get(2)?,
                    hash: row.get(3)?,
                },
            ))
        })?;
        rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .context("read file fingerprints")
    }

    fn relative_string(&self, absolute: &Path) -> Result<String> {
        let canonical = absolute
            .canonicalize()
            .with_context(|| format!("canonicalize source {}", absolute.display()))?;
        if !canonical.starts_with(&self.root) {
            bail!("source escapes workspace root: {}", absolute.display());
        }
        self.relative_string_lexical(&canonical)
    }

    fn relative_string_lexical(&self, absolute: &Path) -> Result<String> {
        let relative = absolute
            .strip_prefix(&self.root)
            .with_context(|| format!("path is outside workspace: {}", absolute.display()))?;
        if relative
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        {
            bail!("unsafe workspace-relative path: {}", relative.display());
        }
        Ok(relative.to_string_lossy().replace('\\', "/"))
    }

    fn safe_absolute(&self, relative: &str) -> Result<PathBuf> {
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        {
            bail!("unsafe indexed path: {relative}");
        }
        let absolute = self.root.join(path);
        let canonical = absolute
            .canonicalize()
            .with_context(|| format!("canonicalize indexed path {relative}"))?;
        if !canonical.starts_with(&self.root) || is_symlink(&absolute) {
            bail!("indexed path escapes the workspace through a symlink: {relative}");
        }
        Ok(canonical)
    }
}

pub fn unique_definition(name: &str, definitions: Vec<SymbolRecord>) -> Result<SymbolRecord> {
    match definitions.len() {
        0 => bail!("no indexed function definition named `{name}`"),
        1 => definitions
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("definition count changed unexpectedly")),
        count => {
            let locations = definitions
                .iter()
                .map(|item| format!("{}:{}", item.path, item.start_line))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("ambiguous function `{name}` has {count} definitions: {locations}; pass --file")
        }
    }
}

fn initialise_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);\
         CREATE TABLE IF NOT EXISTS files (\
           path TEXT PRIMARY KEY, language TEXT NOT NULL, bytes INTEGER NOT NULL,\
           modified_ns INTEGER NOT NULL, content_hash TEXT NOT NULL,\
           parse_ok INTEGER NOT NULL, error TEXT\
         );\
         CREATE TABLE IF NOT EXISTS symbols (\
           id INTEGER PRIMARY KEY, path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,\
           name TEXT NOT NULL, kind TEXT NOT NULL, start_byte INTEGER NOT NULL,\
           end_byte INTEGER NOT NULL, name_start_byte INTEGER NOT NULL, name_end_byte INTEGER NOT NULL,\
           body_start_byte INTEGER, body_end_byte INTEGER, start_line INTEGER NOT NULL,\
           end_line INTEGER NOT NULL\
         );\
         CREATE INDEX IF NOT EXISTS symbols_name_idx ON symbols(name);\
         CREATE INDEX IF NOT EXISTS symbols_path_idx ON symbols(path);\
         CREATE TABLE IF NOT EXISTS refs (\
           id INTEGER PRIMARY KEY, path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,\
           name TEXT NOT NULL, kind TEXT NOT NULL, start_byte INTEGER NOT NULL,\
           end_byte INTEGER NOT NULL, line INTEGER NOT NULL\
         );\
         CREATE INDEX IF NOT EXISTS refs_name_idx ON refs(name);\
         CREATE INDEX IF NOT EXISTS refs_path_idx ON refs(path);",
    )?;
    let stored: Option<i64> = connection
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match stored {
        None => {
            connection.execute(
                "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)",
                [SCHEMA_VERSION],
            )?;
        }
        Some(version) if version == SCHEMA_VERSION => {}
        Some(version) => bail!(
            "unsupported code-mechanic index schema {version}; remove the derived database and re-index"
        ),
    }
    Ok(())
}

fn write_parsed_file(transaction: &Transaction<'_>, file: &ParsedFile) -> Result<()> {
    transaction.execute("DELETE FROM files WHERE path = ?1", [&file.relative])?;
    transaction.execute(
        "INSERT INTO files(path, language, bytes, modified_ns, content_hash, parse_ok, error)\
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            file.relative,
            file.language.as_str(),
            sql_i64(file.bytes)?,
            file.modified_ns,
            file.hash,
            file.parse_ok,
            file.error,
        ],
    )?;
    for symbol in &file.functions {
        transaction.execute(
            "INSERT INTO symbols(path, name, kind, start_byte, end_byte, name_start_byte,\
             name_end_byte, body_start_byte, body_end_byte, start_line, end_line)\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                file.relative,
                symbol.name,
                symbol.kind.as_str(),
                sql_i64(symbol.start_byte)?,
                sql_i64(symbol.end_byte)?,
                sql_i64(symbol.name_start_byte)?,
                sql_i64(symbol.name_end_byte)?,
                symbol.body_start_byte.map(sql_i64).transpose()?,
                symbol.body_end_byte.map(sql_i64).transpose()?,
                sql_i64(symbol.start_line)?,
                sql_i64(symbol.end_line)?,
            ],
        )?;
    }
    for reference in &file.calls {
        transaction.execute(
            "INSERT INTO refs(path, name, kind, start_byte, end_byte, line)\
             VALUES (?1, ?2, 'call', ?3, ?4, ?5)",
            params![
                file.relative,
                reference.name,
                sql_i64(reference.start_byte)?,
                sql_i64(reference.end_byte)?,
                sql_i64(reference.line)?,
            ],
        )?;
    }
    Ok(())
}

fn symbol_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolRecord> {
    Ok(SymbolRecord {
        path: row.get(0)?,
        language: row.get(1)?,
        name: row.get(2)?,
        kind: row.get(3)?,
        start_byte: row_usize(row, 4)?,
        end_byte: row_usize(row, 5)?,
        name_start_byte: row_usize(row, 6)?,
        name_end_byte: row_usize(row, 7)?,
        body_start_byte: row_optional_usize(row, 8)?,
        body_end_byte: row_optional_usize(row, 9)?,
        start_line: row_usize(row, 10)?,
        end_line: row_usize(row, 11)?,
        content_hash: row.get(12)?,
        parse_ok: row.get(13)?,
    })
}

fn count(connection: &Connection, table: &str) -> Result<usize> {
    if !matches!(table, "files" | "symbols" | "refs") {
        bail!("invalid count table {table}");
    }
    let value: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .with_context(|| format!("count {table}"))?;
    usize::try_from(value).with_context(|| format!("negative {table} count"))
}

fn row_usize(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<usize> {
    let value: i64 = row.get(index)?;
    usize::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn row_optional_usize(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<usize>> {
    let value: Option<i64> = row.get(index)?;
    value
        .map(|item| {
            usize::try_from(item).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn sql_i64(value: usize) -> Result<i64> {
    i64::try_from(value).context("source offset exceeds SQLite INTEGER")
}

fn content_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn modified_ns(metadata: &Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .and_then(|value| i64::try_from(value.as_nanos()).ok())
        .unwrap_or_default()
}

fn is_not_found(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_error| io_error.kind() == std::io::ErrorKind::NotFound)
    })
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}
