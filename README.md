# SiteWarden 🛡️

> **Autonomous, ultra-lightweight browser smoke-testing daemon in Rust.**  
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

The script sets up `/opt/sitewarden`, downloads the configuration, pulls the Docker image, and starts the daemon automatically.

---

## 🎯 Key Features

- **🏎️ Hybrid Execution Engine:**
  - **Pure-Rust Static Mode (`reqwest` + `scraper`):** Non-interactive tests run purely in-memory (**~2 MB RAM, ~5ms latency, 0 browser processes**).
  - **On-Demand Headless Browser:** Interactive tests (`click`, `type_text`, `wait_for_selector`) launch headless Chromium on-demand and immediately release all memory upon completion.
- **🌱 Low VPS Resource Footprint:** Idles at **~25–30 MB RSS**, designed specifically for budget \$5/mo (512MB RAM) cloud VPS nodes.
- **🔄 Zero-Downtime Hot-Reloading:** Uses inotify filesystem watchers to detect changes to `config.yaml` and re-schedule cron cycles without restarting the container.
- **📸 Failure Screenshots:** Automatically captures full-page high-resolution screenshots on test failures and saves them to the host directory.
- **⏰ Granular Scheduling:** Standard 5-field or second-precise 6-field cron expressions.

---

## 🛠️ Management & Commands

Once installed in `/opt/sitewarden`:

```bash
# View live daemon logs
cd /opt/sitewarden && docker compose logs -f

# Trigger an immediate smoke test cycle
docker exec -it sitewarden sitewarden --run-once

# View failure screenshots
ls -la /opt/sitewarden/screenshots/

# Restart daemon
cd /opt/sitewarden && docker compose restart
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
| `navigate` | `path: "/route"` | Navigates to relative path or absolute URL |
| `assert_text` | `selector: "h1"`, `contains: "Text"` | Asserts DOM element contains text substring |
| `assert_visible` | `selector: ".btn"` | Asserts DOM element exists and is visible |
| `wait_for_selector`| `selector: "#modal"`, `timeout_ms: 5000` | Polls DOM until element appears |
| `click` | `selector: "button#submit"` | Scrolls to element and triggers click event |
| `type_text` | `selector: "input#name"`, `text: "val"` | Focuses input and types characters |

---

## 💻 Manual Deployment (Without Installer)

If you prefer deploying by hand:

```bash
mkdir -p /opt/sitewarden/screenshots && cd /opt/sitewarden
sudo chown -R 1000:1000 screenshots

curl -fsSL https://raw.githubusercontent.com/Shantodotdev/sitewarden/master/docker-compose.yml -o docker-compose.yml
curl -fsSL https://raw.githubusercontent.com/Shantodotdev/sitewarden/master/config.example.yaml -o config.yaml

docker compose up -d
```

---

## 📄 License
 
Dual-licensed under either:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
