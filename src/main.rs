//! SiteWarden Daemon CLI Entrypoint.
//!
//! Conforms to IEEE Std 830-1998 (SRS Specification).
//! Provides command-line options, signal handling for graceful shutdown,
//! hot-reload initialization, and execution orchestration with an on-demand browser lifecycle.

use anyhow::Result;
use arc_swap::ArcSwap;
use clap::Parser;
use sitewarden::alert::AlertDispatcher;
use sitewarden::config::AppConfig;
use sitewarden::scheduler::{run_all_suites, start_scheduler};
use sitewarden::watcher::ConfigWatcher;
use std::path::PathBuf;
use std::sync::Arc;
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

    // Resolve configuration file path with standard fallbacks
    let config_path = if args.config.exists() {
        args.config
    } else if std::path::Path::new("/app/config/config.yaml").exists() {
        PathBuf::from("/app/config/config.yaml")
    } else if std::path::Path::new("/app/config.yaml").exists() {
        PathBuf::from("/app/config.yaml")
    } else {
        error!(
            path = ?args.config,
            "Configuration file not found. Please provide a valid config file."
        );
        std::process::exit(1);
    };

    let initial_config = match AppConfig::from_file(&config_path) {
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

    let shared_config = Arc::new(ArcSwap::from_pointee(initial_config));
    let alert_dispatcher = AlertDispatcher::new();

    // Handle single-run mode (e.g. CI/CD pipelines, one-off test triggers, docker health checks)
    if args.run_once {
        info!("Executing in --run-once mode");
        let passed = run_all_suites(&shared_config, &alert_dispatcher).await;

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
        match ConfigWatcher::new(&config_path, Arc::clone(&shared_config)) {
            Ok(w) => w,
            Err(err) => {
                error!(error = %err, "Failed to initialize configuration file watcher");
                std::process::exit(1);
            }
        };

    // Start background cron scheduler daemon (Chromium is spawned on-demand per schedule trigger)
    let scheduler_handle = Arc::new(tokio::sync::Mutex::new(
        match start_scheduler(Arc::clone(&shared_config), alert_dispatcher.clone()).await {
            Ok(sched) => sched,
            Err(err) => {
                error!(error = %err, "Failed to start cron scheduler");
                std::process::exit(1);
            }
        },
    ));

    // Spawn reload listener task to record runtime configuration swaps and update scheduler if schedule changed
    let reload_shared_config = Arc::clone(&shared_config);
    let reload_alert_dispatcher = alert_dispatcher.clone();
    let reload_scheduler_handle = Arc::clone(&scheduler_handle);

    tokio::spawn(async move {
        let mut last_schedule = reload_shared_config.load().schedule.clone();

        while reload_rx.recv().await.is_some() {
            let current_config = reload_shared_config.load();
            info!(
                suites = current_config.suites.len(),
                tests = current_config.total_tests(),
                schedule = %current_config.schedule,
                "Live configuration hot-reload applied to memory."
            );

            if current_config.schedule != last_schedule {
                info!(
                    old_schedule = %last_schedule,
                    new_schedule = %current_config.schedule,
                    "Cron schedule expression changed. Rescheduling background daemon..."
                );

                let mut sched_lock = reload_scheduler_handle.lock().await;
                if let Err(err) = sched_lock.shutdown().await {
                    warn!(error = %err, "Issue stopping previous scheduler during reschedule");
                }

                match start_scheduler(
                    Arc::clone(&reload_shared_config),
                    reload_alert_dispatcher.clone(),
                )
                .await
                {
                    Ok(new_sched) => {
                        *sched_lock = new_sched;
                        last_schedule = current_config.schedule.clone();
                        info!("Scheduler successfully updated with new cron schedule.");
                    }
                    Err(err) => {
                        error!(error = %err, "Failed to restart scheduler with new cron expression");
                    }
                }
            }
        }
    });

    info!("SiteWarden daemon is actively monitoring and awaiting schedule triggers. (Press Ctrl+C to terminate)");

    // Await termination signals (SIGINT from keyboard or SIGTERM from Docker/systemd)
    wait_for_shutdown_signal().await;

    info!("Termination signal received. Starting graceful shutdown...");

    // Gracefully stop scheduler loop to prevent new scheduled runs
    let mut sched_lock = scheduler_handle.lock().await;
    if let Err(err) = sched_lock.shutdown().await {
        warn!(error = %err, "Scheduler shutdown encountered an issue");
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
