//! Headless Chromium automation and browser page lifecycle management.
//!
//! Manages direct Chrome DevTools Protocol (CDP) communication via `chromiumoxide`,
//! enforcing strict resource bounds, security flags, and automatic tab recycling.

use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::StreamExt;
use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;
use tracing::{debug, info};

/// Wrapper around `chromiumoxide::Browser` that manages background CDP message handling
/// and graceful shutdown.
pub struct BrowserManager {
    browser: Browser,
    handler_handle: JoinHandle<()>,
}

impl BrowserManager {
    /// Launches a new headless Chromium instance with the hardened flags specified in SRS FR-3.1.
    pub async fn launch() -> Result<Self> {
        info!("Launching headless Chromium instance...");

        let mut config_builder = BrowserConfig::builder();

        // Check if CHROME_BIN or CHROMIUM_PATH is set, or fallback to standard system locations
        if let Ok(path) = std::env::var("CHROME_BIN") {
            config_builder = config_builder.chrome_executable(path);
        } else if let Ok(path) = std::env::var("CHROMIUM_PATH") {
            config_builder = config_builder.chrome_executable(path);
        }

        // Apply mandatory flags per SRS FR-3.1 & cloud VPS best practices:
        // - no_sandbox & disable_setuid_sandbox: needed for unprivileged Docker containers
        // - disable_dev_shm_usage: prevents /dev/shm shared memory crashes on low-resource VPS nodes
        // - headless=new: modern Chrome headless architecture supporting accurate viewport rendering
        // - disable background networking & syncing: minimizes CPU and network noise
        let config = config_builder
            .no_sandbox()
            .arg("--disable-setuid-sandbox")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-gpu")
            .arg("--headless=new")
            .arg("--window-size=1920,1080")
            .arg("--disable-background-networking")
            .arg("--disable-default-apps")
            .arg("--disable-extensions")
            .arg("--disable-sync")
            .arg("--disable-translate")
            .arg("--hide-scrollbars")
            .arg("--metrics-recording-only")
            .arg("--mute-audio")
            .arg("--safebrowsing-disable-auto-update")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build browser configuration: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .context("Failed to launch Chromium process via CDP")?;

        // Background task to continuously drive the CDP WebSocket event loop.
        // Must remain active during page interactions and browser closing.
        let handler_handle = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(err) = event {
                    debug!("Browser CDP handler event notice: {}", err);
                }
            }
            debug!("Browser CDP event handler finished.");
        });

        Ok(Self {
            browser,
            handler_handle,
        })
    }

    /// Creates a dedicated clean browser tab (`about:blank`).
    pub async fn new_page(&self) -> Result<Page> {
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .context("Failed to create new browser page")?;
        Ok(page)
    }

    /// Captures a full-page PNG screenshot of the given page and writes it to disk.
    pub async fn capture_screenshot<P: AsRef<Path>>(
        page: &Page,
        destination_path: P,
    ) -> Result<PathBuf> {
        let path = destination_path.as_ref().to_path_buf();

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create screenshot directory: {:?}", parent))?;
        }

        // Capture full page screenshot
        let screenshot_bytes = page
            .screenshot(
                chromiumoxide::page::ScreenshotParams::builder()
                    .format(
                        chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png,
                    )
                    .full_page(true)
                    .build(),
            )
            .await
            .context("Failed to capture screenshot via CDP")?;

        tokio::fs::write(&path, screenshot_bytes)
            .await
            .with_context(|| format!("Failed to write screenshot file to: {:?}", path))?;

        Ok(path)
    }

    /// Safely closes a page tab and frees memory in the browser process.
    pub async fn close_page(page: Page) {
        if let Err(err) = page.close().await {
            debug!("Notice while closing browser page: {}", err);
        }
    }

    /// Gracefully closes all browser pages and terminates the browser process.
    pub async fn shutdown(mut self) {
        info!("Shutting down headless Chromium process...");
        // Send close command while handler is still alive with 2s timeout
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), self.browser.close()).await;
        self.handler_handle.abort();
    }
}
