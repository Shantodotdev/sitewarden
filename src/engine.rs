//! Declarative step execution engine.
//!
//! Interprets and executes test steps against a live Chromium tab,
//! evaluating DOM assertions, handling network navigations, and capturing errors.

use crate::config::{TestStep, DEFAULT_SELECTOR_TIMEOUT_MS};
use anyhow::{bail, Context, Result};
use chromiumoxide::page::Page;
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::debug;
use url::Url;

/// Result of executing a single test case.
#[derive(Debug, Clone)]
pub struct TestCaseResult {
    /// Name of the executed test.
    pub test_name: String,

    /// Whether all steps passed successfully.
    pub success: bool,

    /// Execution duration for the test.
    pub duration: Duration,

    /// Details if a failure occurred.
    pub failure: Option<StepFailure>,
}

/// Information describing why a specific test step failed.
#[derive(Debug, Clone)]
pub struct StepFailure {
    /// 0-indexed step number where execution stopped.
    pub step_index: usize,

    /// The step action that failed.
    pub action_type: String,

    /// Detailed description of the error or assertion mismatch.
    pub error_message: String,

    /// Path to the failure screenshot, if captured.
    pub screenshot_path: Option<String>,
}

/// Executes all steps of a test case sequentially in the provided browser page.
pub async fn execute_test_case(
    page: &Page,
    base_url: &str,
    test_name: &str,
    steps: &[TestStep],
) -> TestCaseResult {
    let start_time = Instant::now();
    println!(
        "  {}▶ Test: {}{}",
        crate::report::BOLD_CYAN,
        test_name,
        crate::report::RESET
    );

    for (index, step) in steps.iter().enumerate() {
        let step_start = Instant::now();
        let action = crate::report::step_action_name(step);
        let target = crate::report::step_target_desc(step);

        match execute_step(page, base_url, step).await {
            Ok(_) => {
                let step_dur = step_start.elapsed();
                println!(
                    "{}",
                    crate::report::format_step_log(
                        index,
                        steps.len(),
                        action,
                        &target,
                        step_dur,
                        true
                    )
                );
            }
            Err(err) => {
                let step_dur = step_start.elapsed();
                eprintln!(
                    "{}",
                    crate::report::format_step_log(
                        index,
                        steps.len(),
                        action,
                        &target,
                        step_dur,
                        false
                    )
                );
                let duration = start_time.elapsed();

                return TestCaseResult {
                    test_name: test_name.to_string(),
                    success: false,
                    duration,
                    failure: Some(StepFailure {
                        step_index: index,
                        action_type: action.to_string(),
                        error_message: format!("{:#}", err),
                        screenshot_path: None,
                    }),
                };
            }
        }
    }

    let duration = start_time.elapsed();
    println!("{}", crate::report::format_test_passed(duration));

    TestCaseResult {
        test_name: test_name.to_string(),
        success: true,
        duration,
        failure: None,
    }
}

/// Executes an individual declarative step.
async fn execute_step(page: &Page, base_url: &str, step: &TestStep) -> Result<()> {
    match step {
        TestStep::Navigate { path } => {
            let target_url = resolve_url(base_url, path)?;
            debug!(url = %target_url, "Navigating to URL");
            // page.goto initiates and awaits navigation response without race conditions
            page.goto(target_url.as_str())
                .await
                .with_context(|| format!("Failed to navigate to '{}'", target_url))?;
        }

        TestStep::WaitForSelector {
            selector,
            timeout_ms,
        } => {
            let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_SELECTOR_TIMEOUT_MS));
            wait_for_element_visible(page, selector, timeout).await?;
        }

        TestStep::AssertText { selector, contains } => {
            // Evaluates JavaScript in the browser context to inspect innerText / textContent.
            // Using direct JS evaluation avoids serialization quirks and works reliably
            // across complex Single Page Application (SPA) virtual DOM rendering trees.
            let js = format!(
                r#"(() => {{
                    const el = document.querySelector({});
                    if (!el) return {{ found: false, text: "" }};
                    return {{ found: true, text: el.innerText || el.textContent || "" }};
                }})()"#,
                serde_json::to_string(selector)?
            );

            let result_val: Value = page
                .evaluate(js)
                .await
                .with_context(|| format!("Failed to query element for text: '{}'", selector))?
                .into_value()
                .context("Failed to deserialize evaluate result")?;

            let found = result_val
                .get("found")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if !found {
                bail!("Element '{}' was not found in the DOM", selector);
            }

            let text = result_val
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Check if expected substring is present in the extracted text
            if !text.contains(contains.as_str()) {
                bail!(
                    "Text mismatch on '{}'. Expected to contain '{}', found '{}'",
                    selector,
                    contains,
                    text
                );
            }
        }

        TestStep::AssertVisible { selector } => {
            // Confirms both DOM existence and visible rendered dimensions
            let is_visible = check_element_visible(page, selector).await?;
            if !is_visible {
                bail!("Element '{}' is not visible on page", selector);
            }
        }

        TestStep::Click { selector } => {
            // Wait for element to be visible
            wait_for_element_visible(
                page,
                selector,
                Duration::from_millis(DEFAULT_SELECTOR_TIMEOUT_MS),
            )
            .await?;

            // Scroll element into view before clicking to prevent occlusion or offscreen click failures
            let scroll_js = format!(
                r#"(() => {{
                    const el = document.querySelector({});
                    if (el) el.scrollIntoView({{ block: 'center', inline: 'center' }});
                }})()"#,
                serde_json::to_string(selector)?
            );
            let _ = page.evaluate(scroll_js).await;

            let element = page
                .find_element(selector)
                .await
                .with_context(|| format!("Failed to find element to click: '{}'", selector))?;

            element
                .click()
                .await
                .with_context(|| format!("Failed to click element: '{}'", selector))?;
        }

        TestStep::TypeText { selector, text } => {
            wait_for_element_visible(
                page,
                selector,
                Duration::from_millis(DEFAULT_SELECTOR_TIMEOUT_MS),
            )
            .await?;

            // Scroll into view, focus, and clear any existing input value before typing
            let prepare_js = format!(
                r#"(() => {{
                    const el = document.querySelector({});
                    if (el) {{
                        el.scrollIntoView({{ block: 'center', inline: 'center' }});
                        el.focus();
                        if ('value' in el) el.value = '';
                    }}
                }})()"#,
                serde_json::to_string(selector)?
            );
            let _ = page.evaluate(prepare_js).await;

            let element = page
                .find_element(selector)
                .await
                .with_context(|| format!("Failed to find input element: '{}'", selector))?;

            element
                .click()
                .await
                .with_context(|| format!("Failed to focus element '{}' for typing", selector))?;

            element
                .type_str(text)
                .await
                .with_context(|| format!("Failed to type text into '{}'", selector))?;
        }
    }

    Ok(())
}

/// Helper function to resolve a relative path against a base URL, or return the absolute URL.
pub fn resolve_url(base_url: &str, path: &str) -> Result<Url> {
    let lower_path = path.to_ascii_lowercase();
    if lower_path.starts_with("http://") || lower_path.starts_with("https://") {
        Url::parse(path).with_context(|| format!("Invalid absolute URL: '{}'", path))
    } else {
        let base =
            Url::parse(base_url).with_context(|| format!("Invalid base URL: '{}'", base_url))?;
        base.join(path).with_context(|| {
            format!(
                "Failed to join path '{}' with base URL '{}'",
                path, base_url
            )
        })
    }
}

/// Checks if an element exists and has visible non-zero dimensions.
async fn check_element_visible(page: &Page, selector: &str) -> Result<bool> {
    let js = format!(
        r#"(() => {{
            const el = document.querySelector({});
            if (!el) return false;
            const rect = el.getBoundingClientRect();
            const style = window.getComputedStyle(el);
            return style.display !== 'none' &&
                   style.visibility !== 'hidden' &&
                   style.opacity !== '0' &&
                   rect.width > 0 &&
                   rect.height > 0;
        }})()"#,
        serde_json::to_string(selector)?
    );

    let val: Value = page
        .evaluate(js)
        .await
        .with_context(|| format!("Failed to evaluate visibility for selector '{}'", selector))?
        .into_value()
        .context("Failed to parse evaluate output")?;

    Ok(val.as_bool().unwrap_or(false))
}

/// Polls the DOM until the selector is present and visible, or times out.
async fn wait_for_element_visible(page: &Page, selector: &str, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    let poll_interval = Duration::from_millis(100);

    while start.elapsed() < timeout {
        if let Ok(true) = check_element_visible(page, selector).await {
            return Ok(());
        }
        tokio::time::sleep(poll_interval).await;
    }

    bail!(
        "Timed out after {:?} waiting for selector '{}' to become visible",
        timeout,
        selector
    );
}
