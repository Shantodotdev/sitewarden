//! GitHub release checking, semver comparison, and version upgrade manager for SiteWarden.
//!
//! # Purpose & Flow
//! Autonomous monitoring tools need a simple mechanism for operators to check if updates are available
//! and execute upgrades with minimal cognitive load.
//!
//! This module handles:
//! 1. Querying the GitHub REST API for the latest published release tag.
//! 2. Parsing and comparing semantic versions (e.g., `v0.2.0` > `0.1.0`).
//! 3. Inspecting the host runtime environment (detecting containerized Docker VPS vs standalone native binary).
//! 4. Generating precise, copy-pasteable 1-line upgrade commands tailored to the detected environment.

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

/// Official GitHub repository path for release queries.
pub const GITHUB_REPO: &str = "Shantodotdev/sitewarden";

/// Public GitHub REST API endpoint for the latest release.
pub const RELEASES_API_URL: &str =
    "https://api.github.com/repos/Shantodotdev/sitewarden/releases/latest";

/// Raw JSON payload schema returned by the GitHub Releases API.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRelease {
    /// Git release tag (e.g., `"v0.2.0"`).
    pub tag_name: String,
    /// Release title or milestone heading.
    pub name: Option<String>,
    /// Markdown description and changelog notes.
    pub body: Option<String>,
    /// Canonical HTML URL to the release page on GitHub.
    pub html_url: String,
    /// ISO 8601 publication timestamp.
    pub published_at: Option<String>,
}

/// Structured summary of current vs available version details.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// Currently running binary version parsed from Cargo.toml.
    pub current_version: String,
    /// Latest release version tag available on GitHub.
    pub latest_version: String,
    /// Human-readable title of the latest release.
    pub release_name: String,
    /// Web URL to view full changelog on GitHub.
    pub release_url: String,
    /// Markdown changelog body of the latest release.
    pub release_notes: String,
    /// True if the GitHub version is strictly newer than the current binary.
    pub update_available: bool,
}

/// Execution environment classification for generating tailored upgrade commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentType {
    /// Running inside a Docker container (e.g., Docker Compose on Linux VPS).
    Docker,
    /// Running as a native binary installed directly on the host OS.
    Standalone,
}

/// Detects whether SiteWarden is running in a Docker container or as a standalone host binary.
///
/// Checks for the presence of standard container filesystem markers (`/.dockerenv` or `/app/sitewarden`).
pub fn detect_environment() -> EnvironmentType {
    if Path::new("/.dockerenv").exists()
        || Path::new("/app/sitewarden").exists()
        || std::env::var("DOCKER_CONTAINER").is_ok()
    {
        EnvironmentType::Docker
    } else {
        EnvironmentType::Standalone
    }
}

pub const TAGS_API_URL: &str = "https://api.github.com/repos/Shantodotdev/sitewarden/tags";

/// Minimal schema representing a GitHub tag entry.
#[derive(Debug, Deserialize)]
struct GitHubTag {
    name: String,
}

/// Queries the GitHub REST API to determine if a newer version of SiteWarden has been published.
///
/// # Network Constraints & Fail-Open Behavior
/// Uses a strict 5-second connection timeout and custom `User-Agent` header as mandated by GitHub API guidelines.
/// 1. Queries `/releases/latest` for formal published releases and changelog notes.
/// 2. If `/releases/latest` returns 404 (e.g. only git tags exist), falls back to querying `/tags`.
/// 3. If neither exists or the network request fails, gracefully returns the current version.
pub async fn check_latest_release(client: &Client) -> Result<UpdateInfo> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();

    let user_agent = format!(
        "SiteWarden/{} (https://github.com/{})",
        current_version, GITHUB_REPO
    );

    let request = client
        .get(RELEASES_API_URL)
        .header("User-Agent", &user_agent)
        .header("Accept", "application/vnd.github.v3+json")
        .timeout(Duration::from_secs(5));

    if let Ok(response) = request.send().await {
        if response.status().is_success() {
            if let Ok(body_text) = response.text().await {
                if let Ok(release) = serde_json::from_str::<GitHubRelease>(&body_text) {
                    let latest_clean = release.tag_name.trim_start_matches('v').to_string();
                    let update_available = is_newer_version(&current_version, &latest_clean);

                    return Ok(UpdateInfo {
                        current_version,
                        latest_version: latest_clean,
                        release_name: release
                            .name
                            .unwrap_or_else(|| format!("Release {}", release.tag_name)),
                        release_url: release.html_url,
                        release_notes: release.body.unwrap_or_default(),
                        update_available,
                    });
                }
            }
        } else if response.status() == reqwest::StatusCode::NOT_FOUND {
            // Fallback: check git tags if no formal GitHub Release exists yet
            let tag_request = client
                .get(TAGS_API_URL)
                .header("User-Agent", &user_agent)
                .header("Accept", "application/vnd.github.v3+json")
                .timeout(Duration::from_secs(5));

            if let Ok(tag_resp) = tag_request.send().await {
                if tag_resp.status().is_success() {
                    if let Ok(tag_body) = tag_resp.text().await {
                        if let Ok(tags) = serde_json::from_str::<Vec<GitHubTag>>(&tag_body) {
                            if let Some(first_tag) = tags.first() {
                                let latest_clean =
                                    first_tag.name.trim_start_matches('v').to_string();
                                let update_available =
                                    is_newer_version(&current_version, &latest_clean);

                                return Ok(UpdateInfo {
                                    current_version,
                                    latest_version: latest_clean,
                                    release_name: format!("Tag {}", first_tag.name),
                                    release_url: format!(
                                        "https://github.com/{}/releases/tag/{}",
                                        GITHUB_REPO, first_tag.name
                                    ),
                                    release_notes: "Git tag release on GitHub".to_string(),
                                    update_available,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Default fallback: return current version
    Ok(UpdateInfo {
        current_version: current_version.clone(),
        latest_version: current_version,
        release_name: "Current Version".to_string(),
        release_url: format!("https://github.com/{}", GITHUB_REPO),
        release_notes: "No newer releases found.".to_string(),
        update_available: false,
    })
}

/// Evaluates whether the `latest` semver string is strictly newer than `current`.
///
/// Splits numeric components separated by dots (`.`) and ignores pre-release tags (`-beta`, `-rc1`).
///
/// # Examples
/// ```
/// use sitewarden::updater::is_newer_version;
/// assert!(is_newer_version("0.1.0", "0.2.0"));
/// assert!(is_newer_version("0.1.0", "1.0.0"));
/// assert!(!is_newer_version("0.2.0", "0.1.0"));
/// assert!(!is_newer_version("0.1.0", "0.1.0"));
/// ```
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse_semver = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|s| s.split('-').next()) // strip pre-release suffixes
            .filter_map(|s| s.parse::<u32>().ok())
            .collect()
    };

    let curr_parts = parse_semver(current);
    let latest_parts = parse_semver(latest);

    latest_parts > curr_parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(is_newer_version("0.1.0", "1.0.0"));
        assert!(is_newer_version("0.1.0", "0.1.1"));
        assert!(!is_newer_version("0.2.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
    }
}
