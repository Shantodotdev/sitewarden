//! Pure-Rust static HTTP execution engine.
//!
//! Evaluates static smoke tests (HTTP status, HTML body, W3C CSS selectors, and text assertions)
//! entirely in-memory using `reqwest` and `scraper` with zero browser process overhead (~2MB RAM, ~5ms latency).

use crate::config::TestStep;
use crate::engine::{resolve_url, StepFailure, TestCaseResult};
use anyhow::{bail, Context, Result};
use reqwest::Client;
use scraper::{Html, Selector};
use std::time::Instant;
use tracing::debug;

/// Default user agent header for pure-Rust static engine requests.
pub const STATIC_ENGINE_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 SiteWarden/0.1.0";

/// Executes static test steps against target URL using pure-Rust HTTP client.
pub async fn execute_static_test_case(
    client: &Client,
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

    let mut current_html: Option<String> = None;

    for (index, step) in steps.iter().enumerate() {
        let step_start = Instant::now();
        let action = crate::report::step_action_name(step);
        let target = crate::report::step_target_desc(step);

        match execute_static_step(client, base_url, step, &mut current_html).await {
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

/// Executes an individual static test step.
async fn execute_static_step(
    client: &Client,
    base_url: &str,
    step: &TestStep,
    current_html: &mut Option<String>,
) -> Result<()> {
    match step {
        TestStep::Navigate { path } => {
            let target_url = resolve_url(base_url, path)?;
            debug!(url = %target_url, "Sending static HTTP GET request");

            let response = client
                .get(target_url.as_str())
                .header(reqwest::header::USER_AGENT, STATIC_ENGINE_USER_AGENT)
                .send()
                .await
                .with_context(|| format!("Failed to send HTTP GET to '{}'", target_url))?;

            let status = response.status();
            if !status.is_success() && !status.is_redirection() {
                bail!(
                    "HTTP request to '{}' returned non-success status: {}",
                    target_url,
                    status
                );
            }

            let body_text = response.text().await.with_context(|| {
                format!("Failed to read HTTP response body from '{}'", target_url)
            })?;

            *current_html = Some(body_text);
        }

        TestStep::AssertText { selector, contains } => {
            let html_str = current_html
                .as_ref()
                .context("Cannot assert text before navigating to a page (no active DOM)")?;

            let dom = Html::parse_document(html_str);

            let parsed_selector = Selector::parse(selector)
                .map_err(|e| anyhow::anyhow!("Invalid CSS selector '{}': {:?}", selector, e))?;

            let mut matched_elements = dom.select(&parsed_selector);
            let first_match = match matched_elements.next() {
                Some(el) => el,
                None => bail!("Element '{}' was not found in static DOM", selector),
            };

            let extracted_text: String = first_match.text().collect();
            if !extracted_text.contains(contains.as_str()) {
                bail!(
                    "Text mismatch on '{}'. Expected to contain '{}', found '{}'",
                    selector,
                    contains,
                    extracted_text.trim()
                );
            }
        }

        TestStep::AssertVisible { selector } => {
            let html_str = current_html
                .as_ref()
                .context("Cannot assert element before navigating to a page (no active DOM)")?;

            let dom = Html::parse_document(html_str);

            let parsed_selector = Selector::parse(selector)
                .map_err(|e| anyhow::anyhow!("Invalid CSS selector '{}': {:?}", selector, e))?;

            let found = dom.select(&parsed_selector).next().is_some();
            if !found {
                bail!("Element '{}' was not found in static DOM", selector);
            }
        }

        TestStep::WaitForSelector { selector, .. } => {
            bail!(
                "Dynamic step 'wait_for_selector' ({}) cannot be executed in static HTTP mode",
                selector
            );
        }

        TestStep::Click { selector } => {
            bail!(
                "Interactive step 'click' ({}) cannot be executed in static HTTP mode",
                selector
            );
        }

        TestStep::TypeText { selector, .. } => {
            bail!(
                "Interactive step 'type_text' ({}) cannot be executed in static HTTP mode",
                selector
            );
        }
    }

    Ok(())
}
