//! Unit tests for engine URL resolution.

use sitewarden::engine::resolve_url;

#[test]
fn test_resolve_relative_url() {
    let base = "https://example.com/app/";
    let path = "dashboard";
    let resolved = resolve_url(base, path).expect("Failed to resolve URL");
    assert_eq!(resolved.as_str(), "https://example.com/app/dashboard");

    let root_path = "/login";
    let resolved_root = resolve_url(base, root_path).expect("Failed to resolve URL");
    assert_eq!(resolved_root.as_str(), "https://example.com/login");
}

#[test]
fn test_resolve_absolute_url() {
    let base = "https://example.com";
    let path = "https://other-domain.com/status";
    let resolved = resolve_url(base, path).expect("Failed to resolve absolute URL");
    assert_eq!(resolved.as_str(), "https://other-domain.com/status");
}

#[test]
fn test_resolve_invalid_urls() {
    assert!(resolve_url("invalid-base", "path").is_err());
}
