# SiteWarden 🛡️

> **Autonomous, ultra-lightweight browser smoke-testing daemon & CLI toolset in Rust.**  
> Continuously monitors web apps, validates DOM assertions, captures failure screenshots, and consumes **<30 MB RAM**.

[![CI & Container Build](https://github.com/Shantodotdev/sitewarden/actions/workflows/ci.yml/badge.svg)](https://github.com/Shantodotdev/sitewarden/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](#-license)
[![Docker Image](https://img.shields.io/badge/Docker-ghcr.io%2Fshantodotdev%2Fsitewarden-green.svg)](https://github.com/Shantodotdev/sitewarden/pkgs/container/sitewarden)

---

## ⚡ 1-Line Quickstart (Linux VPS)

Install and run SiteWarden on any Linux server with a single command:

```bash
curl -fsSL https://raw.githubusercontent.com/Shantodotdev/sitewarden/master/install.sh | bash
```

The script sets up `/opt/sitewarden`, initializes your `config.yaml`, sets permissions, pulls the official container image, and starts the background daemon.

---

## 🎯 Key Features

- **🏎️ Hybrid Execution Engine:**
  - **Pure-Rust Static Mode (`reqwest` + `scraper`):** Non-interactive tests run purely in-memory (**~2 MB RAM, ~5ms latency, 0 browser processes**).
  - **On-Demand Headless Browser:** Interactive tests (`click`, `type_text`, `wait_for_selector`) launch headless Chromium on-demand and immediately release all memory upon completion.
- **🌱 Low VPS Resource Footprint:** Idles at **~25–30 MB RSS**, designed specifically for budget \$5/mo (512MB RAM) cloud VPS nodes.
- **📊 Interactive CLI Suite:** Rich subcommands (`status`, `history`, `check`, `test`, `update`, `prune`, `doctor`) for fast observability without sifting through logs.
- **🔄 Zero-Downtime Hot-Reloading:** Uses inotify filesystem watchers to detect changes to `config.yaml` and re-schedules cron cycles live in memory.
- **📸 Failure Screenshots:** Automatically captures full-page high-resolution screenshots on test failures and saves them to the host directory.
- **🎨 Vibrant Terminal Tree Reporting:** Laser-aligned Unicode columns, ANSI color-coded actions, and clean ASCII summary scorecards.

---

## 🖥️ Visual Execution Output

```text
2026-09-01T06:04:18Z  INFO 🚀 Starting Smoke Test Cycle • 2026-09-01 06:04:18 UTC [2 Suites • 2 Tests (8 Steps) • Concurrency: 2]
2026-09-01T06:04:19Z  INFO ▶ Executing Suite: Example Domain Health Check (https://example.com) [Engine: Browser • 1 Tests • 4 Steps]
  ▶ Test: Verify Homepage Heading and Body
    ├── [1/4]   🌐 navigate          → /                                    [OK • 38ms]
    ├── [2/4]   ⏳ wait_for_selector → h1                                   [OK • 11ms]
    ├── [3/4]   🔍 assert_text       → 'h1' contains 'Example Domain'       [OK • 2ms]
    ├── [4/4]   👁️ assert_visible    → p                                    [OK • 7ms]
    └── ✅ Test Passed (61ms)

┌──────────────────────────────────────────────────────────────────────────────────────────┐
│                               SiteWarden Test Cycle Report                               │
├──────────────────────────────────────┬──────────┬────────────┬──────────┬────────────────┤
│ Suite Name                           │ Engine   │ Result     │ Tests    │ Duration       │
├──────────────────────────────────────┼──────────┼────────────┼──────────┼────────────────┤
│ Example Domain Health Check          │ Browser  │ ✅ PASS    │ 1/1 (4)  │ 211ms          │
│ Wikipedia Navigation Test            │ Browser  │ ✅ PASS    │ 1/1 (4)  │ 962ms          │
├──────────────────────────────────────┴──────────┴────────────┴──────────┴────────────────┤
│ ✅ All 2 Suites Passed (100%) • Total Cycle Time: 1.37s                                 │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 🛠️ CLI Subcommand Reference

SiteWarden includes a complete interactive toolset:

```text
sitewarden [OPTIONS] [COMMAND]
```

### 1. `sitewarden status`
Displays the overall daemon health, uptime, total runs, success rate, and screenshot disk usage without digging into logs:

```bash
docker exec -it sitewarden sitewarden status
```
```text
┌────────────────────────── SiteWarden Daemon Status ──────────────────────────┐
│ Status:           🟢 Active (Scheduler Daemon)                            │
│ Version:          v0.1.0 (Latest)                                         │
│ Uptime:           4 days, 12 hours                                        │
│ Schedule:         Cron: '0 0 6 * * *'                                     │
├──────────────────────────────────────────────────────────────────────────────┤
│ Total Cycles:     148 runs (146 Passed, 2 Failed • 98.6% Success Rate)   │
│ Last Run:         2026-09-01 06:04:18 UTC (✅ Passed, 1366ms)             │
├──────────────────────────────────────────────────────────────────────────────┤
│ Monitored Suites: 2 configured (2 tests, 8 steps)                         │
│ Screenshots:      2 artifacts (1.42 MB) in /app/screenshots               │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

### 2. `sitewarden history [--limit N]`
Displays a timeline of recent test cycles:

```bash
docker exec -it sitewarden sitewarden history --limit 5
```
```text
┌──────────────────────┬────────────────────────┬────────────┬──────────────┬────────────┐
│ Timestamp (UTC)     │ Suites (Pass/Fail)   │ Result     │ Duration     │ Trigger    │
├──────────────────────┼────────────────────────┼────────────┼──────────────┼────────────┤
│ 2026-09-01 06:04:18  │ 2/2 (8 steps)          │ ✅ PASS    │ 1.37s        │ Cycle      │
│ 2026-09-01 00:00:00  │ 2/2 (8 steps)          │ ✅ PASS    │ 1.12s        │ Cycle      │
│ 2026-08-31 18:00:00  │ 1/2 (7 steps)          │ ❌ FAIL    │ 2.45s        │ Cycle      │
└──────────────────────┴────────────────────────┴────────────┴──────────────┴────────────┘
```

---

### 3. `sitewarden check`
Pre-flight verification for `config.yaml` syntax, cron expressions, CSS selectors, and target endpoint reachability:

```bash
docker exec -it sitewarden sitewarden check
```
```text
🔍 Validating configuration at: "config.yaml"
  ✅ YAML syntax and schema valid
  ✅ Schedule expression valid (Cron: '0 0 6 * * *')
  ✅ 2 Suites loaded (8 Total Steps)

🌐 Testing endpoint connectivity...
  ✅ Reachable: https://example.com (HTTP 200 OK) → [Headless Browser]
  ✅ Reachable: https://en.wikipedia.org (HTTP 200 OK) → [Headless Browser]

🎉 Configuration is 100% valid and production-ready!
```

---

### 4. `sitewarden test [SUITE]`
Executes a specific test suite or all test suites immediately on demand:

```bash
# Run a specific suite
docker exec -it sitewarden sitewarden test "Example Domain Health Check"

# Run all configured suites
docker exec -it sitewarden sitewarden test
```

---

### 5. `sitewarden update [--check]`
Queries the GitHub Releases API, compares semver, and prints the 1-command upgrade procedure:

```bash
docker exec -it sitewarden sitewarden update
```

---

### 6. `sitewarden prune [--days N] [--dry-run]`
Cleans up old failure screenshot artifacts from disk to prevent VPS storage leaks:

```bash
# Simulate pruning screenshots older than 7 days
docker exec -it sitewarden sitewarden prune --days 7 --dry-run

# Delete screenshots older than 14 days
docker exec -it sitewarden sitewarden prune --days 14
```

---

### 7. `sitewarden doctor`
Runs environment and diagnostic health checks (Chromium binary detection, screenshot directory permissions, network DNS):

```bash
docker exec -it sitewarden sitewarden doctor
```

---

## ⚙️ Configuration (`config.yaml`)

SiteWarden is configured declaratively using YAML:

```yaml
# Cron schedule expression (sec min hour day month weekday)
schedule: "0 0 6 * * *"

# Resource limits & concurrency
browser_concurrency: 2
timeout_seconds: 30
screenshot_dir: "/app/screenshots"

# Declarative Test Suites
suites:
  - name: "Marketing Site Health Check"
    base_url: "https://example.com"
    tests:
      - name: "Verify Homepage Heading"
        steps:
          - action: navigate
            path: "/"
          - action: assert_text
            selector: "h1"
            contains: "Example Domain"
          - action: assert_visible
            selector: "p"

  - name: "User Portal Flow"
    base_url: "https://app.example.com"
    tests:
      - name: "Verify Login Interaction"
        steps:
          - action: navigate
            path: "/login"
          - action: wait_for_selector
            selector: "input#email"
            timeout_ms: 5000
          - action: type_text
            selector: "input#email"
            text: "smoke-test@example.com"
          - action: click
            selector: "button#submit"
```

---

## 📦 Supported Test Step Actions

| Action | Parameters | Description |
| :--- | :--- | :--- |
| `navigate` | `path: "/route"` | Navigates to a relative path or absolute URL |
| `assert_text` | `selector: "h1"`, `contains: "Text"` | Asserts DOM element contains text substring |
| `assert_visible` | `selector: ".btn"` | Asserts DOM element exists and is visible |
| `wait_for_selector`| `selector: "#modal"`, `timeout_ms: 5000` | Polls DOM until element appears |
| `click` | `selector: "button#submit"` | Scrolls to element and triggers click event |
| `type_text` | `selector: "input#name"`, `text: "val"` | Focuses input and types characters |

---

## 💻 Manual Deployment (Without Installer)

If you prefer deploying manually with Docker Compose:

```bash
mkdir -p /opt/sitewarden/screenshots && cd /opt/sitewarden
sudo chown -R 1000:1000 screenshots

curl -fsSL https://raw.githubusercontent.com/Shantodotdev/sitewarden/master/docker-compose.yml -o docker-compose.yml
curl -fsSL https://raw.githubusercontent.com/Shantodotdev/sitewarden/master/config.example.yaml -o config.yaml

docker compose up -d
```

---

## 🔧 Building from Source

To compile and run SiteWarden natively:

```bash
# Clone repository
git clone https://github.com/Shantodotdev/sitewarden.git && cd sitewarden

# Run automated tests (29 unit & integration tests)
cargo test

# Run configuration pre-flight check
cargo run -- --config config.example.yaml check

# Run on-demand test cycle
cargo run -- --config config.example.yaml test

# Start background daemon
cargo run -- --config config.example.yaml daemon
```

---

## 📄 License
 
Dual-licensed under either:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
