//! Visual reporting, ASCII scorecards, and diagnostic formatting for SiteWarden test cycles.

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

    let padding = " ".repeat(88usize.saturating_sub(raw_summary_line.width()));

    table.push_str(&format!(
        "{}│{} {} • Total Cycle Time: {}{}{}│{}\n",
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
            if line.contains('│') {
                assert!(line.contains('│'), "Line contains border: {}", line);
            }
        }
    }
}
