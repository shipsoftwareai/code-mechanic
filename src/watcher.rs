//! Bounded recursive filesystem watching with explicit unwatch on every exit.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;

use crate::index::{CodeIndex, IndexSummary};
use crate::language::Language;
use crate::watch_registry::{RegistrationMetadata, WatchRegistration, WatcherRegistry};

#[derive(Debug, Clone)]
pub struct WatchConfig {
    pub duration: Option<Duration>,
    pub idle_exit: Option<Duration>,
    pub debounce: Duration,
    pub reconcile_interval: Duration,
    pub allow_unbounded: bool,
    /// Override the per-user registry, primarily for isolated tests.
    pub registry_path: Option<PathBuf>,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            duration: Some(Duration::from_secs(30)),
            idle_exit: Some(Duration::from_secs(2)),
            debounce: Duration::from_millis(150),
            reconcile_interval: Duration::from_secs(30),
            allow_unbounded: false,
            registry_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WatchReport {
    pub root: String,
    pub reason: String,
    pub event_batches: usize,
    pub paths_refreshed: usize,
    pub overflow_reconciliations: usize,
    pub periodic_reconciliations: usize,
    pub unwatched: bool,
    pub index: IndexSummary,
}

#[derive(Debug)]
struct LoopReport {
    reason: &'static str,
    event_batches: usize,
    paths_refreshed: usize,
    overflow_reconciliations: usize,
    periodic_reconciliations: usize,
    initial: IndexSummary,
}

struct ActiveWatch {
    watcher: Option<RecommendedWatcher>,
    registration: WatchRegistration,
    root: PathBuf,
    closed: bool,
}

/// Watch until the bounded configuration or caller stop flag ends the run.
/// The OS root is explicitly unregistered before this function returns.
pub fn run(index: &CodeIndex, config: &WatchConfig, stop: &AtomicBool) -> Result<WatchReport> {
    if config.duration.is_none() && config.idle_exit.is_none() && !config.allow_unbounded {
        bail!("an unbounded watcher requires the CLI's explicit --forever acknowledgement");
    }
    let (sender, receiver) = channel::<notify::Result<Event>>();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |event| {
        let _ = sender.send(event);
    })
    .context("create recursive OS watcher")?;
    watcher
        .watch(index.root(), RecursiveMode::Recursive)
        .with_context(|| format!("watch {}", index.root().display()))?;

    let registry = match &config.registry_path {
        Some(path) => WatcherRegistry::open_at(path),
        None => WatcherRegistry::open_default(),
    };
    let registry = match registry {
        Ok(registry) => registry,
        Err(error) => {
            watcher.unwatch(index.root()).with_context(|| {
                format!("unwatch {} after registry failure", index.root().display())
            })?;
            return Err(error).context("open watcher lifecycle registry");
        }
    };
    let registration = match registry.register(&RegistrationMetadata {
        root: index.root(),
        database: index.database(),
        duration: config.duration,
        idle_exit: config.idle_exit,
    }) {
        Ok(registration) => registration,
        Err(error) => {
            watcher.unwatch(index.root()).with_context(|| {
                format!(
                    "unwatch {} after registration failure",
                    index.root().display()
                )
            })?;
            return Err(error).context("register watcher lifecycle metadata");
        }
    };
    let mut active = ActiveWatch {
        watcher: Some(watcher),
        registration,
        root: index.root().to_path_buf(),
        closed: false,
    };

    let run_result = watch_loop(index, config, stop, &receiver, &active);
    let close_result = active.close();
    let report = run_result?;
    close_result?;
    let summary = index.summary(
        report.initial.reparsed + report.paths_refreshed,
        report.initial.removed,
    )?;
    Ok(WatchReport {
        root: index.root().display().to_string(),
        reason: report.reason.to_owned(),
        event_batches: report.event_batches,
        paths_refreshed: report.paths_refreshed,
        overflow_reconciliations: report.overflow_reconciliations,
        periodic_reconciliations: report.periodic_reconciliations,
        unwatched: true,
        index: summary,
    })
}

fn watch_loop(
    index: &CodeIndex,
    config: &WatchConfig,
    stop: &AtomicBool,
    receiver: &std::sync::mpsc::Receiver<notify::Result<Event>>,
    active: &ActiveWatch,
) -> Result<LoopReport> {
    // The OS watch is live before reconciliation, so changes made during the
    // full freshness scan remain queued rather than falling into a race window.
    let initial = index.reconcile(true)?;

    let started = Instant::now();
    let mut last_activity = Instant::now();
    let mut last_reconcile = Instant::now();
    let mut last_heartbeat = Instant::now();
    let mut pending = BTreeSet::new();
    let mut event_batches = 0usize;
    let mut paths_refreshed = 0usize;
    let mut overflow_reconciliations = 0usize;
    let mut periodic_reconciliations = 0usize;
    let reason;

    loop {
        if last_heartbeat.elapsed() >= Duration::from_millis(500) {
            if active.registration.heartbeat_and_should_stop()? {
                flush_pending(index, &mut pending, &mut paths_refreshed)?;
                reason = "registry_stop";
                break;
            }
            last_heartbeat = Instant::now();
        }
        if stop.load(Ordering::Relaxed) {
            flush_pending(index, &mut pending, &mut paths_refreshed)?;
            reason = "stopped";
            break;
        }
        if config
            .duration
            .is_some_and(|duration| started.elapsed() >= duration)
        {
            flush_pending(index, &mut pending, &mut paths_refreshed)?;
            reason = "duration_elapsed";
            break;
        }
        if config
            .idle_exit
            .is_some_and(|idle| pending.is_empty() && last_activity.elapsed() >= idle)
        {
            reason = "idle";
            break;
        }
        if last_reconcile.elapsed() >= config.reconcile_interval {
            flush_pending(index, &mut pending, &mut paths_refreshed)?;
            index.reconcile(false)?;
            periodic_reconciliations += 1;
            last_reconcile = Instant::now();
        }

        let wait = config.debounce.min(Duration::from_millis(100));
        match receiver.recv_timeout(wait) {
            Ok(Ok(event)) => {
                if event.need_rescan() {
                    last_activity = Instant::now();
                    pending.clear();
                    index.reconcile(true)?;
                    overflow_reconciliations += 1;
                    last_reconcile = Instant::now();
                    continue;
                }
                if process_content_event(
                    index,
                    event,
                    &mut pending,
                    &mut last_activity,
                    &mut paths_refreshed,
                )? {
                    event_batches += 1;
                    last_reconcile = Instant::now();
                }
            }
            Ok(Err(error)) => {
                pending.clear();
                index
                    .reconcile(true)
                    .with_context(|| format!("reconcile after watcher error: {error}"))?;
                overflow_reconciliations += 1;
                last_reconcile = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {
                if !pending.is_empty() && last_activity.elapsed() >= config.debounce {
                    flush_pending(index, &mut pending, &mut paths_refreshed)?;
                    event_batches += 1;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush_pending(index, &mut pending, &mut paths_refreshed)?;
                reason = "watcher_disconnected";
                break;
            }
        }
    }

    Ok(LoopReport {
        reason,
        event_batches,
        paths_refreshed,
        overflow_reconciliations,
        periodic_reconciliations,
        initial,
    })
}

impl ActiveWatch {
    fn close(&mut self) -> Result<()> {
        let unwatch_result = if let Some(mut watcher) = self.watcher.take() {
            watcher
                .unwatch(&self.root)
                .with_context(|| format!("unwatch {}", self.root.display()))
        } else {
            Ok(())
        };
        let finish_result = self.registration.finish();
        self.closed = true;
        unwatch_result?;
        finish_result.context("remove watcher lifecycle registration")
    }
}

impl Drop for ActiveWatch {
    fn drop(&mut self) {
        if !self.closed {
            if let Some(mut watcher) = self.watcher.take() {
                let _ = watcher.unwatch(&self.root);
            }
            let _ = self.registration.finish();
        }
    }
}

fn flush_pending(
    index: &CodeIndex,
    pending: &mut BTreeSet<PathBuf>,
    paths_refreshed: &mut usize,
) -> Result<()> {
    let paths = std::mem::take(pending);
    for path in paths {
        index.refresh_path(&path)?;
        *paths_refreshed += 1;
    }
    Ok(())
}

fn is_content_event(kind: EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

fn process_content_event(
    index: &CodeIndex,
    event: Event,
    pending: &mut BTreeSet<PathBuf>,
    last_activity: &mut Instant,
    paths_refreshed: &mut usize,
) -> Result<bool> {
    let reconcile_tree = event_requires_tree_reconcile(event.kind);
    if !is_content_event(event.kind) {
        return Ok(false);
    }
    let relevant: Vec<PathBuf> = event
        .paths
        .into_iter()
        .filter(|path| relevant_path(path))
        .collect();
    if relevant.is_empty() {
        return Ok(false);
    }
    *last_activity = Instant::now();
    pending.extend(relevant);
    if !reconcile_tree {
        return Ok(false);
    }

    // Windows can report rename/removal paths in a form that cannot be made
    // relative lexically. Refresh event paths first, then reconcile the file
    // set so a vanished indexed row cannot survive until a periodic sweep.
    flush_pending(index, pending, paths_refreshed)?;
    index.reconcile(false)?;
    Ok(true)
}

fn event_requires_tree_reconcile(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
    )
}

fn relevant_path(path: &Path) -> bool {
    let ignored_component = path.components().any(|component| {
        matches!(
            component,
            Component::Normal(value)
                if matches!(
                    value.to_str(),
                    Some(".git" | ".code-mechanic" | "build" | "dist" | "target")
                )
        )
    });
    !ignored_component && Language::for_path(path).is_some()
}
