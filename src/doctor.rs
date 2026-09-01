//! Diagnostic health checks, environment verification, and pre-flight validation for SiteWarden.
//!
//! # Objective
//! `sitewarden doctor` inspects the host or container environment to proactively identify
//! configuration errors, missing browser dependencies, storage permission traps, or network issues
//! before they cause runtime test failures.
//!
//! # Verified Diagnostic Areas
//! 1. **Configuration:** Validates file existence, YAML syntax, and schema correctness.
//! 2. **Storage Permissions:** Performs a test write & delete in `screenshot_dir` to ensure UID/GID permissions allow artifact saving.
//! 3. **Browser Engine:** Searches known system paths (`/usr/bin/chrome-headless-shell`, `/usr/bin/google-chrome-stable`, etc.) for Chromium executables.
//! 4. **Network & DNS:** Probes outbound HTTPS connectivity to verify DNS resolution and firewall egress.

use crate::browser::BrowserManager;
use crate::config::AppConfig;
use reqwest::Client;
use std::fs;
use std::path::Path;
use std::time::Duration;

/// Individual diagnostic check result with category, title, status, and descriptive details.
#[derive(Debug, Clone)]
pub struct DoctorCheck {
    /// High-level diagnostic domain (e.g., `"Config"`, `"Storage"`, `"Engine"`, `"Network"`).
    pub category: &'static str,
    /// Human-readable title of the check.
    pub name: &'static str,
    /// `true` if the check passed without warnings or errors.
    pub passed: bool,
    /// Detailed diagnostic output, path findings, or error descriptions.
    pub details: String,
}

/// Executes all system and environment diagnostic checks against the given configuration path.
///
/// Runs checks sequentially and returns a list of `DoctorCheck` results suitable for CLI rendering.
pub async fn run_diagnostics(config_path: &Path) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();

    // 1. Check Configuration File
    if !config_path.exists() {
        checks.push(DoctorCheck {
            category: "Config",
            name: "Configuration File Existence",
            passed: false,
            details: format!("File not found at: {:?}", config_path),
        });
    } else {
        match AppConfig::from_file(config_path) {
            Ok(cfg) => {
                checks.push(DoctorCheck {
                    category: "Config",
                    name: "Configuration Syntax & Schema",
                    passed: true,
                    details: format!(
                        "Valid YAML • Schedule: '{}' • {} Suites • {} Steps",
                        cfg.schedule,
                        cfg.suites.len(),
                        cfg.total_steps()
                    ),
                });

                // Check screenshot directory permissions
                let screenshot_dir = Path::new(&cfg.screenshot_dir);
                let write_check = test_directory_write_permissions(screenshot_dir);
                checks.push(DoctorCheck {
                    category: "Storage",
                    name: "Screenshot Directory Permissions",
                    passed: write_check.is_ok(),
                    details: match write_check {
                        Ok(()) => format!("Writable directory at: {:?}", screenshot_dir),
                        Err(err) => {
                            if cfg.screenshot_dir.starts_with("/app")
                                && !Path::new("/.dockerenv").exists()
                            {
                                format!(
                                    "Cannot write to {:?}: {} (Hint: '/app' is a Docker path. Use relative 'screenshots' when running on host).",
                                    screenshot_dir, err
                                )
                            } else {
                                format!("Cannot write to {:?}: {}", screenshot_dir, err)
                            }
                        }
                    },
                });
            }
            Err(err) => {
                checks.push(DoctorCheck {
                    category: "Config",
                    name: "Configuration Syntax & Schema",
                    passed: false,
                    details: format!("Validation error: {:#}", err),
                });
            }
        }
    }

    // 2. Check Headless Chromium Engine
    match BrowserManager::detect_chrome_executable() {
        Some(bin_path) => {
            checks.push(DoctorCheck {
                category: "Engine",
                name: "Headless Chromium / Chrome Shell",
                passed: true,
                details: format!("Found executable at: {:?}", bin_path),
            });
        }
        None => {
            checks.push(DoctorCheck {
                category: "Engine",
                name: "Headless Chromium / Chrome Shell",
                passed: false,
                details: "No Chromium binary found. (Static tests will still function in pure-Rust mode)."
                    .to_string(),
            });
        }
    }

    // 3. Check Outbound Network & DNS Connectivity
    let http_client = Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap_or_default();

    match http_client.get("https://example.com").send().await {
        Ok(resp) => {
            checks.push(DoctorCheck {
                category: "Network",
                name: "Outbound HTTPS & DNS Connectivity",
                passed: resp.status().is_success(),
                details: format!("Connected to https://example.com (HTTP {})", resp.status()),
            });
        }
        Err(err) => {
            checks.push(DoctorCheck {
                category: "Network",
                name: "Outbound HTTPS & DNS Connectivity",
                passed: false,
                details: format!("Connection failed: {}", err),
            });
        }
    }

    checks
}

/// Tests whether the application has write and delete permissions on the given directory.
///
/// Creates parent directories if missing, writes a temporary sentinel file `.sitewarden_doctor_test.tmp`,
/// and removes it immediately to verify full lifecycle file permissions.
fn test_directory_write_permissions(dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    let test_file = dir.join(".sitewarden_doctor_test.tmp");
    fs::write(&test_file, b"permission_test")?;
    fs::remove_file(&test_file)?;
    Ok(())
}
