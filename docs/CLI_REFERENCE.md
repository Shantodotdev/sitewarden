# SiteWarden CLI Comprehensive Reference Manual

This manual provides an in-depth reference for all CLI subcommands, flags, exit codes, and operational procedures in SiteWarden.

---

## Command Syntax

```bash
sitewarden [GLOBAL_OPTIONS] [SUBCOMMAND] [SUBCOMMAND_OPTIONS]
```

### Global Options

| Option | Shorthand | Default | Description |
|---|---|---|---|
| `--config <PATH>` | `-c` | `config.yaml` | Path to the YAML configuration file. Fallbacks: `/app/config/config.yaml`, `/app/config.yaml`. |
| `--run-once` | | `false` | Backwards-compatible flag to run all suites once immediately and exit. |
| `--verbose` | `-v` | `false` | Enables full `chromiumoxide` and CDP WebSocket debug tracing. |
| `--help` | `-h` | | Displays help information. |
| `--version` | `-V` | | Displays the current SiteWarden version. |

---

## Subcommands

### 1. `daemon`
Starts the continuous background testing scheduler daemon with hot-reloading. This is the **default subcommand** when no subcommand is provided.

```bash
sitewarden daemon --config config.yaml
```

* **Behavior**:
  - Initializes inotify file watcher on `config.yaml`.
  - Schedules cron jobs in memory.
  - Dynamically updates cron intervals when `schedule` changes in `config.yaml`.
  - Handles `SIGINT` (Ctrl+C) and `SIGTERM` gracefully, waiting for active test cycles to terminate cleanly.

---

### 2. `status`
Displays an instant ASCII dashboard of daemon health, historical cycle metrics, and storage usage.

```bash
sitewarden status
```

* **Data Sources**:
  - State file (`.sitewarden_state.json` or `state.json`) written atomically after every cycle.
  - Live screenshot storage directory scanner.
  - Background GitHub Releases semver check.
* **Exit Codes**:
  - `0`: Success.
  - `1`: Configuration file missing or unparseable.

---

### 3. `history`
Outputs a timeline table of past test cycle executions.

```bash
sitewarden history [--limit <NUMBER>]
```

* **Flags**:
  - `--limit`, `-l` (Default: `10`): Number of recent cycles to display.
* **Output Columns**:
  - `Timestamp (UTC)`: Execution date and time.
  - `Suites (Pass/Fail)`: Passed suites vs total suites and total steps.
  - `Result`: Colorized `✅ PASS` or `❌ FAIL` badge.
  - `Duration`: Total cycle runtime (e.g., `1.37s` or `420ms`).
  - `Trigger`: `Cycle` (scheduled cron) or `Manual` / `Run-Once`.

---

### 4. `check`
Runs offline syntax validation and live endpoint reachability checks without executing full browser tests.

```bash
sitewarden check [--config <PATH>]
```

* **Checks Performed**:
  - [x] YAML syntax and schema adherence (`deny_unknown_fields`).
  - [x] Cron expression validity (5-field or 6-field second precision).
  - [x] CSS selector syntax validation for every step.
  - [x] DNS resolution and HTTP/HTTPS reachability for every suite's `base_url`.
  - [x] Engine routing calculation (pure-Rust static vs on-demand browser).
* **Exit Codes**:
  - `0`: All checks passed.
  - `1`: Syntax error or unreachable endpoint.

---

### 5. `test`
Executes one or all test suites immediately on demand.

```bash
sitewarden test [SUITE_NAME]
```

* **Arguments**:
  - `SUITE_NAME` (Optional): Name of the specific test suite to run. Case-insensitive substring match (e.g., `sitewarden test "Marketing"` matches `"Marketing Site Health Check"`).
* **Exit Codes**:
  - `0`: All executed test suites passed.
  - `1`: One or more test steps failed (useful in CI/CD pipelines).

---

### 6. `update`
Checks the GitHub Releases API for newer SiteWarden releases.

```bash
sitewarden update [--check]
```

* **Flags**:
  - `--check`: Only performs the version check and returns whether an update is available without printing full upgrade instructions.
* **Automatic Environment Detection**:
  - **Docker Containers**: Detects containerized execution and prints `docker compose pull && docker compose up -d`.
  - **Standalone Binaries**: Detects host OS and architecture, printing the 1-line binary installer.

---

### 7. `prune`
Scans failure screenshot storage and removes stale artifacts older than `N` days.

```bash
sitewarden prune [--days <NUMBER>] [--dry-run]
```

* **Flags**:
  - `--days`, `-d` (Default: `7`): Deletes PNG artifacts with `mtime` older than specified days.
  - `--dry-run`: Simulates deletion without removing files from disk.

---

### 8. `doctor`
Performs environment diagnostics and pre-flight health checks.

```bash
sitewarden doctor
```

* **Diagnostics**:
  1. **Configuration**: File presence and schema validation.
  2. **Storage**: Write, read, and delete permissions in `screenshot_dir`.
  3. **Engine**: Auto-detection of `chrome-headless-shell`, `google-chrome-stable`, or `chromium`.
  4. **Network**: Outbound HTTPS connectivity, TLS handshake, and DNS resolution.
