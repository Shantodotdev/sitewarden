//! Screenshot storage maintenance, lifecycle retention, and disk pruning for SiteWarden.
//!
//! # Problem Statement & Storage Defense
//! When SiteWarden runs 24/7 on small cloud VPS instances (often with limited 10–25GB SSDs),
//! test failure screenshots can slowly accumulate over weeks and exhaust server disk space.
//!
//! This module provides:
//! 1. High-speed file modification timestamp (`mtime`) scanning for PNG screenshot artifacts.
//! 2. Dry-run simulation to inspect stale files and preview reclaimable bytes without destructive I/O.
//! 3. Human-readable byte formatting (`B`, `KB`, `MB`, `GB`) for logs and CLI summaries.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Summary metrics returned following a screenshot directory prune operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneSummary {
    /// Total number of PNG files encountered in the screenshot directory.
    pub total_scanned: usize,
    /// Number of stale screenshot files identified and removed (or targeted in dry-run mode).
    pub deleted_count: usize,
    /// Total disk capacity reclaimed in bytes.
    pub reclaimed_bytes: u64,
}

/// Scans the screenshot directory and removes PNG artifacts older than `max_age_days`.
///
/// # Arguments
/// * `screenshot_dir` - Directory path where failure screenshots are saved (e.g. `/app/screenshots`).
/// * `max_age_days` - Files with modification times older than `now - (max_age_days * 86400)` will be removed.
/// * `dry_run` - When `true`, scans and tallies files without deleting them from the filesystem.
///
/// # Returns
/// A `PruneSummary` containing scanned counts, deleted counts, and total reclaimed bytes.
pub fn prune_screenshots(
    screenshot_dir: &Path,
    max_age_days: u64,
    dry_run: bool,
) -> Result<PruneSummary> {
    if !screenshot_dir.exists() || !screenshot_dir.is_dir() {
        return Ok(PruneSummary::default());
    }

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(max_age_days * 86400))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut summary = PruneSummary::default();

    for entry in fs::read_dir(screenshot_dir).with_context(|| {
        format!(
            "Failed to read screenshot directory at: {:?}",
            screenshot_dir
        )
    })? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().is_some_and(|ext| ext == "png") {
            summary.total_scanned += 1;

            if let Ok(metadata) = entry.metadata() {
                let file_size = metadata.len();
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

                if modified <= cutoff {
                    if !dry_run {
                        if let Err(err) = fs::remove_file(&path) {
                            tracing::warn!(path = ?path, error = %err, "Failed to delete screenshot");
                            continue;
                        }
                    }
                    summary.deleted_count += 1;
                    summary.reclaimed_bytes += file_size;
                }
            }
        }
    }

    Ok(summary)
}

/// Formats a raw byte count into a clean, human-readable string (e.g., `"1.42 MB"`).
///
/// # Examples
/// ```
/// use sitewarden::pruner::format_bytes;
/// assert_eq!(format_bytes(500), "500 B");
/// assert_eq!(format_bytes(2048), "2.00 KB");
/// assert_eq!(format_bytes(1048576 * 3), "3.00 MB");
/// ```
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.2} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_prune_screenshots() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test_failure.png");
        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"fake png data 1234567890").unwrap();

        // Pruning with max_age_days = 0 should remove any file modified <= now
        let summary = prune_screenshots(dir.path(), 0, false).unwrap();
        assert_eq!(summary.total_scanned, 1);
        assert_eq!(summary.deleted_count, 1);
        assert_eq!(summary.reclaimed_bytes, 24);
        assert!(!file_path.exists());
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.00 KB");
        assert_eq!(format_bytes(1048576 * 3), "3.00 MB");
    }
}
