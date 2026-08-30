//! Per-user registry for inspecting and stopping foreground watcher processes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

const ACTIVE_HEARTBEAT_AGE: Duration = Duration::from_secs(5);
const STALE_RETENTION: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct WatcherRegistry {
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RegistrationMetadata<'a> {
    pub root: &'a Path,
    pub database: &'a Path,
    pub duration: Option<Duration>,
    pub idle_exit: Option<Duration>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WatcherRecord {
    pub watcher_id: String,
    pub pid: u32,
    pub root: String,
    pub database: String,
    pub started_unix_ms: i64,
    pub heartbeat_unix_ms: i64,
    pub deadline_unix_ms: Option<i64>,
    pub idle_exit_ms: Option<i64>,
    pub stop_requested: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WatcherList {
    pub registry: String,
    pub active: usize,
    pub stale: usize,
    pub watchers: Vec<WatcherRecord>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StopAllReport {
    pub registry: String,
    pub requested: usize,
    pub stale_pruned: usize,
    pub force_signalled: usize,
    pub remaining_active: usize,
}

#[derive(Debug)]
pub struct WatchRegistration {
    registry: WatcherRegistry,
    watcher_id: String,
    finished: bool,
}

impl WatcherRegistry {
    pub fn open_default() -> Result<Self> {
        let path = default_registry_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("create watcher registry directory {}", parent.display())
            })?;
            restrict_directory(parent)?;
        }
        Self::open_at(&path)
    }

    pub fn open_at(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("create watcher registry directory {}", parent.display())
            })?;
        }
        let registry = Self {
            path: path.to_path_buf(),
        };
        let connection = registry.connection()?;
        initialise_schema(&connection)?;
        restrict_file(path)?;
        Ok(registry)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn register(&self, metadata: &RegistrationMetadata<'_>) -> Result<WatchRegistration> {
        let now = unix_ms()?;
        let pid = std::process::id();
        let mut identity = blake3::Hasher::new();
        identity.update(&pid.to_le_bytes());
        identity.update(&now.to_le_bytes());
        identity.update(metadata.root.as_os_str().as_encoded_bytes());
        identity.update(metadata.database.as_os_str().as_encoded_bytes());
        let watcher_id = identity.finalize().to_hex()[..16].to_owned();
        let duration_ms = optional_duration_ms(metadata.duration)?;
        let deadline = duration_ms.and_then(|duration| now.checked_add(duration));
        let idle_exit_ms = optional_duration_ms(metadata.idle_exit)?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO watchers( \
               watcher_id, pid, root, database, started_unix_ms, heartbeat_unix_ms, \
               deadline_unix_ms, idle_exit_ms, stop_requested \
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, 0)",
            params![
                watcher_id,
                i64::from(pid),
                metadata.root.display().to_string(),
                metadata.database.display().to_string(),
                now,
                deadline,
                idle_exit_ms,
            ],
        )?;
        Ok(WatchRegistration {
            registry: self.clone(),
            watcher_id,
            finished: false,
        })
    }

    pub fn list(&self) -> Result<WatcherList> {
        let now = unix_ms()?;
        let active_after = now - duration_ms(ACTIVE_HEARTBEAT_AGE)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT watcher_id, pid, root, database, started_unix_ms, heartbeat_unix_ms, \
             deadline_unix_ms, idle_exit_ms, stop_requested \
             FROM watchers ORDER BY root, started_unix_ms",
        )?;
        let rows = statement.query_map([], |row| {
            let heartbeat: i64 = row.get(5)?;
            Ok(WatcherRecord {
                watcher_id: row.get(0)?,
                pid: row_u32(row, 1)?,
                root: row.get(2)?,
                database: row.get(3)?,
                started_unix_ms: row.get(4)?,
                heartbeat_unix_ms: heartbeat,
                deadline_unix_ms: row.get(6)?,
                idle_exit_ms: row.get(7)?,
                stop_requested: row.get(8)?,
                status: if heartbeat >= active_after {
                    "active".to_owned()
                } else {
                    "stale".to_owned()
                },
            })
        })?;
        let watchers = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("list registered watchers")?;
        let active = watchers
            .iter()
            .filter(|watcher| watcher.status == "active")
            .count();
        Ok(WatcherList {
            registry: self.path.display().to_string(),
            active,
            stale: watchers.len() - active,
            watchers,
        })
    }

    pub fn prune_stale(&self) -> Result<usize> {
        let cutoff = unix_ms()? - duration_ms(STALE_RETENTION)?;
        let connection = self.connection()?;
        connection
            .execute(
                "DELETE FROM watchers WHERE heartbeat_unix_ms < ?1",
                [cutoff],
            )
            .context("prune stale watcher registrations")
    }

    /// Request cooperative shutdown for every live registration. With `force`,
    /// any process still heartbeating after the grace period receives SIGTERM
    /// (or `taskkill` on Windows).
    pub fn stop_all(&self, force: bool, grace: Duration) -> Result<StopAllReport> {
        let stale_pruned = self.prune_stale()?;
        let active_after = unix_ms()? - duration_ms(ACTIVE_HEARTBEAT_AGE)?;
        let connection = self.connection()?;
        let requested = connection.execute(
            "UPDATE watchers SET stop_requested = 1 WHERE heartbeat_unix_ms >= ?1",
            [active_after],
        )?;
        drop(connection);

        let mut force_signalled = 0usize;
        if force && requested > 0 {
            std::thread::sleep(grace.min(Duration::from_secs(5)));
            let candidates = self.force_candidates()?;
            for pid in candidates {
                if signal_process(pid)? {
                    force_signalled += 1;
                }
            }
        }
        let remaining_active = self.list()?.active;
        Ok(StopAllReport {
            registry: self.path.display().to_string(),
            requested,
            stale_pruned,
            force_signalled,
            remaining_active,
        })
    }

    fn force_candidates(&self) -> Result<BTreeSet<u32>> {
        let active_after = unix_ms()? - duration_ms(ACTIVE_HEARTBEAT_AGE)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT pid FROM watchers \
             WHERE stop_requested = 1 AND heartbeat_unix_ms >= ?1",
        )?;
        let rows = statement.query_map([active_after], |row| row_u32(row, 0))?;
        rows.collect::<rusqlite::Result<BTreeSet<_>>>()
            .context("read watcher processes awaiting forced shutdown")
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)
            .with_context(|| format!("open watcher registry {}", self.path.display()))?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
        )?;
        Ok(connection)
    }
}

impl WatchRegistration {
    pub fn heartbeat_and_should_stop(&self) -> Result<bool> {
        let now = unix_ms()?;
        let connection = self.registry.connection()?;
        let requested = connection
            .query_row(
                "UPDATE watchers SET heartbeat_unix_ms = ?2 WHERE watcher_id = ?1 \
                 RETURNING stop_requested",
                params![self.watcher_id, now],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow!("watcher registration disappeared during execution"))?;
        Ok(requested)
    }

    pub fn finish(&mut self) -> Result<()> {
        if !self.finished {
            let connection = self.registry.connection()?;
            connection.execute(
                "DELETE FROM watchers WHERE watcher_id = ?1",
                [&self.watcher_id],
            )?;
            self.finished = true;
        }
        Ok(())
    }
}

impl Drop for WatchRegistration {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

pub fn default_registry_path() -> Result<PathBuf> {
    if let Some(directory) = std::env::var_os("CODE_MECHANIC_STATE_DIR") {
        return Ok(PathBuf::from(directory).join("watchers.sqlite"));
    }
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|directory| directory.join("code-mechanic/watchers.sqlite"))
        .ok_or_else(|| anyhow!("cannot determine a per-user state directory for watcher metadata"))
}

fn initialise_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS watchers ( \
           watcher_id TEXT PRIMARY KEY, pid INTEGER NOT NULL, root TEXT NOT NULL, \
           database TEXT NOT NULL, started_unix_ms INTEGER NOT NULL, \
           heartbeat_unix_ms INTEGER NOT NULL, deadline_unix_ms INTEGER, \
           idle_exit_ms INTEGER, stop_requested INTEGER NOT NULL DEFAULT 0 \
         ); \
         CREATE INDEX IF NOT EXISTS watchers_heartbeat_idx ON watchers(heartbeat_unix_ms);",
    )?;
    Ok(())
}

fn unix_ms() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?;
    i64::try_from(elapsed.as_millis()).context("Unix timestamp exceeds i64 milliseconds")
}

fn duration_ms(duration: Duration) -> Result<i64> {
    i64::try_from(duration.as_millis()).context("duration exceeds i64 milliseconds")
}

fn optional_duration_ms(duration: Option<Duration>) -> Result<Option<i64>> {
    duration.map(duration_ms).transpose()
}

fn row_u32(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u32> {
    let value: i64 = row.get(index)?;
    u32::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

#[cfg(unix)]
fn signal_process(pid: u32) -> Result<bool> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let pid = i32::try_from(pid).context("watcher PID exceeds i32")?;
    match kill(Pid::from_raw(pid), Signal::SIGTERM) {
        Ok(()) => Ok(true),
        Err(nix::errno::Errno::ESRCH) => Ok(false),
        Err(error) => Err(error).context("send SIGTERM to watcher process"),
    }
}

#[cfg(windows)]
fn signal_process(pid: u32) -> Result<bool> {
    let status = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string()])
        .status()
        .context("run taskkill for watcher process")?;
    Ok(status.success())
}

#[cfg(not(any(unix, windows)))]
fn signal_process(_pid: u32) -> Result<bool> {
    Ok(false)
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("restrict watcher registry directory {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restrict watcher registry file {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> Result<()> {
    Ok(())
}
