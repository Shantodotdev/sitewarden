//! Dynamic configuration file watcher and atomic hot-reloading.
//!
//! Conforms to IEEE Std 830-1998 (SRS FR-1.2).
//! Watches `config.yaml` using a hybrid approach:
//! 1. Native `inotify` file system events for instantaneous local edits.
//! 2. 1-second mtime/content-hash polling fallback to guarantee reliable detection
//!    across Docker bind-mounts and atomic file replacements from editors (Vim, Nano, VS Code).
//!
//! Updates shared application state atomically via `arc_swap::ArcSwap` with zero daemon downtime.

use crate::config::AppConfig;
use anyhow::Result;
use arc_swap::ArcSwap;
use notify::{
    event::ModifyKind, Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode,
    Watcher,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use tracing::{error, info, trace};

/// Debounce period for file write events before triggering a reload.
pub const RELOAD_DEBOUNCE_DURATION: Duration = Duration::from_millis(300);

/// Polling fallback interval to ensure Docker bind mounts detect modifications reliably.
pub const POLLING_FALLBACK_INTERVAL: Duration = Duration::from_secs(1);

/// Manages filesystem watching and atomic state swapping for runtime configuration.
pub struct ConfigWatcher {
    config_path: PathBuf,
    shared_config: Arc<ArcSwap<AppConfig>>,
    _watcher: Option<RecommendedWatcher>,
}

impl ConfigWatcher {
    /// Initializes a file watcher on the target configuration path and returns the manager
    /// along with a receiver for reload notifications.
    pub fn new<P: AsRef<Path>>(
        config_path: P,
        shared_config: Arc<ArcSwap<AppConfig>>,
    ) -> Result<(Self, mpsc::Receiver<()>)> {
        let path = config_path.as_ref().to_path_buf();
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());

        let (event_tx, mut event_rx) = mpsc::channel::<()>(16);
        let (reload_tx, reload_rx) = mpsc::channel(16);

        // 1. Setup inotify watcher (best-effort for native environments)
        let notify_tx = event_tx.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    let is_relevant = matches!(
                        event.kind,
                        EventKind::Modify(ModifyKind::Data(_))
                            | EventKind::Modify(ModifyKind::Name(_))
                            | EventKind::Modify(ModifyKind::Any)
                            | EventKind::Create(_)
                    );
                    if is_relevant {
                        let _ = notify_tx.blocking_send(());
                    }
                }
            },
            NotifyConfig::default(),
        )
        .ok();

        if let Some(ref mut w) = watcher {
            // Watch the direct file path
            let _ = w.watch(&canonical_path, RecursiveMode::NonRecursive);
            // Also watch parent directory if available for atomic rename capture
            if let Some(parent) = canonical_path.parent() {
                let _ = w.watch(parent, RecursiveMode::NonRecursive);
            }
        }

        info!(
            path = ?canonical_path,
            "Registered hybrid filesystem watcher for live configuration"
        );

        // 2. Setup background polling fallback task for Docker bind-mount resilience
        let poll_path = canonical_path.clone();
        let poll_tx = event_tx;

        tokio::spawn(async move {
            let mut last_mtime: Option<SystemTime> = std::fs::metadata(&poll_path)
                .and_then(|m| m.modified())
                .ok();
            let mut last_content = std::fs::read_to_string(&poll_path).unwrap_or_default();

            loop {
                tokio::time::sleep(POLLING_FALLBACK_INTERVAL).await;

                let current_mtime = std::fs::metadata(&poll_path)
                    .and_then(|m| m.modified())
                    .ok();

                if current_mtime != last_mtime {
                    // Check if content actually changed
                    if let Ok(current_content) = std::fs::read_to_string(&poll_path) {
                        if current_content != last_content {
                            last_mtime = current_mtime;
                            last_content = current_content;
                            trace!("Polling fallback detected modification in config file");
                            let _ = poll_tx.send(()).await;
                        }
                    }
                }
            }
        });

        // 3. Central Debounce and Hot-Reload Processing Task
        let loop_path = canonical_path.clone();
        let loop_shared = shared_config.clone();

        tokio::spawn(async move {
            while event_rx.recv().await.is_some() {
                trace!("Detected config modification event. Debouncing...");

                // Debounce window to let editor finish writing
                tokio::time::sleep(RELOAD_DEBOUNCE_DURATION).await;
                while event_rx.try_recv().is_ok() {}

                info!(path = ?loop_path, "Attempting hot-reload of configuration...");

                // Attempt to parse and validate new configuration
                match AppConfig::from_file(&loop_path) {
                    Ok(new_config) => {
                        let suites_count = new_config.suites.len();
                        let total_tests = new_config.total_tests();
                        let schedule = new_config.schedule.clone();

                        // Atomic pointer swap: wait-free for all concurrent readers
                        loop_shared.store(Arc::new(new_config));

                        info!(
                            schedule = %schedule,
                            suites_count = suites_count,
                            total_tests = total_tests,
                            "Configuration hot-reloaded successfully into memory via ArcSwap"
                        );
                        let _ = reload_tx.send(()).await;
                    }
                    Err(err) => {
                        // Resilience: log detailed error with context, but preserve active memory state
                        error!(
                            error = %err,
                            "Hot-reload failed validation! Retaining current active configuration in memory."
                        );
                    }
                }
            }
        });

        Ok((
            Self {
                config_path: canonical_path,
                shared_config,
                _watcher: watcher,
            },
            reload_rx,
        ))
    }

    /// Returns a copy of the canonical config path being monitored.
    pub fn path(&self) -> &Path {
        &self.config_path
    }

    /// Loads the latest snapshot of the configuration.
    pub fn current_config(&self) -> Arc<AppConfig> {
        self.shared_config.load_full()
    }
}
