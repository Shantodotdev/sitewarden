//! Configuration module for SiteWarden.
//!
//! Provides strongly typed representations of YAML test configurations,
//! declarative schema validation via `validator`, and W3C-compliant CSS selector
//! validation via `scraper`. Conforms to IEEE Std 830-1998 (SRS).

use scraper::Selector;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use thiserror::Error;
use url::Url;
use validator::{Validate, ValidationError};

/// Default concurrency for concurrent browser tabs.
pub const DEFAULT_BROWSER_CONCURRENCY: usize = 2;

/// Default timeout in seconds for a complete test case run.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

/// Default directory where failure screenshots are stored.
pub const DEFAULT_SCREENSHOT_DIR: &str = "./screenshots";

/// Default timeout in milliseconds for `wait_for_selector` step.
pub const DEFAULT_SELECTOR_TIMEOUT_MS: u64 = 5000;

/// Maximum allowable timeout in milliseconds for selector polling (5 minutes).
pub const MAX_SELECTOR_TIMEOUT_MS: u64 = 300_000;

/// Errors that can occur during configuration loading and validation.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read configuration file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse YAML configuration: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Invalid configuration: {0}")]
    Validation(String),
}

/// Custom validator for cron schedule expressions using `croner`.
fn validate_cron_expression(schedule: &str) -> Result<(), ValidationError> {
    let trimmed = schedule.trim();
    if trimmed.is_empty() {
        let mut err = ValidationError::new("empty_schedule");
        err.message = Some("Cron schedule expression cannot be empty".into());
        return Err(err);
    }

    trimmed.parse::<croner::Cron>().map_err(|e| {
        let mut err = ValidationError::new("invalid_cron");
        err.message = Some(format!("Invalid cron expression '{}': {}", schedule, e).into());
        err
    })?;

    Ok(())
}

/// Root configuration structure for SiteWarden daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Cron expression defining test execution frequency (e.g. `0 0 6 * * *` or `0 */30 * * * *`).
    #[validate(custom(function = "validate_cron_expression"))]
    pub schedule: String,

    /// Maximum number of browser tabs running tests concurrently (1 to 64).
    #[validate(range(min = 1, max = 64))]
    #[serde(default = "default_browser_concurrency")]
    pub browser_concurrency: usize,

    /// Global timeout in seconds for executing a single test case (1s to 3600s / 1hr).
    #[validate(range(min = 1, max = 3600))]
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,

    /// Directory where failure screenshots are saved.
    #[validate(length(min = 1))]
    #[serde(default = "default_screenshot_dir")]
    pub screenshot_dir: String,

    /// Configured test suites to execute.
    #[validate(length(min = 1), nested)]
    #[serde(default)]
    pub suites: Vec<TestSuite>,
}

fn default_browser_concurrency() -> usize {
    DEFAULT_BROWSER_CONCURRENCY
}

fn default_timeout_seconds() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

fn default_screenshot_dir() -> String {
    DEFAULT_SCREENSHOT_DIR.to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schedule: "0 0 6 * * *".to_string(),
            browser_concurrency: DEFAULT_BROWSER_CONCURRENCY,
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            screenshot_dir: DEFAULT_SCREENSHOT_DIR.to_string(),
            suites: Vec::new(),
        }
    }
}

/// A suite of related test cases targeting a specific base URL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct TestSuite {
    /// Human-readable name of the suite (e.g., "Marketing Site", "Admin Portal").
    #[validate(length(min = 1))]
    pub name: String,

    /// Root URL against which relative paths are resolved (e.g., `https://example.com`).
    #[validate(url)]
    pub base_url: String,

    /// List of test cases to execute.
    #[validate(length(min = 1), nested)]
    pub tests: Vec<TestCase>,
}

impl TestSuite {
    /// Returns the total number of steps across all test cases in this suite.
    pub fn total_steps(&self) -> usize {
        self.tests.iter().map(|t| t.steps.len()).sum()
    }
}

/// A single end-to-end smoke test case consisting of sequential declarative steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct TestCase {
    /// Human-readable name of the test case (e.g., "Pricing Page Render Check").
    #[validate(length(min = 1))]
    pub name: String,

    /// Sequential steps to execute in a dedicated browser page.
    #[validate(length(min = 1))]
    pub steps: Vec<TestStep>,
}

/// Declarative browser automation steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum TestStep {
    /// Navigate to a target URL or path relative to the suite's `base_url`.
    Navigate {
        /// Relative path (e.g. `/pricing`) or absolute URL (e.g. `https://example.com/pricing`).
        path: String,
    },

    /// Wait for a DOM element matching the CSS selector to be present and visible.
    WaitForSelector {
        /// CSS selector to query.
        selector: String,
        /// Maximum duration to wait in milliseconds. Defaults to 5000ms if omitted.
        #[serde(default)]
        timeout_ms: Option<u64>,
    },

    /// Assert that an element identified by CSS selector contains the expected text.
    AssertText {
        /// CSS selector to query.
        selector: String,
        /// Expected substring inside the element's innerText.
        contains: String,
    },

    /// Assert that an element identified by CSS selector exists and has non-zero dimensions.
    AssertVisible {
        /// CSS selector to query.
        selector: String,
    },

    /// Dispatch a mouse click event to the element matching the CSS selector.
    Click {
        /// CSS selector of the element to click.
        selector: String,
    },

    /// Focus an input element and simulate typing text.
    TypeText {
        /// CSS selector of the input field.
        selector: String,
        /// Text string to type into the element.
        text: String,
    },
}

impl AppConfig {
    /// Loads and parses configuration from a YAML file at the given path.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml_str(&content)
    }

    /// Parses and validates configuration from a YAML string.
    ///
    /// Validates all fields eagerly using declarative `validator` rules
    /// and browser-grade W3C CSS selector parsing via `scraper`.
    pub fn from_yaml_str(yaml_str: &str) -> Result<Self, ConfigError> {
        let config: AppConfig = serde_yaml::from_str(yaml_str)?;
        config.validate()?;
        Ok(config)
    }

    /// Returns the total number of test cases across all configured suites.
    pub fn total_tests(&self) -> usize {
        self.suites.iter().map(|s| s.tests.len()).sum()
    }

    /// Returns the total number of steps across all configured test cases.
    pub fn total_steps(&self) -> usize {
        self.suites.iter().map(|s| s.total_steps()).sum()
    }

    /// Performs strict semantic and structural validation of all configuration parameters.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Run declarative validator rules (cron, ranges, lengths, URLs, nested structs)
        Validate::validate(self).map_err(|e| ConfigError::Validation(e.to_string()))?;

        // Screenshot directory null byte safety
        if self.screenshot_dir.contains('\0') {
            return Err(ConfigError::Validation(
                "screenshot_dir contains invalid null character".to_string(),
            ));
        }

        // Validate uniqueness of suite names
        let mut suite_names = HashSet::new();

        for (suite_idx, suite) in self.suites.iter().enumerate() {
            let suite_name = suite.name.trim();

            if !suite_names.insert(suite_name.to_string()) {
                return Err(ConfigError::Validation(format!(
                    "Duplicate suite name '{}' found at index {}. Suite names must be unique",
                    suite_name, suite_idx
                )));
            }

            // Ensure base_url uses HTTP or HTTPS scheme and has a valid hostname
            let parsed_base = Url::parse(&suite.base_url).map_err(|e| {
                ConfigError::Validation(format!(
                    "Suite '{}' has an invalid base_url '{}': {}",
                    suite.name, suite.base_url, e
                ))
            })?;

            match parsed_base.scheme() {
                "http" | "https" => {}
                scheme => {
                    return Err(ConfigError::Validation(format!(
                        "Suite '{}' base_url has unsupported scheme '{}'. Only 'http' and 'https' are allowed",
                        suite.name, scheme
                    )));
                }
            }

            if parsed_base.host_str().is_none() {
                return Err(ConfigError::Validation(format!(
                    "Suite '{}' base_url '{}' is missing a valid host",
                    suite.name, suite.base_url
                )));
            }

            // Validate uniqueness of test names within this suite
            let mut test_names = HashSet::new();

            for (test_idx, test) in suite.tests.iter().enumerate() {
                let test_name = test.name.trim();

                if !test_names.insert(test_name.to_string()) {
                    return Err(ConfigError::Validation(format!(
                        "Duplicate test name '{}' in suite '{}' at index {}. Test names within a suite must be unique",
                        test_name, suite.name, test_idx
                    )));
                }

                // Validate each individual step action using scraper's CSS selector parser
                for (step_idx, step) in test.steps.iter().enumerate() {
                    step.validate(&suite.name, &parsed_base, &test.name, step_idx)?;
                }
            }
        }

        Ok(())
    }
}

impl TestStep {
    /// Validates parameters and syntax for an individual test step.
    fn validate(
        &self,
        suite: &str,
        base_url: &Url,
        test: &str,
        step_idx: usize,
    ) -> Result<(), ConfigError> {
        match self {
            TestStep::Navigate { path } => {
                let p = path.trim();
                if p.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'navigate' has empty path",
                        step_idx, test, suite
                    )));
                }

                // Case-insensitive check to disallow unsafe / unsupported protocols
                let lower = p.to_ascii_lowercase();
                if lower.starts_with("javascript:")
                    || lower.starts_with("data:")
                    || lower.starts_with("file:")
                {
                    return Err(ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'navigate' uses unsupported protocol in path '{}'",
                        step_idx, test, suite, path
                    )));
                }

                // If absolute URL, ensure it parses with HTTP/HTTPS; if relative, verify it joins cleanly
                if lower.starts_with("http://") || lower.starts_with("https://") {
                    Url::parse(p).map_err(|e| {
                        ConfigError::Validation(format!(
                            "Step {} in test '{}' ({}) 'navigate' has invalid absolute URL '{}': {}",
                            step_idx, test, suite, path, e
                        ))
                    })?;
                } else {
                    base_url.join(p).map_err(|e| {
                        ConfigError::Validation(format!(
                            "Step {} in test '{}' ({}) 'navigate' failed to join relative path '{}' with base URL '{}': {}",
                            step_idx, test, suite, path, base_url, e
                        ))
                    })?;
                }
            }
            TestStep::WaitForSelector {
                selector,
                timeout_ms,
            } => {
                validate_css_selector(selector).map_err(|e| {
                    ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'wait_for_selector' has invalid selector: {}",
                        step_idx, test, suite, e
                    ))
                })?;

                if let Some(t) = timeout_ms {
                    if *t < 10 {
                        return Err(ConfigError::Validation(format!(
                            "Step {} in test '{}' ({}) 'wait_for_selector' timeout_ms must be >= 10ms (got {}ms)",
                            step_idx, test, suite, t
                        )));
                    }
                    if *t > MAX_SELECTOR_TIMEOUT_MS {
                        return Err(ConfigError::Validation(format!(
                            "Step {} in test '{}' ({}) 'wait_for_selector' timeout_ms cannot exceed {}ms (5 minutes)",
                            step_idx, test, suite, MAX_SELECTOR_TIMEOUT_MS
                        )));
                    }
                }
            }
            TestStep::AssertText { selector, contains } => {
                validate_css_selector(selector).map_err(|e| {
                    ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'assert_text' has invalid selector: {}",
                        step_idx, test, suite, e
                    ))
                })?;

                if contains.trim().is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'assert_text' contains string cannot be empty or whitespace-only",
                        step_idx, test, suite
                    )));
                }
                if contains.len() > 50_000 {
                    return Err(ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'assert_text' contains text exceeds 50,000 characters",
                        step_idx, test, suite
                    )));
                }
            }
            TestStep::AssertVisible { selector } => {
                validate_css_selector(selector).map_err(|e| {
                    ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'assert_visible' has invalid selector: {}",
                        step_idx, test, suite, e
                    ))
                })?;
            }
            TestStep::Click { selector } => {
                validate_css_selector(selector).map_err(|e| {
                    ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'click' has invalid selector: {}",
                        step_idx, test, suite, e
                    ))
                })?;
            }
            TestStep::TypeText { selector, text } => {
                validate_css_selector(selector).map_err(|e| {
                    ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'type_text' has invalid selector: {}",
                        step_idx, test, suite, e
                    ))
                })?;

                if text.len() > 50_000 {
                    return Err(ConfigError::Validation(format!(
                        "Step {} in test '{}' ({}) 'type_text' text exceeds 50,000 characters",
                        step_idx, test, suite
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Validates that a string is a well-formed W3C CSS selector using Mozilla/Servo's `scraper::Selector`.
pub fn validate_css_selector(selector: &str) -> Result<(), String> {
    let s = selector.trim();
    if s.is_empty() {
        return Err("CSS selector cannot be empty or whitespace-only".to_string());
    }

    Selector::parse(s)
        .map(|_| ())
        .map_err(|e| format!("Invalid CSS selector '{}': {:?}", s, e))
}

impl TestCase {
    /// Returns true if this test case only contains static steps (Navigate, AssertText, AssertVisible)
    /// and does not require JavaScript execution or interactive browser operations (Click, TypeText, WaitForSelector).
    pub fn is_static_executable(&self) -> bool {
        self.steps.iter().all(|step| match step {
            TestStep::Navigate { .. } => true,
            TestStep::AssertText { .. } => true,
            TestStep::AssertVisible { .. } => true,
            TestStep::WaitForSelector { .. }
            | TestStep::Click { .. }
            | TestStep::TypeText { .. } => false,
        })
    }
}

impl TestSuite {
    /// Returns true if all test cases in this suite are executable via the pure-Rust static engine.
    pub fn is_all_static(&self) -> bool {
        self.tests.iter().all(|test| test.is_static_executable())
    }
}
