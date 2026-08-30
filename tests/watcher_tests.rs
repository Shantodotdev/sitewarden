//! Integration tests for ConfigWatcher atomic updates and error recovery.

use arc_swap::ArcSwap;
use sitewarden::config::AppConfig;
use sitewarden::watcher::ConfigWatcher;
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test_watcher_hot_reload_success_and_error_recovery() {
    let initial_yaml = r#"
schedule: "0 0 6 * * *"
suites:
  - name: "Initial Suite"
    base_url: "https://example.com"
    tests:
      - name: "T1"
        steps:
          - action: navigate
            path: "/"
"#;

    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path().to_path_buf();
    std::fs::write(&temp_path, initial_yaml).expect("Failed to write initial YAML");

    let initial_config = AppConfig::from_file(&temp_path).expect("Failed to load initial config");
    let shared_config = Arc::new(ArcSwap::from_pointee(initial_config));

    let (_watcher, mut reload_rx) = ConfigWatcher::new(&temp_path, Arc::clone(&shared_config))
        .expect("Failed to create watcher");

    assert_eq!(shared_config.load().suites[0].name, "Initial Suite");

    // Write updated valid YAML
    let updated_yaml = r#"
schedule: "0 0 12 * * *"
suites:
  - name: "Updated Suite"
    base_url: "https://example.com"
    tests:
      - name: "T2"
        steps:
          - action: navigate
            path: "/dashboard"
"#;

    std::fs::write(&temp_path, updated_yaml).expect("Failed to write updated YAML");

    // Wait for reload signal with timeout
    let reloaded = tokio::time::timeout(Duration::from_secs(3), reload_rx.recv()).await;
    assert!(reloaded.is_ok(), "Expected reload signal within timeout");

    // Verify atomic state swapped
    assert_eq!(shared_config.load().schedule, "0 0 12 * * *");
    assert_eq!(shared_config.load().suites[0].name, "Updated Suite");

    // Now write corrupted YAML - should NOT crash and should RETAIN previous valid config
    let broken_yaml = "invalid: [yaml: broken";
    std::fs::write(&temp_path, broken_yaml).expect("Failed to write broken YAML");

    // Sleep past debounce window
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Previous valid configuration is strictly preserved
    assert_eq!(shared_config.load().schedule, "0 0 12 * * *");
    assert_eq!(shared_config.load().suites[0].name, "Updated Suite");
}
