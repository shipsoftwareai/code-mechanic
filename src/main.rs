use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use code_mechanic::benchmark::{self, BenchmarkCase};
use code_mechanic::index::CodeIndex;
use code_mechanic::query::{self, BodySearchOptions};
use code_mechanic::refactor;
use code_mechanic::watch_registry::WatcherRegistry;
use code_mechanic::watcher::{self, WatchConfig};
use code_mechanic::{canonical_root, default_database_path};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "code-mechanic",
    version,
    about = "AST-indexed, guarded static-language mechanical refactors for coding agents"
)]
struct Cli {
    /// Workspace root. All indexed and edited paths remain below it.
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    /// SQL index path (default: ROOT/.code-mechanic/index.sqlite).
    #[arg(long, global = true)]
    database: Option<PathBuf>,
    /// Pretty-print JSON. Compact one-line JSON is the agent-friendly default.
    #[arg(long, global = true)]
    pretty: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the stable language, query, watcher, and safety surface.
    Capabilities,
    /// Build or incrementally reconcile the persistent index.
    Index(IndexArgs),
    /// Return index counts and parse-failure state.
    Status,
    /// List files refused because their syntax tree contains parse errors.
    Diagnostics,
    /// Return a compact function/prototype outline for one file.
    Outline {
        #[arg(long)]
        file: String,
    },
    /// Return fresh function, signature, and body spans without source text.
    Locate {
        name: String,
        #[arg(long)]
        file: Option<String>,
    },
    /// Search only inside one fresh function body and return bounded lines.
    SearchBody(SearchBodyArgs),
    /// Read exactly one unambiguous function definition.
    Symbol {
        name: String,
        #[arg(long)]
        file: Option<String>,
        /// Emit only the exact function source for minimum agent token cost.
        #[arg(long)]
        raw: bool,
    },
    /// List AST-confirmed function/method calls with exact byte ranges.
    Refs { name: String },
    /// Rename one unique function across definitions, prototypes, and call sites.
    Rename(RenameArgs),
    /// Insert code immediately inside one unique function body.
    InjectEntry(InjectArgs),
    /// Replace the statements inside one unique braced function body.
    ReplaceBody(ReplaceBodyArgs),
    /// Append one formal parameter and the matching argument at every call.
    AppendParameter(AppendParameterArgs),
    /// Keep the index fresh with a bounded recursive OS watcher.
    Watch(WatchArgs),
    /// Inspect and stop watchers across all registered roots for this user.
    Watchers {
        #[command(subcommand)]
        command: WatcherCommand,
    },
    /// Measure equivalent retrieval output with a local o200k token counter.
    Bench(BenchArgs),
}

#[derive(Debug, Args)]
struct IndexArgs {
    /// Reconcile changed/deleted files instead of rebuilding all rows.
    #[arg(long)]
    reconcile: bool,
    /// Hash every candidate during reconciliation (overflow/recovery mode).
    #[arg(long, requires = "reconcile")]
    force_hash: bool,
}

#[derive(Debug, Args)]
struct RenameArgs {
    #[arg(long)]
    from: String,
    #[arg(long)]
    to: String,
    #[arg(long)]
    file: Option<String>,
    /// Write the fresh plan. Preview is the default.
    #[arg(long)]
    apply: bool,
    /// Plan id returned by a fresh preview; required with --apply.
    #[arg(long, requires = "apply")]
    expect_plan: Option<String>,
}

#[derive(Debug, Args)]
struct InjectArgs {
    #[arg(long)]
    symbol: String,
    #[arg(long)]
    code: String,
    #[arg(long)]
    file: Option<String>,
    /// Write the fresh plan. Preview is the default.
    #[arg(long)]
    apply: bool,
    /// Plan id returned by a fresh preview; required with --apply.
    #[arg(long, requires = "apply")]
    expect_plan: Option<String>,
}

#[derive(Debug, Args)]
struct ReplaceBodyArgs {
    #[arg(long)]
    symbol: String,
    #[arg(long)]
    code: String,
    #[arg(long)]
    file: Option<String>,
    /// Write the fresh plan. Preview is the default.
    #[arg(long)]
    apply: bool,
    /// Plan id returned by a fresh preview; required with --apply.
    #[arg(long, requires = "apply")]
    expect_plan: Option<String>,
}

#[derive(Debug, Args)]
struct AppendParameterArgs {
    #[arg(long)]
    symbol: String,
    /// Language-native formal parameter, for example `timeout: Duration`.
    #[arg(long)]
    parameter: String,
    /// Language-native expression appended at each indexed call site.
    #[arg(long)]
    argument: String,
    #[arg(long)]
    file: Option<String>,
    /// Write the fresh plan. Preview is the default.
    #[arg(long)]
    apply: bool,
    /// Plan id returned by a fresh preview; required with --apply.
    #[arg(long, requires = "apply")]
    expect_plan: Option<String>,
}

#[derive(Debug, Args)]
struct SearchBodyArgs {
    /// Unique function or method name whose body should be searched.
    symbol: String,
    /// Literal text by default, or a Rust regular expression with --regex.
    #[arg(long)]
    pattern: String,
    #[arg(long)]
    file: Option<String>,
    #[arg(long)]
    regex: bool,
    #[arg(long)]
    ignore_case: bool,
    /// Maximum matching lines returned; total matching lines are still counted.
    #[arg(long, default_value_t = 20)]
    max_results: usize,
}

#[derive(Debug, Args)]
struct WatchArgs {
    /// Hard runtime bound. Defaults to 30 seconds unless --forever.
    #[arg(long, default_value_t = 30)]
    duration_seconds: u64,
    /// Exit after this many seconds without an event. Zero disables idle exit.
    #[arg(long, default_value_t = 2)]
    until_idle_seconds: u64,
    /// Explicitly allow an unbounded foreground watcher; Ctrl-C still unwatches.
    #[arg(long, conflicts_with = "duration_seconds")]
    forever: bool,
    #[arg(long, default_value_t = 150)]
    debounce_ms: u64,
    #[arg(long, default_value_t = 30)]
    reconcile_seconds: u64,
}

#[derive(Debug, Subcommand)]
enum WatcherCommand {
    /// List roots, PIDs, databases, heartbeats, bounds, and stop state.
    List,
    /// Ask every active watcher to unwatch and exit.
    StopAll {
        /// Send a termination signal if cooperative exit misses the grace period.
        #[arg(long)]
        force: bool,
        #[arg(long, default_value_t = 750)]
        grace_ms: u64,
    },
    /// Remove registrations that have not heartbeated for 60 seconds.
    Prune,
}

#[derive(Debug, Args)]
struct BenchArgs {
    /// SYMBOL or SYMBOL:workspace/relative/file. Repeat for multiple cases.
    #[arg(long = "case", required = true)]
    cases: Vec<String>,
    #[arg(long, default_value_t = 10)]
    warm_runs: usize,
    #[arg(long, default_value_t = 120)]
    window_lines: usize,
    #[arg(long, default_value_t = 50.0)]
    min_token_reduction_pct: f64,
    /// Optional JSON evidence path.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct Capabilities {
    schema_version: u32,
    languages: [&'static str; 7],
    persistent_index: &'static str,
    queries: [&'static str; 7],
    refactors: [&'static str; 4],
    safety: [&'static str; 5],
    watcher: [&'static str; 7],
    output: [&'static str; 2],
}

fn main() {
    if let Err(error) = run() {
        let payload = serde_json::json!({
            "ok": false,
            "error": format!("{error:#}"),
        });
        eprintln!("{}", serde_json::to_string(&payload).unwrap_or_default());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if matches!(cli.command, Command::Capabilities) {
        return print_json(&capabilities(), cli.pretty);
    }
    if let Command::Watchers { command } = &cli.command {
        return run_watcher_command(command, cli.pretty);
    }
    let root = canonical_root(&cli.root)?;
    let database = cli
        .database
        .clone()
        .unwrap_or_else(|| default_database_path(&root));
    let index = CodeIndex::open(&root, &database)?;

    match cli.command {
        Command::Capabilities | Command::Watchers { .. } => {
            unreachable!("handled before index open")
        }
        Command::Index(args) => {
            let report = if args.reconcile {
                index.reconcile(args.force_hash)?
            } else {
                index.rebuild()?
            };
            print_json(&report, cli.pretty)
        }
        Command::Status => print_json(&index.summary(0, 0)?, cli.pretty),
        Command::Diagnostics => print_json(&index.diagnostics()?, cli.pretty),
        Command::Outline { file } => print_json(&index.outline(&file)?, cli.pretty),
        Command::Locate { name, file } => {
            print_json(&query::locate(&index, &name, file.as_deref())?, cli.pretty)
        }
        Command::SearchBody(args) => print_json(
            &query::search_body(
                &index,
                &args.symbol,
                args.file.as_deref(),
                &BodySearchOptions {
                    pattern: &args.pattern,
                    regex: args.regex,
                    ignore_case: args.ignore_case,
                    max_results: args.max_results,
                },
            )?,
            cli.pretty,
        ),
        Command::Symbol { name, file, raw } => {
            let symbol = index.read_symbol(&name, file.as_deref())?;
            if raw {
                print!("{}", symbol.source);
                Ok(())
            } else {
                print_json(&symbol, cli.pretty)
            }
        }
        Command::Refs { name } => print_json(&index.references(&name)?, cli.pretty),
        Command::Rename(args) => print_json(
            &refactor::rename_function(
                &index,
                &args.from,
                &args.to,
                args.file.as_deref(),
                args.apply,
                args.expect_plan.as_deref(),
            )?,
            cli.pretty,
        ),
        Command::InjectEntry(args) => print_json(
            &refactor::inject_function_entry(
                &index,
                &args.symbol,
                &args.code,
                args.file.as_deref(),
                args.apply,
                args.expect_plan.as_deref(),
            )?,
            cli.pretty,
        ),
        Command::ReplaceBody(args) => run_replace_body(&index, &args, cli.pretty),
        Command::AppendParameter(args) => run_append_parameter(&index, &args, cli.pretty),
        Command::Watch(args) => run_watch(&index, &args, cli.pretty),
        Command::Bench(args) => run_benchmark(&index, &args, cli.pretty),
    }
}

fn run_replace_body(index: &CodeIndex, args: &ReplaceBodyArgs, pretty: bool) -> Result<()> {
    print_json(
        &refactor::replace_function_body(
            index,
            &args.symbol,
            &args.code,
            args.file.as_deref(),
            args.apply,
            args.expect_plan.as_deref(),
        )?,
        pretty,
    )
}

fn run_append_parameter(index: &CodeIndex, args: &AppendParameterArgs, pretty: bool) -> Result<()> {
    print_json(
        &refactor::append_parameter(
            index,
            &args.symbol,
            &args.parameter,
            &args.argument,
            args.file.as_deref(),
            args.apply,
            args.expect_plan.as_deref(),
        )?,
        pretty,
    )
}

fn run_watch(index: &CodeIndex, args: &WatchArgs, pretty: bool) -> Result<()> {
    if args.debounce_ms == 0 || args.reconcile_seconds == 0 {
        bail!("watch debounce and reconciliation intervals must be positive");
    }
    let duration = if args.forever {
        None
    } else {
        Some(Duration::from_secs(args.duration_seconds.max(1)))
    };
    let idle_exit =
        (args.until_idle_seconds > 0).then(|| Duration::from_secs(args.until_idle_seconds));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_signal = stop.clone();
    ctrlc::set_handler(move || {
        stop_for_signal.store(true, std::sync::atomic::Ordering::Relaxed);
    })
    .context("install Ctrl-C watcher teardown")?;
    let config = WatchConfig {
        duration,
        idle_exit,
        debounce: Duration::from_millis(args.debounce_ms),
        reconcile_interval: Duration::from_secs(args.reconcile_seconds),
        allow_unbounded: args.forever,
        registry_path: None,
    };
    print_json(&watcher::run(index, &config, stop.as_ref())?, pretty)
}

fn run_watcher_command(command: &WatcherCommand, pretty: bool) -> Result<()> {
    let registry = WatcherRegistry::open_default()?;
    match command {
        WatcherCommand::List => print_json(&registry.list()?, pretty),
        WatcherCommand::StopAll { force, grace_ms } => print_json(
            &registry.stop_all(*force, Duration::from_millis(*grace_ms))?,
            pretty,
        ),
        WatcherCommand::Prune => print_json(
            &serde_json::json!({
                "registry": registry.path(),
                "pruned": registry.prune_stale()?,
            }),
            pretty,
        ),
    }
}

fn run_benchmark(index: &CodeIndex, args: &BenchArgs, pretty: bool) -> Result<()> {
    let cases = args
        .cases
        .iter()
        .map(|value| benchmark::parse_case(value))
        .collect::<Result<Vec<BenchmarkCase>>>()?;
    let report = benchmark::run(
        index,
        &cases,
        args.warm_runs,
        args.window_lines,
        args.min_token_reduction_pct,
    )?;
    if let Some(path) = &args.output {
        write_json(path, &report)?;
    }
    print_json(&report, pretty)?;
    if !report.passed {
        bail!("token/output benchmark failed its equivalence or reduction threshold");
    }
    Ok(())
}

fn capabilities() -> Capabilities {
    Capabilities {
        schema_version: 1,
        languages: [
            "rust",
            "c/header",
            "c++",
            "go",
            "objective-c",
            "glsl",
            "kotlin",
        ],
        persistent_index: "SQLite WAL at ROOT/.code-mechanic/index.sqlite",
        queries: [
            "status",
            "diagnostics",
            "outline",
            "locate",
            "search-body",
            "symbol",
            "refs",
        ],
        refactors: [
            "function rename",
            "function-entry injection",
            "function-body replacement",
            "append formal parameter plus call arguments",
        ],
        safety: [
            "preview by default",
            "apply requires fresh plan id",
            "content-hash stale-index rejection",
            "ambiguity rejection",
            "post-edit Tree-sitter parse gate",
        ],
        watcher: [
            "recursive OS events",
            "150ms default debounce",
            "periodic/overflow reconciliation",
            "bounded by default with explicit unwatch",
            "per-user root/PID/heartbeat registry",
            "cooperative watchers stop-all",
            "optional forced process termination",
        ],
        output: [
            "compact JSON on stdout; structured JSON errors on stderr",
            "locator-first spans and bounded body search; symbol --raw is explicit full source",
        ],
    }
}

fn print_json<T: Serialize>(value: &T, pretty: bool) -> Result<()> {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(value)?);
    }
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create benchmark output directory {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    std::fs::write(path, bytes)
        .with_context(|| format!("write benchmark evidence {}", path.display()))
}
