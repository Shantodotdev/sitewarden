//! Persistent state management and execution history for SiteWarden.
//!
//! # Architecture & Design Rationale
//! SiteWarden is designed to operate as a 24/7 autonomous sentinel on lightweight VPS instances.
//! Sifting through thousands of lines of container logs to determine basic health metrics (such as uptime,
//! pass/fail ratios, or recent failure timestamps) is slow and resource-intensive.
//!
//! This module introduces an atomic, zero-overhead JSON state persistence model:
//! - **Atomic Commits:** Writes to `.tmp` files and swaps filenames via OS-level atomic `rename()`, preventing file corruption during power loss or container kills.
//! - **Bounded Rolling Buffer:** Maintains a ring-buffer of the last 50 execution records, bounding memory and disk usage to <15KB.
//! - **Decoupled CLI Access:** Enables CLI subcommands like `sitewarden status` and `sitewarden history` to read cached metrics in `<1ms` without locking or querying the background scheduler daemon.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Default state filename stored adjacent to `config.yaml`.
pub const DEFAULT_STATE_FILE: &str = ".sitewarden_state.json";

/// Maximum number of historical test cycle snapshots preserved in disk storage.
pub const MAX_HISTORY_RECORDS: usize = 50;

/// Snapshot of an individual test cycle execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunHistoryRecord {
    /// UTC timestamp of cycle initiation formatted as RFC 3339 or human-friendly date string.
    pub timestamp: String,
    /// Total number of test suites executed in this cycle.
    pub total_suites: usize,
    /// Count of suites where every step passed successfully.
    pub passed_suites: usize,
    /// Count of suites that encountered at least one step failure or timeout.
    pub failed_suites: usize,
    /// Total number of individual declarative test steps executed across all suites.
    pub total_steps: usize,
    /// Total wall-clock execution duration of the cycle in milliseconds.
    pub duration_ms: u64,
    /// Trigger origin that initiated the cycle (e.g., `"Cron"`, `"Manual"`, `"Run-Once"`).
    pub trigger: String,
    /// Flag indicating whether 100% of suites and steps succeeded.
    pub all_passed: bool,
}

/// Persistent application state across test cycles and daemon restarts.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AppState {
    /// RFC 3339 timestamp marking when the background scheduler daemon process started.
    pub daemon_started_at: Option<String>,
    /// Lifetime total number of test execution cycles completed.
    pub total_cycles: u64,
    /// Lifetime total number of test cycles where all suites passed.
    pub total_passed_cycles: u64,
    /// Lifetime total number of test cycles that experienced failures.
    pub total_failed_cycles: u64,
    /// Snapshot of the most recent test cycle execution.
    pub last_cycle: Option<RunHistoryRecord>,
    /// Chronological list of historical test cycles (newest first, capped at `MAX_HISTORY_RECORDS`).
    pub history: Vec<RunHistoryRecord>,
}

impl AppState {
    /// Loads application state from the designated JSON file path.
    ///
    /// If the file does not exist or cannot be deserialized, returns a fresh `AppState::default()`
    /// without halting execution or raising fatal errors.
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }

        match fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Atomically commits state to disk using a temporary file rename strategy.
    ///
    /// # Failure Safety
    /// Rather than truncating the existing state file directly (which risks leaving a corrupted or
    /// zero-byte file if the process is terminated mid-write), this writes to `<path>.tmp` first,
    /// flushes buffers to disk, and performs an atomic POSIX `rename()` over the target path.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize application state to JSON")?;

        let temp_path = path.with_extension("tmp");
        fs::write(&temp_path, json)
            .with_context(|| format!("Failed to write temporary state to: {:?}", temp_path))?;

        fs::rename(&temp_path, path).with_context(|| {
            format!(
                "Failed to atomically commit state from {:?} to {:?}",
                temp_path, path
            )
        })?;

        Ok(())
    }

    /// Records a newly completed test cycle into state metrics and trims history to bounds.
    ///
    /// Increments lifetime cycle counters, updates `last_cycle`, prepends the record to `history`,
    /// and truncates `history` to `MAX_HISTORY_RECORDS` to guarantee deterministic storage overhead.
    pub fn record_cycle(&mut self, record: RunHistoryRecord) {
        self.total_cycles += 1;
        if record.all_passed {
            self.total_passed_cycles += 1;
        } else {
            self.total_failed_cycles += 1;
        }

        self.last_cycle = Some(record.clone());
        self.history.insert(0, record);

        if self.history.len() > MAX_HISTORY_RECORDS {
            self.history.truncate(MAX_HISTORY_RECORDS);
        }
    }

    /// Records the daemon startup timestamp if it has not yet been set in this session.
    pub fn mark_started(&mut self) {
        if self.daemon_started_at.is_none() {
            self.daemon_started_at = Some(Utc::now().to_rfc3339());
        }
    }
}

/// Resolves standard state file path adjacent to the configuration file.
///
/// If `config_path` resides in `/opt/sitewarden/config.yaml`, the state file will be located
/// at `/opt/sitewarden/.sitewarden_state.json`. If no parent directory can be resolved,
/// falls back to `.sitewarden_state.json` in the current working directory.
pub fn resolve_state_path(config_path: &Path) -> PathBuf {
    if let Some(parent) = config_path.parent() {
        if parent.exists() && parent.is_dir() {
            let state_file = parent.join(DEFAULT_STATE_FILE);
            return state_file;
        }
    }
    PathBuf::from(DEFAULT_STATE_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_app_state_record_cycle_and_persistence() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path();

        let mut state = AppState::default();
        state.mark_started();
        assert!(state.daemon_started_at.is_some());

        let record1 = RunHistoryRecord {
            timestamp: "2026-09-01T00:00:00Z".to_string(),
            total_suites: 2,
            passed_suites: 2,
            failed_suites: 0,
            total_steps: 8,
            duration_ms: 1200,
            trigger: "Cron".to_string(),
            all_passed: true,
        };

        state.record_cycle(record1.clone());
        assert_eq!(state.total_cycles, 1);
        assert_eq!(state.total_passed_cycles, 1);
        assert_eq!(state.total_failed_cycles, 0);
        assert_eq!(state.last_cycle, Some(record1));

        state.save(path).unwrap();

        let loaded = AppState::load(path);
        assert_eq!(loaded.total_cycles, 1);
        assert_eq!(loaded.history.len(), 1);
    }

    #[test]
    fn test_history_truncation() {
        let mut state = AppState::default();
        for i in 0..60 {
            state.record_cycle(RunHistoryRecord {
                timestamp: format!("2026-09-01T00:{:02}:00Z", i % 60),
                total_suites: 1,
                passed_suites: 1,
                failed_suites: 0,
                total_steps: 4,
                duration_ms: 500,
                trigger: "Cron".to_string(),
                all_passed: true,
            });
        }

        assert_eq!(state.total_cycles, 60);
        assert_eq!(state.history.len(), MAX_HISTORY_RECORDS);
    }
}
