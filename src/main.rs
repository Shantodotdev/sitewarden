//! SiteWarden Daemon CLI Entrypoint.
//!
//! Conforms to IEEE Std 830-1998 (SRS Specification).
//! Provides command-line options, signal handling for graceful shutdown,
//! hot-reload initialization, and execution orchestration.

use anyhow::Result;
use arc_swap::ArcSwap;
use clap::Parser;
use sitewarden::alert::AlertDispatcher;
use sitewarden::browser::BrowserManager;
use sitewarden::config::AppConfig;
use sitewarden::scheduler::{run_all_suites, start_scheduler};
use sitewarden::watcher::ConfigWatcher;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Command line arguments parser.
#[derive(Parser, Debug)]
#[command(
    name = "sitewarden",
    author = "SiteWarden Core Team",
    version = "0.1.0",
    about = "Autonomous, ultra-lightweight browser smoke-testing sentinel in Rust"
)]
struct Args {
    /// Path to YAML configuration file.
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    /// Execute all test suites once immediately and exit (useful for CI/CD or smoke testing).
    #[arg(long)]
    run_once: bool,

    /// Enable verbose debug logging.
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize structured logging:
    // In standard mode, silence benign unmapped Chromium DevTools Protocol (CDP) WebSocket enum
    // warnings (chromiumoxide=error) to keep output clean, while logging all SiteWarden events.
    // In verbose mode (--verbose), enable detailed CDP debugging traces.
    let filter_level = if args.verbose {
        "sitewarden=debug,chromiumoxide=debug,info"
    } else {
        "sitewarden=info,chromiumoxide=error,info"
    };

    // Register tracing subscriber with fallback to RUST_LOG environment variable,
    // explicitly routing logs to stdout for standard Docker/systemd daemon aggregation.
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter_level)))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Initializing SiteWarden..."
    );

    // Load and validate initial configuration
    if !args.config.exists() {
        error!(
            path = ?args.config,
            "Configuration file not found. Please provide a valid config file."
        );
        std::process::exit(1);
    }

    let initial_config = match AppConfig::from_file(&args.config) {
        Ok(cfg) => cfg,
        Err(err) => {
            error!(
                path = ?args.config,
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

    let shared_config = Arc::new(ArcSwap::from_pointee(initial_config));

    // Launch headless Chromium process
    let browser_manager = match BrowserManager::launch().await {
        Ok(bm) => Arc::new(bm),
        Err(err) => {
            error!(error = %err, "Failed to initialize headless Chromium");
            std::process::exit(1);
        }
    };

    let alert_dispatcher = AlertDispatcher::new();

    // Handle single-run mode (e.g. CI/CD pipelines, one-off test triggers, docker health checks)
    if args.run_once {
        info!("Executing in --run-once mode");
        let passed = run_all_suites(&shared_config, &browser_manager, &alert_dispatcher).await;

        // Explicitly tear down headless browser before exit
        browser_manager.shutdown().await;

        // Return exit code 0 on all pass, 1 on failure
        if passed {
            info!("All smoke tests passed successfully.");
            std::process::exit(0);
        } else {
            error!("One or more smoke tests failed.");
            std::process::exit(1);
        }
    }

    // Initialize Config Watcher for zero-downtime hot-reloading
    let (_watcher, mut reload_rx) =
        match ConfigWatcher::new(&args.config, Arc::clone(&shared_config)) {
            Ok(w) => w,
            Err(err) => {
                error!(error = %err, "Failed to initialize configuration file watcher");
                std::process::exit(1);
            }
        };

    // Spawn reload listener task to record runtime configuration swaps
    tokio::spawn(async move {
        while reload_rx.recv().await.is_some() {
            info!("Live configuration update applied in memory.");
        }
    });

    // Start background cron scheduler daemon
    let mut scheduler = match start_scheduler(
        Arc::clone(&shared_config),
        Arc::clone(&browser_manager),
        alert_dispatcher.clone(),
    )
    .await
    {
        Ok(sched) => sched,
        Err(err) => {
            error!(error = %err, "Failed to start cron scheduler");
            std::process::exit(1);
        }
    };

    info!("SiteWarden daemon is actively monitoring and awaiting schedule triggers. (Press Ctrl+C to terminate)");

    // Await termination signals (SIGINT from keyboard or SIGTERM from Docker/systemd)
    wait_for_shutdown_signal().await;

    info!("Termination signal received. Starting graceful shutdown...");

    // Gracefully stop scheduler loop to prevent new scheduled runs
    if let Err(err) = scheduler.shutdown().await {
        warn!(error = %err, "Scheduler shutdown encountered an issue");
    }

    // Gracefully terminate browser process within 3-second deadline per SRS NFR 4.3
    let shutdown_timeout = Duration::from_secs(3);
    if tokio::time::timeout(shutdown_timeout, browser_manager.shutdown())
        .await
        .is_err()
    {
        warn!("Browser shutdown timed out after 3 seconds. Forcing process exit.");
    }

    info!("SiteWarden shutdown complete.");
    Ok(())
}

/// Waits for either SIGINT (`Ctrl+C`) or SIGTERM signal.
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to register Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => stream.recv().await,
            Err(e) => {
                error!(error = %e, "Failed to register SIGTERM handler");
                std::future::pending::<Option<()>>().await
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received SIGINT (Ctrl+C)");
        },
        _ = terminate => {
            info!("Received SIGTERM");
        },
    }
}
