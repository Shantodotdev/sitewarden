//! SiteWarden CLI & Daemon Entrypoint.
//!
//! Conforms to IEEE Std 830-1998 (SRS Specification).
//! Provides a comprehensive CLI suite (`status`, `history`, `check`, `test`, `update`, `prune`, `doctor`, `daemon`)
//! alongside signal handling for graceful shutdown, hot-reload initialization, and hybrid execution.

use anyhow::Result;
use arc_swap::ArcSwap;
use clap::{Parser, Subcommand};
use reqwest::Client;
use sitewarden::alert::AlertDispatcher;
use sitewarden::config::AppConfig;
use sitewarden::doctor::run_diagnostics;
use sitewarden::pruner::prune_screenshots;
use sitewarden::report::{
    format_doctor_report, format_history_table, format_status_dashboard, BOLD_CYAN, BOLD_GREEN,
    BOLD_RED, BOLD_WHITE, RESET,
};
use sitewarden::scheduler::{run_all_suites, run_named_suite, start_scheduler};
use sitewarden::state::{resolve_state_path, AppState};
use sitewarden::updater::{check_latest_release, detect_environment, EnvironmentType};
use sitewarden::watcher::ConfigWatcher;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser, Debug)]
#[command(
    name = "sitewarden",
    author = "SiteWarden Core Team",
    version = env!("CARGO_PKG_VERSION"),
    about = "Autonomous, ultra-lightweight browser smoke-testing sentinel in Rust"
)]
struct Cli {
    /// Path to YAML configuration file.
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    /// Execute all test suites once immediately and exit (backwards-compatible alias for 'test').
    #[arg(long)]
    run_once: bool,

    /// Enable verbose debug logging.
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the continuous scheduled testing daemon (default mode)
    Daemon,
    /// Display overall daemon status, uptime, success rate, and storage metrics
    Status,
    /// Display a timeline history of past test cycles
    History {
        /// Number of recent cycles to display
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    /// Check and validate configuration file and target reachability
    Check,
    /// Execute a specific test suite or all test suites immediately on demand
    Test {
        /// Name of the test suite to execute (runs all suites if omitted)
        suite: Option<String>,
    },
    /// Check for SiteWarden updates or view upgrade instructions
    Update {
        /// Only check for updates without printing upgrade instructions
        #[arg(long)]
        check: bool,
    },
    /// Prune old failure screenshot artifacts to reclaim storage
    Prune {
        /// Delete screenshots older than specified days
        #[arg(short, long, default_value_t = 7)]
        days: u64,
        /// Dry run mode (simulate deletion without removing files)
        #[arg(long)]
        dry_run: bool,
    },
    /// Run diagnostic environment and health checks
    Doctor,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize structured logging for daemon and test modes
    let filter_level = if cli.verbose {
        "sitewarden=debug,chromiumoxide=debug,info"
    } else {
        "sitewarden=info,chromiumoxide=error,info"
    };

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter_level)))
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(true)
                .compact()
                .with_writer(std::io::stdout),
        )
        .init();

    let config_path = resolve_config_path(&cli.config);

    // Route Subcommands
    match cli.command {
        Some(Commands::Status) => handle_status(&config_path).await,
        Some(Commands::History { limit }) => handle_history(&config_path, limit),
        Some(Commands::Check) => handle_check(&config_path).await,
        Some(Commands::Test { suite }) => handle_test(&config_path, suite).await,
        Some(Commands::Update { check }) => handle_update(check).await,
        Some(Commands::Prune { days, dry_run }) => handle_prune(&config_path, days, dry_run),
        Some(Commands::Doctor) => handle_doctor(&config_path).await,
        Some(Commands::Daemon) | None => {
            if cli.run_once {
                handle_test(&config_path, None).await
            } else {
                run_daemon(&config_path, cli.verbose).await
            }
        }
    }
}

/// Resolves configuration file path with standard fallbacks.
fn resolve_config_path(specified: &Path) -> PathBuf {
    if specified.exists() {
        specified.to_path_buf()
    } else if Path::new("/app/config/config.yaml").exists() {
        PathBuf::from("/app/config/config.yaml")
    } else if Path::new("/app/config.yaml").exists() {
        PathBuf::from("/app/config.yaml")
    } else {
        specified.to_path_buf()
    }
}

/// Subcommand: `sitewarden status`
///
/// Loads persistent metrics from `.sitewarden_state.json`, reads the active `config.yaml`,
/// queries screenshot storage usage, and initiates a non-blocking background check for updates.
/// Renders a formatted ASCII dashboard containing uptime, success rates, and cycle health.
async fn handle_status(config_path: &Path) -> Result<()> {
    if !config_path.exists() {
        eprintln!(
            "{}❌ Configuration file not found at: {:?}{}",
            BOLD_RED, config_path, RESET
        );
        std::process::exit(1);
    }

    let config = match AppConfig::from_file(config_path) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!(
                "{}❌ Failed to load configuration: {:#}{}",
                BOLD_RED, err, RESET
            );
            std::process::exit(1);
        }
    };

    let state_path = resolve_state_path(config_path);
    let state = AppState::load(&state_path);

    // Quick background check for updates (silent fallback)
    let http_client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_default();
    let update_info = check_latest_release(&http_client).await.ok();

    // Count screenshots
    let screenshot_dir = Path::new(&config.screenshot_dir);
    let prune_info = prune_screenshots(screenshot_dir, 999999, true).unwrap_or_default();

    let dashboard = format_status_dashboard(
        &state,
        &config,
        update_info.as_ref(),
        prune_info.total_scanned,
        prune_info.reclaimed_bytes,
    );
    println!("{}", dashboard);

    Ok(())
}

/// Subcommand: `sitewarden history`
///
/// Retrieves the most recent `limit` test cycle snapshots from the rolling state buffer
/// and renders a clean, tabular chronological history view with durations and pass/fail badges.
fn handle_history(config_path: &Path, limit: usize) -> Result<()> {
    let state_path = resolve_state_path(config_path);
    let state = AppState::load(&state_path);

    let records: Vec<_> = state.history.iter().take(limit).cloned().collect();
    let table = format_history_table(&records);
    println!("{}", table);

    Ok(())
}

/// Subcommand: `sitewarden check`
///
/// Runs pre-flight verification against the configuration file:
/// 1. Verifies YAML syntax and schema adherence.
/// 2. Evaluates cron schedule validity.
/// 3. Performs DNS lookup and live HTTP connectivity probes against all suite base URLs.
async fn handle_check(config_path: &Path) -> Result<()> {
    println!("\n🔍 Validating configuration at: {:?}", config_path);

    if !config_path.exists() {
        eprintln!(
            "{}❌ Configuration file not found at: {:?}{}",
            BOLD_RED, config_path, RESET
        );
        std::process::exit(1);
    }

    let config = match AppConfig::from_file(config_path) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!(
                "{}❌ Configuration syntax error: {:#}{}",
                BOLD_RED, err, RESET
            );
            std::process::exit(1);
        }
    };

    println!("  {}✅ YAML syntax and schema valid{}", BOLD_GREEN, RESET);
    println!(
        "  {}✅ Schedule expression valid (Cron: '{}'){}",
        BOLD_GREEN, config.schedule, RESET
    );
    println!(
        "  {}✅ {} Suites loaded ({} Total Steps){}",
        BOLD_GREEN,
        config.suites.len(),
        config.total_steps(),
        RESET
    );

    // Test DNS & URL reachability for each suite
    let http_client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    println!("\n🌐 Testing endpoint connectivity...");
    let mut all_reachable = true;

    for suite in &config.suites {
        let is_static = suite.is_all_static();
        let engine_tag = if is_static {
            format!("{}[Pure-Rust Static]{}", BOLD_CYAN, RESET)
        } else {
            format!("{}[Headless Browser]{}", BOLD_CYAN, RESET)
        };

        match http_client.get(&suite.base_url).send().await {
            Ok(resp) => {
                println!(
                    "  {}✅ Reachable: {} (HTTP {}) → {}{}",
                    BOLD_GREEN,
                    suite.base_url,
                    resp.status(),
                    engine_tag,
                    RESET
                );
            }
            Err(err) => {
                all_reachable = false;
                eprintln!(
                    "  {}❌ Unreachable: {} ({}) → {}{}",
                    BOLD_RED, suite.base_url, err, engine_tag, RESET
                );
            }
        }
    }

    println!();
    if all_reachable {
        println!(
            "{}🎉 Configuration is 100% valid and production-ready!{}",
            BOLD_GREEN, RESET
        );
    } else {
        eprintln!(
            "{}⚠️ Configuration has reachability warnings. Review endpoints above.{}",
            BOLD_RED, RESET
        );
    }

    Ok(())
}

/// Subcommand: `sitewarden test [SUITE]`
///
/// Executes test suites on demand:
/// - If `suite_name` is provided, filters and executes only the matching suite (case-insensitive substring).
/// - If `suite_name` is omitted, executes all suites sequentially or with bounded concurrency.
///
/// Exits with code `0` on 100% success, or `1` if any test step fails (making it CI/CD pipeline friendly).
async fn handle_test(config_path: &Path, suite_name: Option<String>) -> Result<()> {
    if !config_path.exists() {
        eprintln!(
            "{}❌ Configuration file not found at: {:?}{}",
            BOLD_RED, config_path, RESET
        );
        std::process::exit(1);
    }

    let config = match AppConfig::from_file(config_path) {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!(
                "{}❌ Failed to load configuration: {:#}{}",
                BOLD_RED, err, RESET
            );
            std::process::exit(1);
        }
    };

    let shared_config = Arc::new(ArcSwap::from_pointee(config));
    let alert_dispatcher = AlertDispatcher::new();

    let passed = if let Some(ref name) = suite_name {
        run_named_suite(name, &shared_config, &alert_dispatcher).await
    } else {
        run_all_suites(&shared_config, &alert_dispatcher).await
    };

    if !passed {
        std::process::exit(1);
    }

    Ok(())
}

/// Subcommand: `sitewarden update [--check]`
///
/// Queries GitHub REST API for newer releases, displays changelog summaries,
/// and outputs targeted 1-command upgrade scripts based on whether Docker or standalone binary execution is detected.
async fn handle_update(check_only: bool) -> Result<()> {
    println!("\n🔍 Checking GitHub Releases API for SiteWarden updates...");
    let http_client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    match check_latest_release(&http_client).await {
        Ok(info) => {
            if info.update_available {
                println!(
                    "\n{}✨ Update Available: v{} (Current: v{}){}",
                    BOLD_CYAN, info.latest_version, info.current_version, RESET
                );
                println!("  {}Release:{} {}", BOLD_WHITE, RESET, info.release_name);
                println!("  {}URL:{}     {}", BOLD_WHITE, RESET, info.release_url);

                if !info.release_notes.is_empty() {
                    println!(
                        "\n{}Release Notes:{}\n{}",
                        BOLD_WHITE, RESET, info.release_notes
                    );
                }

                if !check_only {
                    let env = detect_environment();
                    println!("\n{}🚀 Upgrade Instructions:{}\n", BOLD_GREEN, RESET);
                    match env {
                        EnvironmentType::Docker => {
                            println!("  Docker container deployment detected:");
                            println!("  {}cd /opt/sitewarden && docker compose pull && docker compose up -d{}", BOLD_CYAN, RESET);
                        }
                        EnvironmentType::Standalone => {
                            println!("  Standalone binary deployment detected:");
                            println!("  {}curl -fsSL https://raw.githubusercontent.com/Shantodotdev/sitewarden/master/install.sh | bash{}", BOLD_CYAN, RESET);
                        }
                    }
                }
            } else {
                println!(
                    "\n{}✅ SiteWarden is up-to-date! (Version v{}){}",
                    BOLD_GREEN, info.current_version, RESET
                );
            }
        }
        Err(err) => {
            eprintln!(
                "\n{}❌ Failed to check for updates: {:#}{}",
                BOLD_RED, err, RESET
            );
        }
    }

    Ok(())
}

/// Subcommand: `sitewarden prune [--days N]`
///
/// Traverses the screenshot directory, identifies failure captures older than `days`,
/// deletes them from disk, and reports the exact byte capacity reclaimed.
fn handle_prune(config_path: &Path, days: u64, dry_run: bool) -> Result<()> {
    let config = if config_path.exists() {
        AppConfig::from_file(config_path).ok()
    } else {
        None
    };

    let screenshot_dir_str = config
        .map(|c| c.screenshot_dir)
        .unwrap_or_else(|| "screenshots".to_string());
    let screenshot_dir = Path::new(&screenshot_dir_str);

    println!(
        "\n🧹 {}Scanning screenshot directory: {:?} (Older than {} days)...",
        if dry_run { "[DRY RUN] " } else { "" },
        screenshot_dir,
        days
    );

    let summary = prune_screenshots(screenshot_dir, days, dry_run)?;

    if summary.deleted_count > 0 {
        let mode_str = if dry_run {
            "Found for removal"
        } else {
            "Cleaned up"
        };
        println!(
            "  {}🗑️ {} {} screenshot artifacts (Reclaimed {}){}",
            BOLD_GREEN,
            mode_str,
            summary.deleted_count,
            sitewarden::pruner::format_bytes(summary.reclaimed_bytes),
            RESET
        );
    } else {
        println!(
            "  {}✨ No stale screenshots found. Total artifacts on disk: {}{}",
            BOLD_GREEN, summary.total_scanned, RESET
        );
    }

    Ok(())
}

/// Subcommand: `sitewarden doctor`
///
/// Runs diagnostic pre-flight checks across configuration, filesystem permissions,
/// Chromium binary presence, and network/DNS connectivity.
async fn handle_doctor(config_path: &Path) -> Result<()> {
    let checks = run_diagnostics(config_path).await;
    let report = format_doctor_report(&checks);
    println!("{}", report);
    Ok(())
}

/// Background scheduled continuous daemon runner.
async fn run_daemon(config_path: &Path, _verbose: bool) -> Result<()> {
    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Initializing SiteWarden..."
    );

    if !config_path.exists() {
        error!(
            path = ?config_path,
            "Configuration file not found. Please provide a valid config file."
        );
        std::process::exit(1);
    }

    let initial_config = match AppConfig::from_file(config_path) {
        Ok(cfg) => cfg,
        Err(err) => {
            error!(
                path = ?config_path,
                error = %err,
                "Fatal configuration parsing or validation error"
            );
            std::process::exit(1);
        }
    };

    info!(
        schedule = %initial_config.schedule,
        suites_count = initial_config.suites.len(),
        concurrency = initial_config.browser_concurrency,
        "Configuration successfully loaded and validated"
    );

    // Track daemon start in state.json
    let state_path = resolve_state_path(config_path);
    let mut state = AppState::load(&state_path);
    state.mark_started();
    let _ = state.save(&state_path);

    let shared_config = Arc::new(ArcSwap::from_pointee(initial_config));
    let alert_dispatcher = AlertDispatcher::new();

    // Initialize Config Watcher for zero-downtime hot-reloading
    let (_watcher, mut reload_rx) =
        match ConfigWatcher::new(config_path, Arc::clone(&shared_config)) {
            Ok(w) => w,
            Err(err) => {
                error!(error = %err, "Failed to initialize configuration file watcher");
                std::process::exit(1);
            }
        };

    // Initialize Background Cron Scheduler Daemon
    let scheduler =
        match start_scheduler(Arc::clone(&shared_config), alert_dispatcher.clone()).await {
            Ok(sched) => sched,
            Err(err) => {
                error!(error = %err, "Failed to initialize background cron scheduler");
                std::process::exit(1);
            }
        };
    let scheduler_handle = Arc::new(Mutex::new(scheduler));

    // Listen for graceful shutdown signals (SIGINT / SIGTERM)
    let shutdown_signal = async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigint = signal(SignalKind::interrupt()).expect("Failed to register SIGINT");
            let mut sigterm = signal(SignalKind::terminate()).expect("Failed to register SIGTERM");
            tokio::select! {
                _ = sigint.recv() => info!("Received SIGINT shutdown signal"),
                _ = sigterm.recv() => info!("Received SIGTERM shutdown signal"),
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to listen for Ctrl+C");
            info!("Received Ctrl+C shutdown signal");
        }
    };

    tokio::pin!(shutdown_signal);

    let mut current_schedule = shared_config.load().schedule.clone();

    // Main event loop
    loop {
        tokio::select! {
            _ = &mut shutdown_signal => {
                info!("Initiating graceful daemon shutdown...");
                let mut sched = scheduler_handle.lock().await;
                if let Err(err) = sched.shutdown().await {
                    warn!(error = %err, "Error shutting down job scheduler");
                }
                info!("SiteWarden sentinel cleanly terminated. Goodbye!");
                break;
            }
            _ = reload_rx.recv() => {
                let new_config = shared_config.load();
                info!(
                    schedule = %new_config.schedule,
                    suites_count = new_config.suites.len(),
                    concurrency = new_config.browser_concurrency,
                    "Live configuration hot-reload applied to memory."
                );

                // If cron schedule expression changed, dynamically reschedule
                if new_config.schedule != current_schedule {
                    info!(
                        old_schedule = %current_schedule,
                        new_schedule = %new_config.schedule,
                        "Detected updated cron schedule expression. Rescheduling background jobs..."
                    );
                    current_schedule = new_config.schedule.clone();

                    let mut sched = scheduler_handle.lock().await;
                    let _ = sched.shutdown().await;

                    match start_scheduler(Arc::clone(&shared_config), alert_dispatcher.clone()).await {
                        Ok(new_sched) => {
                            *sched = new_sched;
                            info!(schedule = %current_schedule, "Background scheduler successfully updated with new cron frequency");
                        }
                        Err(err) => {
                            error!(error = %err, "Failed to reschedule with new cron expression");
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
