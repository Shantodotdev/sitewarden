//! Dynamic configuration file watcher and atomic hot-reloading.
//!
//! Conforms to IEEE Std 830-1998 (SRS FR-1.2).
//! Watches `config.yaml` using `notify`, debounces file modifications for 300ms,
//! and atomically updates shared application state via `arc_swap::ArcSwap`.

use crate::config::AppConfig;
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use notify::{
    event::ModifyKind, Config as NotifyConfig, Event, EventKind, RecommendedWatcher, RecursiveMode,
    Watcher,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, trace};

/// Debounce period for file write events before triggering a reload.
pub const RELOAD_DEBOUNCE_DURATION: Duration = Duration::from_millis(300);

/// Manages filesystem watching and atomic state swapping for runtime configuration.
pub struct ConfigWatcher {
    config_path: PathBuf,
    shared_config: Arc<ArcSwap<AppConfig>>,
    _watcher: RecommendedWatcher,
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

        let (tx, rx) = mpsc::channel(16);
        let (reload_tx, reload_rx) = mpsc::channel(16);

        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    let _ = tx.blocking_send(event);
                }
            },
            NotifyConfig::default(),
        )
        .context("Failed to initialize notify filesystem watcher")?;

        // Watch the parent directory rather than the file directly.
        // This guarantees change events are captured even when editors (Vim, VS Code)
        // perform atomic file replacement via temporary swapfiles and renames.
        let watch_target = if let Some(parent) = canonical_path.parent() {
            parent.to_path_buf()
        } else {
            canonical_path.clone()
        };

        watcher
            .watch(&watch_target, RecursiveMode::NonRecursive)
            .with_context(|| format!("Failed to watch path: {:?}", watch_target))?;

        info!(
            path = ?canonical_path,
            watched_dir = ?watch_target,
            "Registered filesystem watcher for configuration"
        );

        // Spawn async event loop to handle inotify events and debounce rapid saves
        let loop_path = canonical_path.clone();
        let loop_shared = shared_config.clone();

        tokio::spawn(async move {
            let mut event_rx = rx;

            while let Some(event) = event_rx.recv().await {
                // Filter specifically for content modification, atomic renames, and file recreation.
                // Many text editors write to a temporary swapfile and atomically rename/recreate
                // the target file on save, which triggers Create, Modify(Data), or Modify(Name).
                let is_relevant = match event.kind {
                    EventKind::Modify(ModifyKind::Data(_))
                    | EventKind::Modify(ModifyKind::Name(_))
                    | EventKind::Modify(ModifyKind::Any)
                    | EventKind::Create(_) => event
                        .paths
                        .iter()
                        .any(|p| p.file_name() == loop_path.file_name() || p == &loop_path),
                    _ => false,
                };

                if !is_relevant {
                    continue;
                }

                trace!("Detected file modification event. Starting 300ms debounce...");

                // Debounce window: wait for editor to finish writing and flush buffers,
                // then drain any residual queued events for the same edit cycle.
                tokio::time::sleep(RELOAD_DEBOUNCE_DURATION).await;
                while event_rx.try_recv().is_ok() {}

                info!(path = ?loop_path, "Attempting hot-reload of configuration...");

                // Attempt to parse and validate new configuration
                match AppConfig::from_file(&loop_path) {
                    Ok(new_config) => {
                        // Atomic pointer swap: wait-free for all concurrent readers (schedulers/workers)
                        loop_shared.store(Arc::new(new_config));
                        info!("Configuration hot-reloaded successfully into memory via ArcSwap");
                        let _ = reload_tx.send(()).await;
                    }
                    Err(err) => {
                        // Resilience: log detailed error with line context, but strictly preserve
                        // the previous valid configuration in memory so the daemon keeps running.
                        error!(
                            error = %err,
                            "Hot-reload failed! Retaining current active configuration in memory."
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
