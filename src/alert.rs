//! Minimal alert module for SiteWarden.
//!
//! Provides a lightweight notification interface for future alerting backends
//! (such as webhooks, Slack, email, or Telegram).

use tracing::warn;

/// Failure notification details.
#[derive(Debug, Clone)]
pub struct FailureAlert {
    /// Name of the test suite that failed.
    pub suite_name: String,
    /// Number of tests that failed.
    pub failed_count: usize,
}

/// Minimal alert dispatcher stub for future notification channels.
#[derive(Debug, Clone, Default)]
pub struct AlertDispatcher;

impl AlertDispatcher {
    /// Creates a new minimal alert dispatcher.
    pub fn new() -> Self {
        Self
    }

    /// Dispatches failure alerts to configured destinations.
    pub async fn dispatch(&self, alert: &FailureAlert) {
        warn!(
            suite = %alert.suite_name,
            failures = alert.failed_count,
            "Smoke test suite incurred failures. "
        );
    }
}
