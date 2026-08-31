//! Scheduling, cron evaluation, and concurrent test execution pipeline.
//!
//! Conforms to IEEE Std 830-1998 (SRS Section 3.2, 3.3, 3.4).
//! Orchestrates scheduled and on-demand test runs using a Hybrid Execution Architecture:
//! - Pure-Rust Static Engine (`reqwest` + `scraper`) for non-interactive tests (~2MB RAM, ~5ms latency)
//! - On-Demand Headless Browser Engine for interactive steps (Click, TypeText, WaitForSelector)

use crate::alert::{AlertDispatcher, FailureAlert};
use crate::browser::BrowserManager;
use crate::config::{AppConfig, TestCase, TestSuite};
use crate::engine::{execute_test_case, TestCaseResult};
use crate::static_engine::execute_static_test_case;
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use chrono::Utc;
use futures::stream::{self, StreamExt};
use reqwest::Client;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info, warn};

/// Orchestrates test execution across all configured test suites with hybrid execution.
pub async fn run_all_suites(
    shared_config: &Arc<ArcSwap<AppConfig>>,
    alert_dispatcher: &AlertDispatcher,
) -> bool {
    let config = shared_config.load_full();
    let start_time = Utc::now();
    info!(
        timestamp = %start_time.to_rfc3339(),
        suite_count = config.suites.len(),
        total_tests = config.total_tests(),
        total_steps = config.total_steps(),
        concurrency = config.browser_concurrency,
        "Starting smoke test execution cycle"
    );

    let any_dynamic = config.suites.iter().any(|s| !s.is_all_static());
    let http_client = Arc::new(Client::builder().build().unwrap_or_default());

    // If dynamic browser execution is required, launch browser on-demand for this cycle
    let browser = if any_dynamic {
        info!("Interactive tests detected. Launching on-demand Chromium engine...");
        match BrowserManager::launch().await {
            Ok(bm) => Some(Arc::new(bm)),
            Err(err) => {
                error!(error = %err, "Failed to launch on-demand Chromium for dynamic test cycle");
                return false;
            }
        }
    } else {
        info!(
            "All tests are static-executable. Running in pure-Rust engine (zero browser overhead)."
        );
        None
    };

    let mut all_suites_passed = true;

    for suite in &config.suites {
        let suite_passed = run_suite(
            suite,
            &config,
            browser.as_ref(),
            &http_client,
            alert_dispatcher,
        )
        .await;
        if !suite_passed {
            all_suites_passed = false;
        }
    }

    // Immediately shut down browser process if one was launched
    if let Some(b) = browser {
        b.shutdown().await;
        info!("Released on-demand browser resources back to system.");
    }

    info!(
        all_passed = all_suites_passed,
        "Smoke test execution cycle completed."
    );

    all_suites_passed
}

/// Executes a single test suite with concurrency bounded by `browser_concurrency`.
async fn run_suite(
    suite: &TestSuite,
    config: &AppConfig,
    browser: Option<&Arc<BrowserManager>>,
    http_client: &Arc<Client>,
    alert_dispatcher: &AlertDispatcher,
) -> bool {
    info!(
        suite = %suite.name,
        test_count = suite.tests.len(),
        step_count = suite.total_steps(),
        base_url = %suite.base_url,
        is_static = suite.is_all_static(),
        "Executing test suite"
    );

    let concurrency = config.browser_concurrency;
    let base_url = suite.base_url.clone();
    let suite_name = suite.name.clone();
    let timeout_secs = config.timeout_seconds;
    let screenshot_dir = config.screenshot_dir.clone();

    // Stream test executions with bounded concurrency
    let results: Vec<TestCaseResult> = stream::iter(suite.tests.clone())
        .map(|test_case| {
            let browser = browser.cloned();
            let http_client = Arc::clone(http_client);
            let base_url = base_url.clone();
            let suite_name = suite_name.clone();
            let screenshot_dir = screenshot_dir.clone();

            async move {
                if test_case.is_static_executable() {
                    // Pure-Rust static execution
                    execute_static_test_case(
                        &http_client,
                        &base_url,
                        &test_case.name,
                        &test_case.steps,
                    )
                    .await
                } else if let Some(ref b) = browser {
                    // Dynamic browser execution
                    run_single_test_with_recovery(
                        &test_case,
                        &base_url,
                        &suite_name,
                        timeout_secs,
                        &screenshot_dir,
                        b,
                    )
                    .await
                } else {
                    // Fallback safety if no browser was provisioned
                    TestCaseResult {
                        test_name: test_case.name.clone(),
                        success: false,
                        duration: Duration::ZERO,
                        failure: Some(crate::engine::StepFailure {
                            step_index: 0,
                            action_type: "engine_routing".to_string(),
                            error_message:
                                "Dynamic test encountered but browser engine was not initialized"
                                    .to_string(),
                            screenshot_path: None,
                        }),
                    }
                }
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    // Count failures
    let mut failed_count = 0;
    let mut suite_success = true;

    for result in results {
        if !result.success {
            suite_success = false;
            failed_count += 1;
        }
    }

    // If any test in the suite failed, trigger alert dispatcher
    if !suite_success {
        warn!(
            suite = %suite.name,
            failures = failed_count,
            "Test suite incurred failures."
        );

        let alert = FailureAlert {
            suite_name: suite.name.clone(),
            failed_count,
        };

        alert_dispatcher.dispatch(&alert).await;
    } else {
        info!(suite = %suite.name, "All tests in suite passed successfully!");
    }

    suite_success
}

/// Executes a single dynamic test case inside a dedicated tab, with strict timeout and failure screenshot capture.
async fn run_single_test_with_recovery(
    test_case: &TestCase,
    base_url: &str,
    suite_name: &str,
    timeout_secs: u64,
    screenshot_dir: &str,
    browser: &BrowserManager,
) -> TestCaseResult {
    let page = match browser.new_page().await {
        Ok(p) => p,
        Err(err) => {
            error!(test = %test_case.name, error = %err, "Failed to allocate new browser page");
            return TestCaseResult {
                test_name: test_case.name.clone(),
                success: false,
                duration: Duration::ZERO,
                failure: Some(crate::engine::StepFailure {
                    step_index: 0,
                    action_type: "tab_allocation".to_string(),
                    error_message: format!("Browser tab creation failed: {}", err),
                    screenshot_path: None,
                }),
            };
        }
    };

    let test_fut = execute_test_case(&page, base_url, &test_case.name, &test_case.steps);
    let timeout_duration = Duration::from_secs(timeout_secs);

    let mut result = match tokio::time::timeout(timeout_duration, test_fut).await {
        Ok(res) => res,
        Err(_) => {
            error!(
                test = %test_case.name,
                timeout_secs = timeout_secs,
                "Test case timed out"
            );
            TestCaseResult {
                test_name: test_case.name.clone(),
                success: false,
                duration: timeout_duration,
                failure: Some(crate::engine::StepFailure {
                    step_index: 0,
                    action_type: "timeout".to_string(),
                    error_message: format!("Test timed out after {} seconds", timeout_secs),
                    screenshot_path: None,
                }),
            }
        }
    };

    // If test failed, capture screenshot before closing page
    if !result.success {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
        let sanitized_suite = sanitize_filename(suite_name);
        let sanitized_test = sanitize_filename(&test_case.name);
        let filename = format!(
            "{}_{}_{}_failed.png",
            sanitized_suite, sanitized_test, timestamp
        );
        let screenshot_path = PathBuf::from(screenshot_dir).join(filename);

        match BrowserManager::capture_screenshot(&page, &screenshot_path).await {
            Ok(saved_path) => {
                info!(
                    path = ?saved_path,
                    test = %test_case.name,
                    "Saved failure screenshot artifact"
                );
                if let Some(ref mut failure) = result.failure {
                    failure.screenshot_path = Some(saved_path.to_string_lossy().to_string());
                }
            }
            Err(err) => {
                error!(
                    error = %err,
                    test = %test_case.name,
                    "Failed to capture failure screenshot"
                );
            }
        }
    }

    // Always close page tab to release resources
    BrowserManager::close_page(page).await;

    result
}

/// Helper to sanitize strings for filesystem path safety.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Initializes and starts the background cron scheduler daemon.
pub async fn start_scheduler(
    shared_config: Arc<ArcSwap<AppConfig>>,
    alert_dispatcher: AlertDispatcher,
) -> Result<JobScheduler> {
    let scheduler = JobScheduler::new()
        .await
        .context("Failed to create JobScheduler instance")?;

    let config = shared_config.load();
    let cron_expr = config.schedule.clone();
    drop(config);

    let sched_shared_config = Arc::clone(&shared_config);
    let sched_alert = alert_dispatcher.clone();

    // Mutex lock to prevent overlapping test runs if a test cycle exceeds the cron trigger interval
    let execution_lock = Arc::new(Mutex::new(()));

    let job = Job::new_async(cron_expr.as_str(), move |_uuid, _lock| {
        let conf = Arc::clone(&sched_shared_config);
        let alert = sched_alert.clone();
        let exec_lock = Arc::clone(&execution_lock);

        Box::pin(async move {
            let _guard = match exec_lock.try_lock() {
                Ok(guard) => guard,
                Err(_) => {
                    warn!("Previous test execution cycle is still in progress. Skipping scheduled trigger.");
                    return;
                }
            };

            info!("Cron trigger activated. Starting hybrid test execution...");
            run_all_suites(&conf, &alert).await;
        })
    })
    .with_context(|| format!("Failed to parse cron schedule expression: '{}'", cron_expr))?;

    scheduler
        .add(job)
        .await
        .context("Failed to add job to scheduler")?;

    scheduler
        .start()
        .await
        .context("Failed to start scheduler loop")?;

    info!(schedule = %cron_expr, "Scheduler service successfully running");

    Ok(scheduler)
}
