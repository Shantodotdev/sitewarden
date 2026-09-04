//! Visual execution tree reporting, ASCII scorecards, and diagnostic formatting for SiteWarden.
//!
//! # Visual Reporting Engine Design
//! SiteWarden uses a custom-crafted terminal reporting engine that balances human aesthetics with
//! automated log-parser compatibility:
//! - **Unicode-Width Padding:** Accurately computes true terminal column widths for 2-column emojis (`🌐`, `⏳`, `🔍`, `👁️`, `🖱️`, `✍️`, `✅`, `❌`), ensuring arrows and badges align vertically.
//! - **Semantic Color Palette:** Actions are color-coded (Cyan for navigation, Yellow for waiting, Magenta for assertions, Blue for visibility).
//! - **Hierarchical Execution Tree:** Tests and steps are nested using box-drawing characters (`├──`, `└──`) for clean visual scanning.
//! - **Symmetric Scorecard Tables:** ASCII summary tables close with exact column widths across dynamic test suite names and durations.

use crate::config::TestStep;
use std::time::Duration;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

// ANSI terminal styling escape constants
pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const MAGENTA: &str = "\x1b[35m";
pub const CYAN: &str = "\x1b[36m";
pub const BOLD_RED: &str = "\x1b[1;31m";
pub const BOLD_GREEN: &str = "\x1b[1;32m";
pub const BOLD_YELLOW: &str = "\x1b[1;33m";
pub const BOLD_CYAN: &str = "\x1b[1;36m";
pub const BOLD_WHITE: &str = "\x1b[1;37m";
pub const GRAY: &str = "\x1b[90m";

/// Returns the action name for a given test step.
pub fn step_action_name(step: &TestStep) -> &'static str {
    match step {
        TestStep::Navigate { .. } => "navigate",
        TestStep::WaitForSelector { .. } => "wait_for_selector",
        TestStep::AssertText { .. } => "assert_text",
        TestStep::AssertVisible { .. } => "assert_visible",
        TestStep::Click { .. } => "click",
        TestStep::TypeText { .. } => "type_text",
    }
}

/// Returns a human-friendly target description for a given test step.
pub fn step_target_desc(step: &TestStep) -> String {
    match step {
        TestStep::Navigate { path } => path.clone(),
        TestStep::WaitForSelector { selector, .. } => selector.clone(),
        TestStep::AssertText { selector, contains } => {
            format!("'{}' contains '{}'", selector, contains)
        }
        TestStep::AssertVisible { selector } => selector.clone(),
        TestStep::Click { selector } => selector.clone(),
        TestStep::TypeText { selector, text } => format!("'{}' = '{}'", selector, text),
    }
}

/// Returns a thematic ANSI color for an individual test action.
pub fn action_color(action: &str) -> &'static str {
    match action {
        "navigate" => CYAN,
        "wait_for_selector" => YELLOW,
        "assert_text" => MAGENTA,
        "assert_visible" => BLUE,
        "click" => GREEN,
        "type_text" => MAGENTA,
        _ => WHITE_COLOR,
    }
}
const WHITE_COLOR: &str = "\x1b[37m";

/// Individual test execution summary for reporting.
#[derive(Debug, Clone)]
pub struct SuiteExecutionSummary {
    pub name: String,
    pub engine_type: &'static str,
    pub passed: bool,
    pub total_tests: usize,
    pub passed_tests: usize,
    pub total_steps: usize,
    pub duration: Duration,
}

/// Formats a step log entry with strict vertical alignment and vibrant ANSI terminal colors.
pub fn format_step_log(
    step_idx: usize,
    total_steps: usize,
    action: &str,
    target: &str,
    duration: Duration,
    success: bool,
) -> String {
    let icon = match action {
        "navigate" => "🌐",
        "assert_text" => "🔍",
        "assert_visible" => "👁️",
        "wait_for_selector" => "⏳",
        "click" => "🖱️",
        "type_text" => "✍️",
        _ => "▶",
    };

    let step_prefix = format!("[{}/{}]", step_idx + 1, total_steps);
    let action_with_icon = format!("{} {}", icon, action);
    let padded_action = pad_to_width(&action_with_icon, 20);
    let padded_target = pad_to_width(&truncate_to_width(target, 36), 36);

    let status = if success { "OK" } else { "FAILED" };
    let time_str = format_duration(duration);
    let color = action_color(action);

    let status_badge = if success {
        format!("{}{}[{} • {}]{}", BOLD, GREEN, status, time_str, RESET)
    } else {
        format!("{}{}[{} • {}]{}", BOLD, RED, status, time_str, RESET)
    };

    let tree_branch = format!("{}├── {}{:<7}{}", GRAY, RESET, step_prefix, GRAY);
    let action_str = format!("{}{}{}{}", color, BOLD, padded_action, RESET);
    let arrow_str = format!("{}→{}", GRAY, RESET);
    let target_str = format!("{}{}{}", BOLD_WHITE, padded_target, RESET);

    format!(
        "    {} {} {} {} {}",
        tree_branch, action_str, arrow_str, target_str, status_badge
    )
}

/// Formats the test completion line.
pub fn format_test_passed(duration: Duration) -> String {
    format!(
        "    └── {}✅ Test Passed ({}){}",
        BOLD_GREEN,
        format_duration(duration),
        RESET
    )
}

/// Generates a high-visibility diagnostic box when a test step fails.
pub fn format_failure_card(
    suite_name: &str,
    test_name: &str,
    step_idx: usize,
    action_type: &str,
    error_message: &str,
    screenshot_path: Option<&str>,
) -> String {
    let mut card = String::new();
    card.push_str(&format!(
        "\n{}┌─────────────────────────────── STEP FAILURE ───────────────────────────────┐{}\n",
        BOLD_RED, RESET
    ));
    card.push_str(&format!(
        "{}│{} Suite:       {}{:<62}{} {}│{}\n",
        BOLD_RED,
        RESET,
        BOLD_WHITE,
        truncate_to_width(suite_name, 62),
        RESET,
        BOLD_RED,
        RESET
    ));
    card.push_str(&format!(
        "{}│{} Test:        {}{:<62}{} {}│{}\n",
        BOLD_RED,
        RESET,
        BOLD_WHITE,
        truncate_to_width(test_name, 62),
        RESET,
        BOLD_RED,
        RESET
    ));
    card.push_str(&format!(
        "{}│{} Step #{:<5} {}{:<62}{} {}│{}\n",
        BOLD_RED,
        RESET,
        step_idx + 1,
        YELLOW,
        truncate_to_width(&format!("Action: {}", action_type), 62),
        RESET,
        BOLD_RED,
        RESET
    ));
    card.push_str(&format!(
        "{}│                                                                             │{}\n",
        BOLD_RED, RESET
    ));

    // Cleanly wrap error message lines
    for line in error_message.lines().take(4) {
        card.push_str(&format!(
            "{}│{} Error:       {}{:<62}{} {}│{}\n",
            BOLD_RED,
            RESET,
            RED,
            truncate_to_width(line, 62),
            RESET,
            BOLD_RED,
            RESET
        ));
    }

    if let Some(path) = screenshot_path {
        card.push_str(&format!(
            "{}│{} Artifact:    {}{:<62}{} {}│{}\n",
            BOLD_RED,
            RESET,
            CYAN,
            truncate_to_width(path, 62),
            RESET,
            BOLD_RED,
            RESET
        ));
    }

    card.push_str(&format!(
        "{}└─────────────────────────────────────────────────────────────────────────────┘{}\n",
        BOLD_RED, RESET
    ));
    card
}

/// Generates an ASCII scorecard table with 100% accurate column alignment and colors.
pub fn format_summary_table(
    summaries: &[SuiteExecutionSummary],
    total_cycle_duration: Duration,
) -> String {
    let total_suites = summaries.len();
    let passed_suites = summaries.iter().filter(|s| s.passed).count();
    let failed_suites = total_suites - passed_suites;
    let all_passed = failed_suites == 0;

    let mut table = String::new();
    table.push('\n');
    table.push_str(&format!(
        "{}┌──────────────────────────────────────────────────────────────────────────────────────────┐{}\n",
        GRAY, RESET
    ));
    table.push_str(&format!(
        "{}│                               {}SiteWarden Test Cycle Report{}                               {}│{}\n",
        GRAY, BOLD_WHITE, RESET, GRAY, RESET
    ));
    table.push_str(&format!(
        "{}├──────────────────────────────────────┬──────────┬────────────┬──────────┬────────────────┤{}\n",
        GRAY, RESET
    ));
    table.push_str(&format!(
        "{}│{} Suite Name                           {}│{} Engine   {}│{} Result     {}│{} Tests    {}│{} Duration       {}│{}\n",
        GRAY, BOLD, GRAY, BOLD, GRAY, BOLD, GRAY, BOLD, GRAY, BOLD, GRAY, RESET
    ));
    table.push_str(&format!(
        "{}├──────────────────────────────────────┼──────────┼────────────┼──────────┼────────────────┤{}\n",
        GRAY, RESET
    ));

    for s in summaries {
        let result_colored = if s.passed {
            format!("{}{}{}", BOLD_GREEN, pad_to_width("✅ PASS", 10), RESET)
        } else {
            format!("{}{}{}", BOLD_RED, pad_to_width("❌ FAIL", 10), RESET)
        };

        let tests_str = format!("{}/{} ({})", s.passed_tests, s.total_tests, s.total_steps);
        let duration_str = format_duration(s.duration);
        let engine_colored = if s.engine_type == "Static" {
            format!("{}{}{}", CYAN, pad_to_width(s.engine_type, 8), RESET)
        } else {
            format!("{}{}{}", YELLOW, pad_to_width(s.engine_type, 8), RESET)
        };

        let col1 = pad_to_width(&truncate_to_width(&s.name, 36), 36);
        let col4 = pad_to_width(&truncate_to_width(&tests_str, 8), 8);
        let col5 = pad_to_width(&truncate_to_width(&duration_str, 14), 14);

        table.push_str(&format!(
            "{}│{} {} {}│{} {} {}│{} {} {}│{} {} {}│{} {} {}│{}\n",
            GRAY,
            RESET,
            col1,
            GRAY,
            RESET,
            engine_colored,
            GRAY,
            RESET,
            result_colored,
            GRAY,
            RESET,
            col4,
            GRAY,
            RESET,
            col5,
            GRAY,
            RESET
        ));
    }

    table.push_str(&format!(
        "{}├──────────────────────────────────────┴──────────┴────────────┴──────────┴────────────────┤{}\n",
        GRAY, RESET
    ));

    let summary_badge = if all_passed {
        format!(
            "{}✅ All {} Suites Passed (100%){}",
            BOLD_GREEN, total_suites, RESET
        )
    } else {
        format!(
            "{}❌ {} Passed, {} Failed{}",
            BOLD_RED, passed_suites, failed_suites, RESET
        )
    };

    let total_time_str = format_duration(total_cycle_duration);
    let raw_summary_line = if all_passed {
        format!(
            "✅ All {} Suites Passed (100%) • Total Cycle Time: {}",
            total_suites, total_time_str
        )
    } else {
        format!(
            "❌ {} Passed, {} Failed • Total Cycle Time: {}",
            passed_suites, failed_suites, total_time_str
        )
    };

    let pad_len = 88usize.saturating_sub(raw_summary_line.width());
    let padding = " ".repeat(pad_len);

    table.push_str(&format!(
        "{}│{} {} • Total Cycle Time: {}{}{} │{}\n",
        GRAY, RESET, summary_badge, total_time_str, padding, GRAY, RESET
    ));
    table.push_str(&format!(
        "{}└──────────────────────────────────────────────────────────────────────────────────────────┘{}\n",
        GRAY, RESET
    ));

    table
}

/// Helper to format durations cleanly (e.g. "42ms" or "1.82s").
pub fn format_duration(duration: Duration) -> String {
    let ms = duration.as_millis();
    if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

/// Pads a string with spaces so its terminal display width matches `target_width`.
pub fn pad_to_width(s: &str, target_width: usize) -> String {
    let current_width = s.width();
    if current_width >= target_width {
        s.to_string()
    } else {
        let padding = " ".repeat(target_width - current_width);
        format!("{}{}", s, padding)
    }
}

/// Truncates a string to fit within `max_width` display columns, adding `...` if truncated.
pub fn truncate_to_width(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    if max_width <= 3 {
        return "...".chars().take(max_width).collect();
    }

    let mut result = String::new();
    let mut current_width = 0;
    let limit = max_width.saturating_sub(3);

    for ch in s.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > limit {
            break;
        }
        result.push(ch);
        current_width += ch_width;
    }

    result.push_str("...");
    result
}

/// Formats the overall status dashboard for `sitewarden status`.
pub fn format_status_dashboard(
    state: &crate::state::AppState,
    config: &crate::config::AppConfig,
    update_info: Option<&crate::updater::UpdateInfo>,
    screenshot_count: usize,
    screenshot_bytes: u64,
) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!(
        "{}┌───────────────────────── SiteWarden Service Status ──────────────────────────┐{}\n",
        GRAY, RESET
    ));

    // Helper to format consistent 80-column dashboard rows
    let format_row = |label: &str, display_val: &str, raw_val: &str| -> String {
        let label_padded = pad_to_width(label, 18);
        let val_width = raw_val.width();
        let pad_len = 58usize.saturating_sub(val_width);
        let padding = " ".repeat(pad_len);
        format!(
            "{}│{} {}{}{}{} {}│{}\n",
            GRAY, RESET, label_padded, display_val, RESET, padding, GRAY, RESET
        )
    };

    // 1. Mode & Execution Posture
    let (mode_display, mode_raw, sched_label, sched_display, sched_raw) =
        if state.is_daemon_running() {
            if let Some(ref started_at) = state.daemon_started_at {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(started_at) {
                    let dur = chrono::Utc::now().signed_duration_since(dt);
                    let days = dur.num_days();
                    let hours = dur.num_hours() % 24;
                    let mins = dur.num_minutes() % 60;
                    let uptime = if days > 0 {
                        format!("{}d {}h {}m", days, hours, mins)
                    } else if hours > 0 {
                        format!("{}h {}m", hours, mins)
                    } else {
                        format!("{}m", mins)
                    };
                    let pid_tag = if let Some(pid) = state.daemon_pid {
                        format!(" (PID: {})", pid)
                    } else {
                        String::new()
                    };
                    (
                        format!("{}🟢 Active Daemon{}{}", BOLD_GREEN, pid_tag, RESET),
                        format!("🟢 Active Daemon{}", pid_tag),
                        "Uptime & Cron:".to_string(),
                        format!("Up {} (Cron: '{}')", uptime, config.schedule),
                        format!("Up {} (Cron: '{}')", uptime, config.schedule),
                    )
                } else {
                    (
                        format!("{}🟢 Active Daemon{}", BOLD_GREEN, RESET),
                        "🟢 Active Daemon".to_string(),
                        "Schedule:".to_string(),
                        format!("Cron: '{}'", config.schedule),
                        format!("Cron: '{}'", config.schedule),
                    )
                }
            } else {
                (
                    format!("{}🟢 Active Daemon{}", BOLD_GREEN, RESET),
                    "🟢 Active Daemon".to_string(),
                    "Schedule:".to_string(),
                    format!("Cron: '{}'", config.schedule),
                    format!("Cron: '{}'", config.schedule),
                )
            }
        } else {
            (
                format!("{}⚪ Standalone CLI (Daemon inactive){}", BOLD_WHITE, RESET),
                "⚪ Standalone CLI (Daemon inactive)".to_string(),
                "Config Schedule:".to_string(),
                format!("Cron: '{}' (Ready)", config.schedule),
                format!("Cron: '{}' (Ready)", config.schedule),
            )
        };

    out.push_str(&format_row("Execution Mode:", &mode_display, &mode_raw));
    out.push_str(&format_row(&sched_label, &sched_display, &sched_raw));

    // 2. Version
    let (version_str, raw_version) = if let Some(up) = update_info {
        if up.update_available {
            (
                format!(
                    "v{} ({}✨ v{} Available! Run: sitewarden update{})",
                    up.current_version, BOLD_CYAN, up.latest_version, RESET
                ),
                format!(
                    "v{} (✨ v{} Available! Run: sitewarden update)",
                    up.current_version, up.latest_version
                ),
            )
        } else {
            (
                format!("v{} ({}Latest{})", up.current_version, GREEN, RESET),
                format!("v{} (Latest)", up.current_version),
            )
        }
    } else {
        (
            format!("v{}", env!("CARGO_PKG_VERSION")),
            format!("v{}", env!("CARGO_PKG_VERSION")),
        )
    };
    out.push_str(&format_row("Version:", &version_str, &raw_version));

    out.push_str(&format!(
        "{}├──────────────────────────────────────────────────────────────────────────────┤{}\n",
        GRAY, RESET
    ));

    // 5. Statistics
    let success_rate = if state.total_cycles > 0 {
        format!(
            "{:.1}%",
            (state.total_passed_cycles as f64 / state.total_cycles as f64) * 100.0
        )
    } else {
        "100.0%".to_string()
    };

    let stats_str = format!(
        "{} runs ({} Passed, {} Failed • {} Success Rate)",
        state.total_cycles, state.total_passed_cycles, state.total_failed_cycles, success_rate
    );
    out.push_str(&format_row("Total Cycles:", &stats_str, &stats_str));

    let last_run_str = if let Some(ref last) = state.last_cycle {
        let res = if last.all_passed {
            "✅ Passed"
        } else {
            "❌ Failed"
        };
        format!("{} ({}, {}ms)", last.timestamp, res, last.duration_ms)
    } else {
        "No test cycles executed yet".to_string()
    };
    out.push_str(&format_row("Last Run:", &last_run_str, &last_run_str));

    out.push_str(&format!(
        "{}├──────────────────────────────────────────────────────────────────────────────┤{}\n",
        GRAY, RESET
    ));

    // 6. Suites & Storage
    let suites_str = format!(
        "{} configured ({} tests, {} steps)",
        config.suites.len(),
        config.total_tests(),
        config.total_steps()
    );
    out.push_str(&format_row("Monitored Suites:", &suites_str, &suites_str));

    let storage_str = format!(
        "{} artifacts ({}) in {}",
        screenshot_count,
        crate::pruner::format_bytes(screenshot_bytes),
        config.screenshot_dir
    );
    out.push_str(&format_row("Screenshots:", &storage_str, &storage_str));

    out.push_str(&format!(
        "{}└──────────────────────────────────────────────────────────────────────────────┘{}\n",
        GRAY, RESET
    ));

    out
}

/// Formats the history timeline table for `sitewarden history`.
pub fn format_history_table(records: &[crate::state::RunHistoryRecord]) -> String {
    let mut table = String::new();
    table.push('\n');
    table.push_str(&format!(
        "{}┌──────────────────────┬────────────────────────┬────────────┬──────────────┬────────────┐{}\n",
        GRAY, RESET
    ));

    let h1 = pad_to_width("Timestamp (UTC)", 20);
    let h2 = pad_to_width("Suites (Pass/Fail)", 22);
    let h3 = pad_to_width("Result", 10);
    let h4 = pad_to_width("Duration", 12);
    let h5 = pad_to_width("Trigger", 10);

    table.push_str(&format!(
        "{}│{} {} {}│{} {} {}│{} {} {}│{} {} {}│{} {} {}│{}\n",
        GRAY,
        BOLD_WHITE,
        h1,
        GRAY,
        BOLD_WHITE,
        h2,
        GRAY,
        BOLD_WHITE,
        h3,
        GRAY,
        BOLD_WHITE,
        h4,
        GRAY,
        BOLD_WHITE,
        h5,
        GRAY,
        RESET
    ));
    table.push_str(&format!(
        "{}├──────────────────────┼────────────────────────┼────────────┼──────────────┼────────────┤{}\n",
        GRAY, RESET
    ));

    if records.is_empty() {
        let empty_msg = "No test cycle history recorded yet";
        let pad = " ".repeat(86usize.saturating_sub(empty_msg.width()));
        table.push_str(&format!(
            "{}│{} {}{}{} │{}\n",
            GRAY, RESET, empty_msg, pad, GRAY, RESET
        ));
    } else {
        for r in records {
            let result_colored = if r.all_passed {
                format!("{}{}{}", BOLD_GREEN, pad_to_width("✅ PASS", 10), RESET)
            } else {
                format!("{}{}{}", BOLD_RED, pad_to_width("❌ FAIL", 10), RESET)
            };

            let suites_str = format!(
                "{}/{} ({} steps)",
                r.passed_suites, r.total_suites, r.total_steps
            );
            let dur_str = format_duration(Duration::from_millis(r.duration_ms));

            let col1 = pad_to_width(&truncate_to_width(&r.timestamp, 20), 20);
            let col2 = pad_to_width(&truncate_to_width(&suites_str, 22), 22);
            let col4 = pad_to_width(&truncate_to_width(&dur_str, 12), 12);
            let col5 = pad_to_width(&truncate_to_width(&r.trigger, 10), 10);

            table.push_str(&format!(
                "{}│{} {} {}│{} {} {}│{} {} {}│{} {} {}│{} {} {}│{}\n",
                GRAY,
                RESET,
                col1,
                GRAY,
                RESET,
                col2,
                GRAY,
                RESET,
                result_colored,
                GRAY,
                RESET,
                col4,
                GRAY,
                RESET,
                col5,
                GRAY,
                RESET
            ));
        }
    }

    table.push_str(&format!(
        "{}└──────────────────────┴────────────────────────┴────────────┴──────────────┴────────────┘{}\n",
        GRAY, RESET
    ));

    table
}

/// Formats the doctor diagnostics checklist.
pub fn format_doctor_report(checks: &[crate::doctor::DoctorCheck]) -> String {
    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!(
        "{}🩺 SiteWarden Environment Diagnostics & Pre-Flight Checks{}\n",
        BOLD_WHITE, RESET
    ));
    out.push_str(&format!("{}{}{}\n\n", GRAY, "─".repeat(70), RESET));

    let mut all_ok = true;
    for check in checks {
        let badge = if check.passed {
            format!("{}[PASS]{}", BOLD_GREEN, RESET)
        } else {
            all_ok = false;
            format!("{}[FAIL]{}", BOLD_RED, RESET)
        };

        out.push_str(&format!(
            "  {} {:<10} {} {}{}{}\n",
            badge,
            format!("[{}]", check.category),
            "→",
            BOLD_WHITE,
            check.name,
            RESET
        ));
        out.push_str(&format!(
            "             {}{}{}\n",
            GRAY, check.details, RESET
        ));
    }

    out.push('\n');
    if all_ok {
        out.push_str(&format!(
            "{}🎉 All diagnostic checks passed! System is 100% healthy and operational.{}\n",
            BOLD_GREEN, RESET
        ));
    } else {
        out.push_str(&format!(
            "{}⚠️ One or more diagnostic checks encountered issues. Review details above.{}\n",
            BOLD_RED, RESET
        ));
    }

    out
}

#[cfg(test)]
/// Helper to strip ANSI escape codes for pure visible display width calculation.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut in_escape = false;
    for ch in s.chars() {
        if ch == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if ch == 'm' {
                in_escape = false;
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_step_log_alignment() {
        let line1 = format_step_log(0, 4, "navigate", "/", Duration::from_millis(38), true);
        let line2 = format_step_log(
            1,
            4,
            "wait_for_selector",
            "h1",
            Duration::from_millis(13),
            true,
        );
        let line3 = format_step_log(
            2,
            4,
            "assert_text",
            "'h1' contains 'Example Domain'",
            Duration::from_millis(4),
            true,
        );

        let clean1 = strip_ansi(&line1);
        let clean2 = strip_ansi(&line2);
        let clean3 = strip_ansi(&line3);

        // Find position of "→"
        let arrow_pos1 = clean1.find('→').unwrap();
        let arrow_pos2 = clean2.find('→').unwrap();
        let arrow_pos3 = clean3.find('→').unwrap();

        // Ensure arrows align on the exact same terminal display column width
        assert_eq!(clean1[..arrow_pos1].width(), clean2[..arrow_pos2].width());
        assert_eq!(clean2[..arrow_pos2].width(), clean3[..arrow_pos3].width());

        // Ensure timing badges align on the exact same terminal display column width
        let badge_pos1 = clean1.rfind('[').unwrap();
        let badge_pos2 = clean2.rfind('[').unwrap();
        let badge_pos3 = clean3.rfind('[').unwrap();

        assert_eq!(clean1[..badge_pos1].width(), clean2[..badge_pos2].width());
        assert_eq!(clean2[..badge_pos2].width(), clean3[..badge_pos3].width());
    }

    #[test]
    fn test_format_summary_table_borders() {
        let summaries = vec![
            SuiteExecutionSummary {
                name: "Example Domain Health Check".to_string(),
                engine_type: "Browser",
                passed: true,
                total_tests: 1,
                passed_tests: 1,
                total_steps: 4,
                duration: Duration::from_millis(214),
            },
            SuiteExecutionSummary {
                name: "Wikipedia Navigation Test".to_string(),
                engine_type: "Browser",
                passed: true,
                total_tests: 1,
                passed_tests: 1,
                total_steps: 4,
                duration: Duration::from_millis(532),
            },
        ];

        let table = format_summary_table(&summaries, Duration::from_millis(746));
        for line in table.lines() {
            if !line.trim().is_empty() {
                let clean = strip_ansi(line);
                assert_eq!(
                    clean.width(),
                    92,
                    "Summary table line width mismatch: '{}' (width {})",
                    clean,
                    clean.width()
                );
            }
        }
    }

    #[test]
    fn test_format_status_dashboard_alignment() {
        let state = crate::state::AppState::default();
        let config = crate::config::AppConfig {
            schedule: "0 0 6 * * *".to_string(),
            browser_concurrency: 2,
            timeout_seconds: 30,
            screenshot_dir: "/app/screenshots".to_string(),
            alerts: None,
            suites: vec![],
        };

        let dashboard = format_status_dashboard(&state, &config, None, 0, 0);
        for line in dashboard.lines() {
            if !line.trim().is_empty() {
                let clean = strip_ansi(line);
                assert_eq!(
                    clean.width(),
                    80,
                    "Dashboard line width mismatch: '{}' (width {})",
                    clean,
                    clean.width()
                );
            }
        }
    }

    #[test]
    fn test_format_history_table_alignment() {
        let records = vec![crate::state::RunHistoryRecord {
            timestamp: "2026-09-01 06:04:18".to_string(),
            total_suites: 2,
            passed_suites: 2,
            failed_suites: 0,
            total_steps: 8,
            duration_ms: 1370,
            trigger: "Cycle".to_string(),
            all_passed: true,
        }];

        let table = format_history_table(&records);
        for line in table.lines() {
            if !line.trim().is_empty() {
                let clean = strip_ansi(line);
                assert_eq!(
                    clean.width(),
                    90,
                    "History table line width mismatch: '{}' (width {})",
                    clean,
                    clean.width()
                );
            }
        }
    }
}
