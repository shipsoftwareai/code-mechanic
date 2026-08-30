//! Static-language structural indexing and guarded mechanical refactors for coding agents.

#![allow(clippy::missing_errors_doc)]

pub mod benchmark;
pub mod index;
pub mod language;
pub mod query;
pub mod refactor;
pub mod watch_registry;
pub mod watcher;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Resolve a workspace root without accepting a file path.
pub fn canonical_root(root: &Path) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .with_context(|| format!("canonicalize workspace root {}", root.display()))?;
    if !root.is_dir() {
        bail!("workspace root is not a directory: {}", root.display());
    }
    Ok(root)
}

/// Default disposable index location below the selected workspace root.
#[must_use]
pub fn default_database_path(root: &Path) -> PathBuf {
    root.join(".code-mechanic/index.sqlite")
}
