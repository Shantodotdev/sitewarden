# Software Requirements Specification (SRS)

**Project:** SiteWarden  
**Standard:** IEEE Std 830-1998 Compliant  
**Author:** SiteWarden Core Team  
**Status:** Approved for Implementation  
**Version:** 1.0.0  

---

## 1. Introduction

### 1.1 Purpose
This Software Requirements Specification (SRS) establishes the complete functional, performance, operational, and interface requirements for **SiteWarden**, an autonomous, scheduled browser smoke testing daemon written in Rust.

### 1.2 Scope of Software
SiteWarden is a background daemon designed for deployment on cloud virtual private servers (VPS). It provides:
1. Declarative browser-level testing of web applications.
2. Scheduled, recurring test execution via an integrated cron engine.
3. Zero-downtime hot-reloading of configuration files.
4. Failure diagnostics via automatic screenshot generation and webhook dispatch.
5. Containerized deployment with automated continuous delivery.

### 1.3 Definitions, Acronyms, and Abbreviations
* **CDP:** Chrome DevTools Protocol (WebSocket-based control protocol for Chromium).
* **DOM:** Document Object Model.
* **SPA:** Single Page Application (client-side rendered via React, Vue, Svelte, etc.).
* **NFR:** Non-Functional Requirement.
* **Hot-Reload:** Updating application state in memory without process restart or connection dropping.
* **ArcSwap:** Atomic pointer swap pattern providing wait-free read access to shared data structures.

---

## 2. Overall Description

### 2.1 System Architecture
The application runs as a single Linux process (or inside a lightweight Docker container) comprising five primary subsystems:

```
+---------------------------------------------------------------+
|                       SiteWarden Core                         |
+---------------------------------------------------------------+
|                                                               |
|  +-------------------+       +-----------------------------+  |
|  |  Config Watcher   | ----> |  State Store (ArcSwap)      |  |
|  |  (notify / FS)    |       |  (In-Memory AppConfig)      |  |
|  +-------------------+       +-----------------------------+  |
|                                             |                 |
|  +-------------------+                      v                 |
|  |  Cron Scheduler   | ----> +-----------------------------+  |
|  |  (tokio-cron)     |       |  Test Execution Engine      |  |
|  +-------------------+       +-----------------------------+  |
|                                       |           |           |
|                                       v           v           |
|                     +-------------------+  +---------------+  |
|                     | Chromiumoxide CDP |  | Alert Engine  |  |
|                     | Browser / Tabs    |  | Webhooks      |  |
|                     +-------------------+  +---------------+  |
|                                                               |
+---------------------------------------------------------------+
```

### 2.2 Product Functions
* **Schedule Management:** Parses standard 5-to-6-field cron expressions and triggers test cycles.
* **Live Configuration Sync:** Continuously watches `config.yaml` for file modification events and reloads the active configuration without service disruption.
* **Browser Lifecycle Management:** Maintains a headless Chromium instance, spawning isolated tabs/pages for test execution and ensuring proper teardown to prevent memory leaks.
* **Step Interpretation:** Sequentially evaluates navigation, selector synchronization, text assertions, and interactions against the DOM.
* **Artifact Generation:** Captures full-page PNG screenshots on assertion failures and saves them to a designated volume.
* **Notification Dispatch:** Formats failure metadata and dispatches alert payloads to configured HTTP webhooks.

### 2.3 Operating Environment
* **Target Operating System:** Linux (Kernel >= 5.4, x86_64 and aarch64).
* **Container Runtime:** Docker Engine >= 24.0 or Containerd.
* **Chromium Binary:** Headless Chromium >= 120.0.
* **Minimum Host Resources:** 1 vCPU, 512 MB RAM (1 GB recommended for high tab concurrency).

---

## 3. Specific Functional Requirements

### 3.1 Module 1: Configuration & Runtime State Management

#### FR-1.1: Configuration Schema
The system must deserialize configuration from YAML with the following specification:

```rust
pub struct AppConfig {
    pub schedule: String,               // Cron expression
    pub browser_concurrency: usize,     // Max concurrent test tabs (default: 2)
    pub timeout_seconds: u64,           // Global test timeout (default: 30)
    pub screenshot_dir: String,         // Path to store screenshots
    pub alert_webhooks: Vec<String>,    // Destination webhook URLs
    pub suites: Vec<TestSuite>,
}

pub struct TestSuite {
    pub name: String,
    pub base_url: String,
    pub tests: Vec<TestCase>,
}

pub struct TestCase {
    pub name: String,
    pub steps: Vec<TestStep>,
}
```

#### FR-1.2: Dynamic Hot-Reloading
* The system **SHALL** register a non-recursive filesystem watch on `config.yaml` using the `notify` crate.
* Upon detecting a write/modify event, the system **SHALL** debounce events for 300ms before initiating a parse cycle.
* If parsing succeeds, the new configuration **SHALL** be stored atomically in memory via `arc_swap::ArcSwap::store`.
* If parsing fails (e.g. invalid YAML syntax), the system **SHALL** log an error with line and column details, and **RETAIN** the previous valid configuration without panicking.

---

### 3.2 Module 2: Scheduling & Job Orchestration

#### FR-2.1: Cron Expression Evaluation
* The scheduler **SHALL** support standard 6-field cron expressions (e.g., `0 0 6 * * *` for 06:00:00 UTC daily).
* The scheduler **SHALL** query the atomic configuration pointer on each execution tick to ensure the latest test definitions are used.

#### FR-2.2: Concurrency & Worker Throttling
* Test cases **SHALL** be processed concurrently using asynchronous streams (`futures::stream::buffer_unordered`).
* The number of simultaneously open browser tabs **SHALL NOT** exceed `browser_concurrency`.

---

### 3.3 Module 3: Browser Automation & Step Execution

#### FR-3.1: Browser Process Initialization
The system **SHALL** launch Chromium with the following required operational arguments:
- `--no-sandbox`
- `--disable-setuid-sandbox`
- `--disable-dev-shm-usage` (redirects shared memory buffers to avoid VPS `/dev/shm` crashes)
- `--disable-gpu`
- `--headless=new`

#### FR-3.2: Supported Test Steps & Actions
The system **SHALL** execute the following declarative step types:

| Action | Parameters | Behavior |
| :--- | :--- | :--- |
| `navigate` | `path: String` | Resolves target URL against `base_url` and commands the browser page to navigate. Waits for the `DOMContentLoaded` event. |
| `wait_for_selector` | `selector: String`, `timeout_ms: Option<u64>` | Polls the DOM until the CSS selector exists and `is_displayed() == true`. Throws a timeout error if not satisfied within `timeout_ms` (default: 5000ms). |
| `assert_text` | `selector: String`, `contains: String` | Queries the target element, extracts `inner_text()`, and asserts that it contains the expected substring. |
| `assert_visible` | `selector: String` | Asserts that the element exists and is rendered with visible dimensions (`height > 0 && width > 0`). |
| `click` | `selector: String` | Locates the element and dispatches a CDP mouse click event. |
| `type_text` | `selector: String`, `text: String` | Focuses the input element and emits keypress CDP events for the given string. |

#### FR-3.3: Tab Lifecycle Management
* Each test case **MUST** open a dedicated browser page (`about:blank`) and close the page immediately upon test completion or error.
* In the event of an unhandled error or step failure, the page **MUST** be explicitly closed to free memory.

---

### 3.4 Module 4: Artifact Capture & Alert Dispatch

#### FR-4.1: Failure Screenshot Capture
* When an assertion fails or a timeout occurs, the system **SHALL** capture a full-page PNG screenshot before closing the page tab.
* The screenshot filename **SHALL** follow the naming convention:  
  `<screenshot_dir>/<suite_name>_<test_name>_<timestamp>_failed.png`.

#### FR-4.2: Webhook Notification Payload
Upon one or more test failures within a suite run, the system **SHALL** construct and dispatch an HTTP `POST` JSON payload:

```json
{
  "event": "SMOKE_TEST_FAILURE",
  "timestamp": "2026-08-29T06:00:15Z",
  "suite": "Marketing Site",
  "failed_tests": [
    {
      "test_name": "Pricing Page Render Check",
      "failed_step_index": 2,
      "error_message": "Text mismatch on 'h2.pricing-tier'. Expected 'Pro Plan', found 'Error 500'",
      "screenshot_path": "/app/screenshots/Marketing_Site_Pricing_failed.png"
    }
  ]
}
```

---

## 4. Non-Functional Requirements (NFRs)

### 4.1 Performance & Resource Constraints
* **Memory Ceiling:** Under idle state, the process memory footprint must not exceed 30 MB RSS. Under active execution (2 concurrent tabs), memory footprint must not exceed 300 MB RSS.
* **Process Reaping:** The runtime container must execute under `dumb-init` to ensure all orphaned Chromium child processes are reaped correctly.
* **Network Resilience:** All network navigation requests must support a configurable timeout.

### 4.2 Security Requirements
* **No Privilege Escalation:** The binary should run as a non-root user when deployed on bare metal.
* **Secret Masking:** Alert webhooks and authorization headers must not be printed in plaintext to stdout logs unless `RUST_LOG=trace`.

### 4.3 Reliability & Availability
* **Panic Resilience:** Any panic in a single test case worker must be caught and logged; the scheduler and daemon must remain operational.
* **Graceful Termination:** On `SIGINT` (`Ctrl+C`) or `SIGTERM`, the daemon must gracefully close open browser connections and shutdown within 3 seconds.

---

## 5. Deployment & CI/CD Specification

### 5.1 Multi-Stage Dockerfile Specification
* **Stage 1 (Builder):** `rust:1.80-slim-bookworm` compiling with `--release`.
* **Stage 2 (Runtime):** `debian:bookworm-slim` with `chromium`, `fonts-liberation`, and `dumb-init`.

### 5.2 CI/CD Automation
* **Trigger:** Push to `main` branch.
* **Registry:** GitHub Container Registry (`ghcr.io/<org>/sitewarden:latest`).
* **Auto-Update Engine:** Watchtower polling interval set to 300 seconds on production host.
