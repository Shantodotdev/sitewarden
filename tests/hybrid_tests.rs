use sitewarden::config::{TestCase, TestStep, TestSuite};
use sitewarden::static_engine::execute_static_test_case;
use std::net::SocketAddr;
use tokio::net::TcpListener;

/// Helper function that starts a mock HTTP server returning sample HTML for testing.
async fn spawn_mock_http_server(
    html_body: &'static str,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html_body.len(),
                html_body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });

    (addr, handle)
}

#[test]
fn test_is_static_executable_auto_detection() {
    let static_test = TestCase {
        name: "Static Test".to_string(),
        steps: vec![
            TestStep::Navigate {
                path: "/".to_string(),
            },
            TestStep::AssertText {
                selector: "h1".to_string(),
                contains: "Welcome".to_string(),
            },
            TestStep::AssertVisible {
                selector: ".header".to_string(),
            },
        ],
    };
    assert!(static_test.is_static_executable());

    let dynamic_click_test = TestCase {
        name: "Dynamic Click Test".to_string(),
        steps: vec![
            TestStep::Navigate {
                path: "/".to_string(),
            },
            TestStep::Click {
                selector: "button#submit".to_string(),
            },
        ],
    };
    assert!(!dynamic_click_test.is_static_executable());

    let dynamic_type_test = TestCase {
        name: "Dynamic Type Test".to_string(),
        steps: vec![
            TestStep::Navigate {
                path: "/".to_string(),
            },
            TestStep::TypeText {
                selector: "input#email".to_string(),
                text: "admin@example.com".to_string(),
            },
        ],
    };
    assert!(!dynamic_type_test.is_static_executable());

    let dynamic_wait_test = TestCase {
        name: "Dynamic Wait Test".to_string(),
        steps: vec![
            TestStep::Navigate {
                path: "/".to_string(),
            },
            TestStep::WaitForSelector {
                selector: ".async-loaded".to_string(),
                timeout_ms: Some(2000),
            },
        ],
    };
    assert!(!dynamic_wait_test.is_static_executable());
}

#[test]
fn test_suite_is_all_static_query() {
    let all_static_suite = TestSuite {
        name: "Static Suite".to_string(),
        base_url: "https://example.com".to_string(),
        tests: vec![TestCase {
            name: "Test 1".to_string(),
            steps: vec![
                TestStep::Navigate {
                    path: "/".to_string(),
                },
                TestStep::AssertText {
                    selector: "h1".to_string(),
                    contains: "Hello".to_string(),
                },
            ],
        }],
    };
    assert!(all_static_suite.is_all_static());

    let mixed_suite = TestSuite {
        name: "Mixed Suite".to_string(),
        base_url: "https://example.com".to_string(),
        tests: vec![
            TestCase {
                name: "Static Test".to_string(),
                steps: vec![TestStep::Navigate {
                    path: "/".to_string(),
                }],
            },
            TestCase {
                name: "Dynamic Test".to_string(),
                steps: vec![TestStep::Click {
                    selector: "button".to_string(),
                }],
            },
        ],
    };
    assert!(!mixed_suite.is_all_static());
}

#[tokio::test]
async fn test_static_engine_execute_success() {
    let html = r#"
        <!DOCTYPE html>
        <html>
            <head><title>SiteWarden Test</title></head>
            <body>
                <header class="navbar">
                    <h1 id="title">SiteWarden Pure-Rust Static Engine</h1>
                </header>
                <div class="content">
                    <p>Ultra-lightweight monitoring active.</p>
                </div>
            </body>
        </html>
    "#;

    let (addr, server_handle) = spawn_mock_http_server(html).await;
    let base_url = format!("http://{}", addr);

    let test_case = TestCase {
        name: "Static Engine Verification".to_string(),
        steps: vec![
            TestStep::Navigate {
                path: "/".to_string(),
            },
            TestStep::AssertVisible {
                selector: ".navbar".to_string(),
            },
            TestStep::AssertText {
                selector: "#title".to_string(),
                contains: "Pure-Rust Static Engine".to_string(),
            },
            TestStep::AssertText {
                selector: ".content p".to_string(),
                contains: "Ultra-lightweight".to_string(),
            },
        ],
    };

    let client = reqwest::Client::builder().build().unwrap();
    let result =
        execute_static_test_case(&client, &base_url, &test_case.name, &test_case.steps).await;

    server_handle.abort();

    assert!(
        result.success,
        "Static test case should succeed: {:?}",
        result.failure
    );
    assert_eq!(result.test_name, "Static Engine Verification");
    assert!(result.failure.is_none());
}

#[tokio::test]
async fn test_static_engine_text_mismatch_failure() {
    let html = r#"
        <html><body><h1>Server Error 500</h1></body></html>
    "#;

    let (addr, server_handle) = spawn_mock_http_server(html).await;
    let base_url = format!("http://{}", addr);

    let test_case = TestCase {
        name: "Failing Assertion Test".to_string(),
        steps: vec![
            TestStep::Navigate {
                path: "/".to_string(),
            },
            TestStep::AssertText {
                selector: "h1".to_string(),
                contains: "Welcome Home".to_string(),
            },
        ],
    };

    let client = reqwest::Client::builder().build().unwrap();
    let result =
        execute_static_test_case(&client, &base_url, &test_case.name, &test_case.steps).await;

    server_handle.abort();

    assert!(!result.success);
    let failure = result.failure.expect("Expected StepFailure");
    assert_eq!(failure.step_index, 1);
    assert_eq!(failure.action_type, "assert_text");
    assert!(failure
        .error_message
        .contains("Expected to contain 'Welcome Home'"));
}

#[tokio::test]
async fn test_static_engine_missing_selector_failure() {
    let html = r#"
        <html><body><p>Hello world</p></body></html>
    "#;

    let (addr, server_handle) = spawn_mock_http_server(html).await;
    let base_url = format!("http://{}", addr);

    let test_case = TestCase {
        name: "Missing Element Test".to_string(),
        steps: vec![
            TestStep::Navigate {
                path: "/".to_string(),
            },
            TestStep::AssertVisible {
                selector: "div.non-existent".to_string(),
            },
        ],
    };

    let client = reqwest::Client::builder().build().unwrap();
    let result =
        execute_static_test_case(&client, &base_url, &test_case.name, &test_case.steps).await;

    server_handle.abort();

    assert!(!result.success);
    let failure = result.failure.expect("Expected StepFailure");
    assert_eq!(failure.step_index, 1);
    assert_eq!(failure.action_type, "assert_visible");
    assert!(failure
        .error_message
        .contains("was not found in static DOM"));
}
