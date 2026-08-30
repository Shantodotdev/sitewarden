//! Comprehensive unit tests for configuration parsing, validation, cron checking,
//! CSS selector validation, uniqueness constraints, bounds checking, and strict serde typing.

use sitewarden::config::{
    validate_css_selector, AppConfig, DEFAULT_BROWSER_CONCURRENCY, DEFAULT_SCREENSHOT_DIR,
    DEFAULT_TIMEOUT_SECONDS,
};

#[test]
fn test_valid_minimal_config() {
    let yaml = r#"
schedule: "0 0 6 * * *"
suites:
  - name: "Health Check"
    base_url: "https://example.com"
    tests:
      - name: "Homepage Load"
        steps:
          - action: navigate
            path: "/"
          - action: assert_visible
            selector: "body"
"#;

    let config = AppConfig::from_yaml_str(yaml).expect("Failed to parse valid YAML");
    assert_eq!(config.schedule, "0 0 6 * * *");
    assert_eq!(config.browser_concurrency, DEFAULT_BROWSER_CONCURRENCY);
    assert_eq!(config.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
    assert_eq!(config.screenshot_dir, DEFAULT_SCREENSHOT_DIR);
    assert_eq!(config.suites.len(), 1);
    assert_eq!(config.total_tests(), 1);
    assert_eq!(config.total_steps(), 2);

    let suite = &config.suites[0];
    assert_eq!(suite.name, "Health Check");
    assert_eq!(suite.base_url, "https://example.com");
    assert_eq!(suite.tests.len(), 1);
    assert_eq!(suite.total_steps(), 2);

    let test = &suite.tests[0];
    assert_eq!(test.name, "Homepage Load");
    assert_eq!(test.steps.len(), 2);
}

#[test]
fn test_default_trait_app_config() {
    let default_config = AppConfig::default();
    assert_eq!(default_config.schedule, "0 0 6 * * *");
    assert_eq!(
        default_config.browser_concurrency,
        DEFAULT_BROWSER_CONCURRENCY
    );
    assert_eq!(default_config.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
    assert_eq!(default_config.screenshot_dir, DEFAULT_SCREENSHOT_DIR);
    assert!(default_config.suites.is_empty());
    assert_eq!(default_config.total_tests(), 0);
    assert_eq!(default_config.total_steps(), 0);
}

#[test]
fn test_deny_unknown_fields_in_root_config() {
    let yaml_with_typo = r#"
schedule: "0 0 6 * * *"
concurrency: 4 # Typo! Should be browser_concurrency
suites:
  - name: "S1"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
"#;
    let err =
        AppConfig::from_yaml_str(yaml_with_typo).expect_err("Expected unknown field rejection");
    assert!(err.to_string().contains("unknown field `concurrency`"));
}

#[test]
fn test_deny_unknown_fields_in_test_step() {
    let yaml_with_step_typo = r##"
schedule: "0 0 6 * * *"
suites:
  - name: "S1"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: click
            target: "#btn"
"##;
    let err = AppConfig::from_yaml_str(yaml_with_step_typo)
        .expect_err("Expected unknown field rejection in step");
    assert!(err.to_string().contains("unknown field `target`"));
}

#[test]
fn test_valid_5_field_and_6_field_cron_expressions() {
    // 5-field cron
    let yaml_5 = r#"
schedule: "*/15 * * * *"
suites:
  - name: "S1"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
"#;
    assert!(AppConfig::from_yaml_str(yaml_5).is_ok());

    // 6-field cron
    let yaml_6 = r#"
schedule: "0 30 6 * * *"
suites:
  - name: "S1"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
"#;
    assert!(AppConfig::from_yaml_str(yaml_6).is_ok());
}

#[test]
fn test_invalid_cron_expressions() {
    let bad_cron_cases = [
        "not a cron at all",
        "60 * * * * *",      // invalid seconds
        "* * * * * * * * *", // too many fields
        "abc def 1 2 3",
    ];

    for bad_cron in bad_cron_cases {
        let yaml = format!(
            r#"
schedule: "{}"
suites:
  - name: "S1"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
"#,
            bad_cron
        );
        assert!(
            AppConfig::from_yaml_str(&yaml).is_err(),
            "Expected bad cron '{}' to fail validation",
            bad_cron
        );
    }
}

#[test]
fn test_duplicate_suite_names_rejected() {
    let yaml = r#"
schedule: "0 0 6 * * *"
suites:
  - name: "Duplicate Suite"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
  - name: "Duplicate Suite"
    base_url: "https://other.com"
    tests:
      - name: "T2"
        steps:
          - action: navigate
            path: "/"
"#;
    let err = AppConfig::from_yaml_str(yaml).expect_err("Expected duplicate suite name error");
    assert!(err.to_string().contains("Duplicate suite name"));
}

#[test]
fn test_duplicate_test_names_in_same_suite_rejected() {
    let yaml = r#"
schedule: "0 0 6 * * *"
suites:
  - name: "Suite 1"
    base_url: "https://example.com"
    tests:
      - name: "Same Test Name"
        steps:
          - action: navigate
            path: "/"
      - name: "Same Test Name"
        steps:
          - action: navigate
            path: "/about"
"#;
    let err = AppConfig::from_yaml_str(yaml).expect_err("Expected duplicate test name error");
    assert!(err.to_string().contains("Duplicate test name"));
}

#[test]
fn test_unsupported_url_schemes() {
    let bad_schemes = [
        "ftp://example.com",
        "file:///etc/passwd",
        "ws://example.com",
    ];
    for scheme in bad_schemes {
        let yaml = format!(
            r#"
schedule: "0 0 6 * * *"
suites:
  - name: "S1"
    base_url: "{}"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
"#,
            scheme
        );
        assert!(AppConfig::from_yaml_str(&yaml).is_err());
    }
}

#[test]
fn test_unsafe_navigation_paths_case_insensitive() {
    let unsafe_paths = [
        "javascript:alert(1)",
        "JavaScript:void(0)",
        "JAVASCRIPT:prompt()",
        "data:text/html,<h1>hacked</h1>",
        "DATA:text/plain;base64,AAA",
        "file:///etc/hosts",
        "FILE:///C:/Windows",
    ];

    for bad_path in unsafe_paths {
        let yaml = format!(
            r#"
schedule: "0 0 6 * * *"
suites:
  - name: "S1"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "{}"
"#,
            bad_path
        );
        assert!(
            AppConfig::from_yaml_str(&yaml).is_err(),
            "Expected unsafe path '{}' to be rejected",
            bad_path
        );
    }
}

#[test]
fn test_concurrency_bounds() {
    // 0 is invalid
    let yaml_zero = r#"
schedule: "0 0 6 * * *"
browser_concurrency: 0
suites:
  - name: "S1"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
"#;
    assert!(AppConfig::from_yaml_str(yaml_zero).is_err());

    // > 64 is invalid
    let yaml_too_large = r#"
schedule: "0 0 6 * * *"
browser_concurrency: 128
suites:
  - name: "S1"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
"#;
    assert!(AppConfig::from_yaml_str(yaml_too_large).is_err());
}

#[test]
fn test_timeout_bounds() {
    // > 3600s is invalid
    let yaml_too_long = r#"
schedule: "0 0 6 * * *"
timeout_seconds: 7200
suites:
  - name: "S1"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
"#;
    assert!(AppConfig::from_yaml_str(yaml_too_long).is_err());
}

#[test]
fn test_css_selector_validator_unit() {
    // Valid selectors according to W3C CSS specification
    assert!(validate_css_selector("h1").is_ok());
    assert!(validate_css_selector(".card > button.btn-primary").is_ok());
    assert!(validate_css_selector("input[name='email']").is_ok());
    assert!(validate_css_selector("input[data-val=\"test\"]").is_ok());
    assert!(validate_css_selector("div.item:nth-child(2n+1)").is_ok());

    // Invalid selectors rejected by scraper/Servo parser
    assert!(validate_css_selector("").is_err());
    assert!(validate_css_selector("   ").is_err());
    assert!(validate_css_selector("button,").is_err()); // trailing comma
    assert!(validate_css_selector("div >").is_err()); // trailing combinator
    assert!(validate_css_selector("..bad-class").is_err()); // invalid double dot
    assert!(validate_css_selector(":nth-child(not-a-number)").is_err()); // invalid pseudo formula
    assert!(validate_css_selector(":not()").is_err()); // empty pseudo negation
}
