//! Alerting module for SiteWarden.
//!
//! Provides failure notifications across supported backends (SMTP email, console,
//! and future webhook channels), with rich HTML formatting, error diagnostics,
//! and automatic screenshot attachments.

use crate::config::{AppConfig, EmailAlertConfig, SmtpEncryption};
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use chrono::Utc;
use lettre::message::header::ContentType;
use lettre::message::{Attachment, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Failure details for an individual test case.
#[derive(Debug, Clone)]
pub struct FailedTestDetail {
    /// Name of the failed test case.
    pub test_name: String,
    /// 0-indexed step number where execution stopped.
    pub step_index: usize,
    /// Action that failed (e.g., `navigate`, `wait_for_selector`, `assert_text`).
    pub action_type: String,
    /// Error message or assertion mismatch description.
    pub error_message: String,
    /// Path to the captured failure screenshot, if any.
    pub screenshot_path: Option<String>,
}

/// Comprehensive failure alert payload.
#[derive(Debug, Clone)]
pub struct FailureAlert {
    /// Name of the test suite that failed.
    pub suite_name: String,
    /// Base URL of the monitored site.
    pub base_url: String,
    /// Number of tests in the suite that failed.
    pub failed_count: usize,
    /// Total number of tests in the suite.
    pub total_tests: usize,
    /// Detailed failure breakdowns.
    pub failed_tests: Vec<FailedTestDetail>,
}

/// Central alert dispatcher supporting console logging and automated email alerts.
#[derive(Clone, Default)]
pub struct AlertDispatcher {
    shared_config: Option<Arc<ArcSwap<AppConfig>>>,
}

impl AlertDispatcher {
    /// Creates a new alert dispatcher without active configuration.
    pub fn new() -> Self {
        Self {
            shared_config: None,
        }
    }

    /// Creates an alert dispatcher with access to hot-reloaded configuration.
    pub fn with_config(config: Arc<ArcSwap<AppConfig>>) -> Self {
        Self {
            shared_config: Some(config),
        }
    }

    /// Dispatches failure alerts to configured destinations.
    pub async fn dispatch(&self, alert: &FailureAlert) {
        warn!(
            suite = %alert.suite_name,
            failures = alert.failed_count,
            total = alert.total_tests,
            "Smoke test suite incurred failures. Dispatching alerts..."
        );

        if let Some(ref shared) = self.shared_config {
            let app_config = shared.load();
            if let Some(ref alerts) = app_config.alerts {
                if let Some(ref email_config) = alerts.email {
                    if email_config.enabled {
                        info!(
                            suite = %alert.suite_name,
                            smtp_host = %email_config.smtp_host,
                            recipients = email_config.to.len(),
                            "Dispatching email failure alert..."
                        );

                        if let Err(err) = send_email_alert(email_config, alert).await {
                            error!(
                                suite = %alert.suite_name,
                                error = %err,
                                "Failed to send email alert"
                            );
                        } else {
                            info!(
                                suite = %alert.suite_name,
                                "Email alert successfully sent to configured recipients"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Builds an authenticated or unauthenticated SMTP transport based on configuration.
fn build_smtp_transport(
    email_config: &EmailAlertConfig,
) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
    let mut builder = match email_config.encryption {
        SmtpEncryption::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&email_config.smtp_host)
                .with_context(|| {
                    format!(
                        "Failed to initialize STARTTLS SMTP relay for host '{}'",
                        email_config.smtp_host
                    )
                })?
                .port(email_config.smtp_port)
        }
        SmtpEncryption::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&email_config.smtp_host)
            .with_context(|| {
                format!(
                    "Failed to initialize TLS SMTP relay for host '{}'",
                    email_config.smtp_host
                )
            })?
            .port(email_config.smtp_port),
        SmtpEncryption::None => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&email_config.smtp_host)
                .port(email_config.smtp_port)
        }
    };

    if let Some(ref user) = email_config.username {
        if let Some(pwd) = email_config.resolve_password() {
            builder = builder.credentials(Credentials::new(user.clone(), pwd));
        }
    }

    Ok(builder.build())
}

/// Formats a human-friendly timestamp (e.g., "Friday, September 4, 2026 at 2:55:08 AM UTC").
fn friendly_timestamp() -> String {
    Utc::now().format("%A, %B %-d, %Y at %-I:%M:%S %p UTC").to_string()
}

/// Formats and dispatches a test failure notification via SMTP.
pub async fn send_email_alert(email_config: &EmailAlertConfig, alert: &FailureAlert) -> Result<()> {
    let mailer = build_smtp_transport(email_config)?;

    let from_mailbox: lettre::message::Mailbox = email_config
        .from
        .parse()
        .with_context(|| format!("Invalid 'from' address '{}'", email_config.from))?;

    let subject = format!(
        "🚨 [SiteWarden] Alert: '{}' Failed ({} of {} failed)",
        alert.suite_name, alert.failed_count, alert.total_tests
    );

    let mut message_builder = Message::builder().from(from_mailbox).subject(subject);

    for recipient in &email_config.to {
        let to_mailbox: lettre::message::Mailbox = recipient
            .parse()
            .with_context(|| format!("Invalid 'to' address '{}'", recipient))?;
        message_builder = message_builder.to(to_mailbox);
    }

    let timestamp = friendly_timestamp();

    let screenshot_count = alert
        .failed_tests
        .iter()
        .filter(|f| {
            f.screenshot_path
                .as_ref()
                .map(|p| Path::new(p).exists())
                .unwrap_or(false)
        })
        .count();

    // Generate Plaintext Body
    let mut text_body = format!(
        "🚨 SiteWarden Smoke Test Alert\n\
         ======================================================\n\
         Incident:    Smoke Test Failure Detected\n\
         Suite:       {}\n\
         Target URL:  {}\n\
         Impact:      {} of {} Tests Failed\n\
         Timestamp:   {}\n\
         Artifacts:   {} Screenshot(s) Attached\n\
         ======================================================\n\n\
         DIAGNOSTIC FAILURE DETAILS:\n\
         ------------------------------------------------------\n",
        alert.suite_name,
        alert.base_url,
        alert.failed_count,
        alert.total_tests,
        timestamp,
        screenshot_count
    );

    for (idx, failure) in alert.failed_tests.iter().enumerate() {
        text_body.push_str(&format!(
            "[{}/{}] Test: {}\n  • Failed Step: Step {} (Action: {})\n  • Error:       {}\n",
            idx + 1,
            alert.total_tests,
            failure.test_name,
            failure.step_index + 1,
            failure.action_type,
            failure.error_message
        ));
        if let Some(ref p) = failure.screenshot_path {
            let filename = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("screenshot.png");
            text_body.push_str(&format!("  • Screenshot:  Attached as {}\n", filename));
        }
        text_body.push('\n');
    }

    text_body.push_str(
        "QUICK INVESTIGATION GUIDE:\n\
         ------------------------------------------------------\n\
         1. Inspect the attached PNG screenshot to view the visual DOM state at failure.\n\
         2. Verify whether selectors or network latency changed on the target endpoint.\n\
         3. Re-run this test suite immediately via CLI:\n\
            sitewarden test \"<suite_name>\"\n\n\
         -- \n\
         Dispatched autonomously by SiteWarden Sentinel Daemon\n"
    );

    // Generate Card-Based HTML Body for Failed Tests
    let mut failure_cards = String::new();
    for (idx, failure) in alert.failed_tests.iter().enumerate() {
        let screenshot_badge = if let Some(ref p) = failure.screenshot_path {
            let filename = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("screenshot.png");
            format!(
                "<div style='margin-top: 12px; padding: 10px 14px; background-color: #f8fafc; border: 1px solid #e2e8f0; border-radius: 6px; font-size: 12px; color: #475569;'>\
                   📸 <strong>Evidence Captured:</strong> Full-page screenshot attached as <code style='background: #e2e8f0; padding: 2px 6px; border-radius: 4px; color: #0f172a; font-weight: 600;'>{}</code> for visual inspection.\
                 </div>",
                html_escape(filename)
            )
        } else {
            String::new()
        };

        failure_cards.push_str(&format!(
            "<div style='background-color: #ffffff; border: 1px solid #fecaca; border-radius: 8px; margin-bottom: 16px; overflow: hidden; box-shadow: 0 1px 3px rgba(0,0,0,0.04);'>\
               <table role='presentation' border='0' cellpadding='0' cellspacing='0' width='100%' style='background-color: #fff5f5; border-bottom: 1px solid #fee2e2; padding: 12px 16px;'>\
                 <tr>\
                   <td>\
                     <div style='font-size: 14px; font-weight: 700; color: #991b1b;'>\
                       ❌ Test #{}: {}\
                     </div>\
                   </td>\
                   <td align='right' style='white-space: nowrap;'>\
                     <span style='background-color: #fee2e2; border: 1px solid #fecdd3; color: #991b1b; font-size: 11px; font-weight: 700; padding: 3px 8px; border-radius: 4px; text-transform: uppercase;'>\
                       Step {} • {}\
                     </span>\
                   </td>\
                 </tr>\
               </table>\
               <div style='padding: 16px;'>\
                 <div style='font-size: 11px; font-weight: 700; text-transform: uppercase; color: #b91c1c; letter-spacing: 0.05em; margin-bottom: 6px;'>\
                   Diagnostic Traceback / Assertion Error:\
                 </div>\
                 <div style='background-color: #fff1f2; border: 1px solid #fecdd3; border-radius: 6px; padding: 12px 14px; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; font-size: 12px; color: #9f1239; line-height: 1.5; word-break: break-word;'>\
                   {}\
                 </div>\
                 {}\
               </div>\
             </div>",
            idx + 1,
            html_escape(&failure.test_name),
            failure.step_index + 1,
            html_escape(&failure.action_type),
            html_escape(&failure.error_message),
            screenshot_badge
        ));
    }

    let html_body = format!(
        "<!DOCTYPE html>\
         <html lang='en'>\
         <head><meta charset='utf-8'><meta name='viewport' content='width=device-width, initial-scale=1.0'></head>\
         <body style='margin: 0; padding: 32px 16px; background-color: #f1f5f9; font-family: -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, Helvetica, Arial, sans-serif; -webkit-font-smoothing: antialiased; color: #1e293b;'>\
           <table role='presentation' border='0' cellpadding='0' cellspacing='0' width='100%' style='max-width: 640px; margin: 0 auto; background-color: #ffffff; border-radius: 12px; overflow: hidden; box-shadow: 0 4px 16px rgba(15, 23, 42, 0.08); border: 1px solid #e2e8f0;'>\
             <!-- Top Red Alert Stripe -->\
             <tr><td height='5' style='background: linear-gradient(90deg, #ef4444, #dc2626); font-size: 5px; line-height: 5px;'>&nbsp;</td></tr>\
             <!-- Header -->\
             <tr>\
               <td style='padding: 28px 32px 20px 32px; border-bottom: 1px solid #f1f5f9;'>\
                 <table role='presentation' border='0' cellpadding='0' cellspacing='0' width='100%'>\
                   <tr>\
                     <td>\
                       <div style='font-size: 11px; font-weight: 700; color: #64748b; text-transform: uppercase; letter-spacing: 0.08em; margin-bottom: 6px;'>\
                         🛡️ SiteWarden Sentinel\
                       </div>\
                       <h1 style='margin: 0; font-size: 22px; font-weight: 800; color: #0f172a; line-height: 1.25;'>\
                         Smoke Test Failure Detected\
                       </h1>\
                       <div style='margin-top: 6px; font-size: 13px; color: #64748b;'>\
                         📅 {}\
                       </div>\
                     </td>\
                     <td align='right' valign='top'>\
                       <span style='display: inline-block; background-color: #fef2f2; border: 1px solid #fecaca; color: #b91c1c; font-size: 11px; font-weight: 700; padding: 5px 12px; border-radius: 9999px; text-transform: uppercase; letter-spacing: 0.04em; white-space: nowrap;'>\
                         Action Required\
                       </span>\
                     </td>\
                   </tr>\
                 </table>\
               </td>\
             </tr>\
             <!-- Overview -->\
             <tr>\
               <td style='padding: 24px 32px 8px 32px;'>\
                 <div style='font-size: 14px; line-height: 1.5; color: #334155; margin-bottom: 20px;'>\
                   Scheduled smoke testing detected <strong style='color: #ef4444;'>{} failure(s)</strong> during verification of suite <strong>{}</strong>.\
                 </div>\
                 <!-- 2x2 Stats Grid -->\
                 <table role='presentation' border='0' cellpadding='0' cellspacing='0' width='100%' style='background-color: #f8fafc; border: 1px solid #e2e8f0; border-radius: 8px; margin-bottom: 24px;'>\
                   <tr>\
                     <td width='50%' style='padding: 14px 16px; border-bottom: 1px solid #e2e8f0; border-right: 1px solid #e2e8f0;'>\
                       <div style='font-size: 11px; font-weight: 600; text-transform: uppercase; color: #64748b; letter-spacing: 0.04em;'>Monitored Suite</div>\
                       <div style='font-size: 14px; font-weight: 700; color: #0f172a; margin-top: 3px; word-break: break-word;'>{}</div>\
                     </td>\
                     <td width='50%' style='padding: 14px 16px; border-bottom: 1px solid #e2e8f0;'>\
                       <div style='font-size: 11px; font-weight: 600; text-transform: uppercase; color: #64748b; letter-spacing: 0.04em;'>Target URL</div>\
                       <div style='font-size: 14px; font-weight: 600; margin-top: 3px; word-break: break-all;'>\
                         <a href='{}' target='_blank' style='color: #2563eb; text-decoration: none;'>{} ↗</a>\
                       </div>\
                     </td>\
                   </tr>\
                   <tr>\
                     <td width='50%' style='padding: 14px 16px; border-right: 1px solid #e2e8f0;'>\
                       <div style='font-size: 11px; font-weight: 600; text-transform: uppercase; color: #64748b; letter-spacing: 0.04em;'>Failure Impact</div>\
                       <div style='font-size: 14px; font-weight: 700; color: #dc2626; margin-top: 3px;'>\
                         {} of {} Tests Failed\
                       </div>\
                     </td>\
                     <td width='50%' style='padding: 14px 16px;'>\
                       <div style='font-size: 11px; font-weight: 600; text-transform: uppercase; color: #64748b; letter-spacing: 0.04em;'>Evidence Artifacts</div>\
                       <div style='font-size: 14px; font-weight: 600; color: #0f172a; margin-top: 3px;'>\
                         {} Screenshot(s) Attached 📸\
                       </div>\
                     </td>\
                   </tr>\
                 </table>\
               </td>\
             </tr>\
             <!-- Failure Cards Section -->\
             <tr>\
               <td style='padding: 0 32px 16px 32px;'>\
                 <div style='font-size: 13px; font-weight: 700; text-transform: uppercase; color: #475569; letter-spacing: 0.05em; margin-bottom: 12px;'>\
                   Diagnostic Failure Details\
                 </div>\
                 {}\
               </td>\
             </tr>\
             <!-- Recommended Actions -->\
             <tr>\
               <td style='padding: 0 32px 28px 32px;'>\
                 <div style='background-color: #f8fafc; border: 1px solid #e2e8f0; border-radius: 8px; padding: 14px 18px;'>\
                   <div style='font-size: 12px; font-weight: 700; color: #334155; text-transform: uppercase; letter-spacing: 0.04em; margin-bottom: 8px;'>\
                     💡 Quick Investigation Guide\
                   </div>\
                   <ul style='margin: 0; padding-left: 18px; font-size: 13px; color: #475569; line-height: 1.6;'>\
                     <li>Inspect the attached <strong>PNG screenshot</strong> to view the visual browser DOM state at the time of failure.</li>\
                     <li>Verify whether the target element selector changed in recent deployments or requires a longer timeout.</li>\
                     <li>Re-run this test suite immediately via CLI: <code style='background: #e2e8f0; padding: 2px 6px; border-radius: 4px; font-size: 12px; color: #0f172a;'>sitewarden test \"{}\"</code></li>\
                   </ul>\
                 </div>\
               </td>\
             </tr>\
             <!-- Footer -->\
             <tr>\
               <td style='background-color: #f8fafc; border-top: 1px solid #e2e8f0; padding: 20px 32px; text-align: center;'>\
                 <div style='font-size: 12px; color: #64748b; line-height: 1.5;'>\
                   Dispatched autonomously by <strong>SiteWarden Sentinel Daemon</strong><br/>\
                   Continuous browser smoke testing &amp; automated regression monitoring\
                 </div>\
               </td>\
             </tr>\
           </table>\
         </body>\
         </html>",
        timestamp,
        alert.failed_count,
        html_escape(&alert.suite_name),
        html_escape(&alert.suite_name),
        html_escape(&alert.base_url),
        html_escape(&alert.base_url),
        alert.failed_count,
        alert.total_tests,
        screenshot_count,
        failure_cards,
        html_escape(&alert.suite_name)
    );

    // Build email with alternative parts (text and html)
    let alt_part = MultiPart::alternative()
        .singlepart(SinglePart::plain(text_body))
        .singlepart(SinglePart::html(html_body));

    // Gather screenshot attachments if enabled
    let mut attachments: Vec<SinglePart> = Vec::new();
    if email_config.attach_screenshot {
        for failure in &alert.failed_tests {
            if let Some(ref path_str) = failure.screenshot_path {
                let p = Path::new(path_str);
                if p.exists() {
                    match std::fs::read(p) {
                        Ok(data) => {
                            let filename = p
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("screenshot.png");
                            let content_type =
                                ContentType::parse("image/png").unwrap_or(ContentType::TEXT_PLAIN);
                            attachments.push(
                                Attachment::new(filename.to_string()).body(data, content_type),
                            );
                        }
                        Err(err) => {
                            warn!(path = %path_str, error = %err, "Could not read screenshot artifact for email attachment");
                        }
                    }
                }
            }
        }
    }

    let email_message = if attachments.is_empty() {
        message_builder
            .multipart(alt_part)
            .context("Failed to construct email message")?
    } else {
        let mut mixed = MultiPart::mixed().multipart(alt_part);
        for att in attachments {
            mixed = mixed.singlepart(att);
        }
        message_builder
            .multipart(mixed)
            .context("Failed to construct multipart email message with attachments")?
    };

    mailer
        .send(email_message)
        .await
        .context("Failed to transmit email through SMTP server")?;

    Ok(())
}

/// Sends a one-off test email to verify SMTP configuration and recipient reachability.
pub async fn send_test_email(
    email_config: &EmailAlertConfig,
    recipient_override: Option<&str>,
) -> Result<()> {
    let mailer = build_smtp_transport(email_config)?;

    let from_mailbox: lettre::message::Mailbox = email_config
        .from
        .parse()
        .with_context(|| format!("Invalid 'from' address '{}'", email_config.from))?;

    let recipients = if let Some(recip) = recipient_override {
        vec![recip.to_string()]
    } else {
        email_config.to.clone()
    };

    let subject = "✅ [SiteWarden] Test Alert: SMTP Pipeline Verified".to_string();

    let mut message_builder = Message::builder().from(from_mailbox).subject(subject);

    for recipient in &recipients {
        let to_mailbox: lettre::message::Mailbox = recipient
            .parse()
            .with_context(|| format!("Invalid 'to' address '{}'", recipient))?;
        message_builder = message_builder.to(to_mailbox);
    }

    let timestamp = friendly_timestamp();
    let recipients_display = recipients.join(", ");

    let text_body = format!(
        "SiteWarden SMTP Verification\n\
         ======================================================\n\
         Status:      VERIFIED & OPERATIONAL\n\
         Timestamp:   {}\n\
         SMTP Host:   {}:{}\n\
         Encryption:  {:?}\n\
         Sender:      {}\n\
         Recipient:   {}\n\
         ======================================================\n\n\
         Congratulations! Your SiteWarden email alerting pipeline is configured correctly and fully operational.\n\
         When smoke tests detect failures or regressions, automated incident alerts with failure screenshots will be delivered here.\n\n\
         -- \n\
         Dispatched autonomously by SiteWarden Sentinel Daemon\n",
        timestamp,
        email_config.smtp_host,
        email_config.smtp_port,
        email_config.encryption,
        email_config.from,
        recipients_display
    );

    let html_body = format!(
        "<!DOCTYPE html>\
         <html lang='en'>\
         <head><meta charset='utf-8'><meta name='viewport' content='width=device-width, initial-scale=1.0'></head>\
         <body style='margin: 0; padding: 32px 16px; background-color: #f1f5f9; font-family: -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, Helvetica, Arial, sans-serif; -webkit-font-smoothing: antialiased; color: #1e293b;'>\
           <table role='presentation' border='0' cellpadding='0' cellspacing='0' width='100%' style='max-width: 600px; margin: 0 auto; background-color: #ffffff; border-radius: 12px; overflow: hidden; box-shadow: 0 4px 16px rgba(15, 23, 42, 0.08); border: 1px solid #e2e8f0;'>\
             <!-- Top Emerald Stripe -->\
             <tr><td height='5' style='background: linear-gradient(90deg, #10b981, #059669); font-size: 5px; line-height: 5px;'>&nbsp;</td></tr>\
             <!-- Header -->\
             <tr>\
               <td style='padding: 28px 32px 20px 32px; border-bottom: 1px solid #f1f5f9;'>\
                 <table role='presentation' border='0' cellpadding='0' cellspacing='0' width='100%'>\
                   <tr>\
                     <td>\
                       <div style='font-size: 11px; font-weight: 700; color: #64748b; text-transform: uppercase; letter-spacing: 0.08em; margin-bottom: 6px;'>\
                         🛡️ SiteWarden Sentinel\
                       </div>\
                       <h1 style='margin: 0; font-size: 22px; font-weight: 800; color: #0f172a; line-height: 1.25;'>\
                         SMTP Verification Successful\
                       </h1>\
                       <div style='margin-top: 6px; font-size: 13px; color: #64748b;'>\
                         📅 {}\
                       </div>\
                     </td>\
                     <td align='right' valign='top'>\
                       <span style='display: inline-block; background-color: #ecfdf5; border: 1px solid #a7f3d0; color: #047857; font-size: 11px; font-weight: 700; padding: 5px 12px; border-radius: 9999px; text-transform: uppercase; letter-spacing: 0.04em; white-space: nowrap;'>\
                         Operational\
                       </span>\
                     </td>\
                   </tr>\
                 </table>\
               </td>\
             </tr>\
             <!-- Body -->\
             <tr>\
               <td style='padding: 24px 32px;'>\
                 <p style='margin: 0 0 16px 0; font-size: 14px; line-height: 1.6; color: #334155;'>\
                   Your email alerting pipeline is configured correctly and authenticated with <strong>{}</strong>. \
                   When smoke test suites encounter assertion failures or timeouts, detailed diagnostic alerts and full-page screenshots will be sent here automatically.\
                 </p>\
                 <!-- Config Summary Grid -->\
                 <table role='presentation' border='0' cellpadding='0' cellspacing='0' width='100%' style='background-color: #f8fafc; border: 1px solid #e2e8f0; border-radius: 8px; margin-bottom: 16px;'>\
                   <tr>\
                     <td width='50%' style='padding: 12px 16px; border-bottom: 1px solid #e2e8f0; border-right: 1px solid #e2e8f0;'>\
                       <div style='font-size: 11px; font-weight: 600; text-transform: uppercase; color: #64748b;'>SMTP Relay Host</div>\
                       <div style='font-size: 13px; font-weight: 700; color: #0f172a; margin-top: 2px;'>{}:{}</div>\
                     </td>\
                     <td width='50%' style='padding: 12px 16px; border-bottom: 1px solid #e2e8f0;'>\
                       <div style='font-size: 11px; font-weight: 600; text-transform: uppercase; color: #64748b;'>Transport Security</div>\
                       <div style='font-size: 13px; font-weight: 700; color: #0f172a; margin-top: 2px;'>{:?}</div>\
                     </td>\
                   </tr>\
                   <tr>\
                     <td width='50%' style='padding: 12px 16px; border-right: 1px solid #e2e8f0;'>\
                       <div style='font-size: 11px; font-weight: 600; text-transform: uppercase; color: #64748b;'>Sender Address</div>\
                       <div style='font-size: 13px; font-weight: 600; color: #0f172a; margin-top: 2px; word-break: break-all;'>{}</div>\
                     </td>\
                     <td width='50%' style='padding: 12px 16px;'>\
                       <div style='font-size: 11px; font-weight: 600; text-transform: uppercase; color: #64748b;'>Active Recipient(s)</div>\
                       <div style='font-size: 13px; font-weight: 600; color: #0f172a; margin-top: 2px; word-break: break-all;'>{}</div>\
                     </td>\
                   </tr>\
                 </table>\
               </td>\
             </tr>\
             <!-- Footer -->\
             <tr>\
               <td style='background-color: #f8fafc; border-top: 1px solid #e2e8f0; padding: 20px 32px; text-align: center;'>\
                 <div style='font-size: 12px; color: #64748b; line-height: 1.5;'>\
                   Dispatched autonomously by <strong>SiteWarden Sentinel Daemon</strong><br/>\
                   Continuous browser smoke testing &amp; automated regression monitoring\
                 </div>\
               </td>\
             </tr>\
           </table>\
         </body>\
         </html>",
        timestamp,
        html_escape(&email_config.smtp_host),
        html_escape(&email_config.smtp_host),
        email_config.smtp_port,
        email_config.encryption,
        html_escape(&email_config.from),
        html_escape(&recipients_display)
    );

    let alt_part = MultiPart::alternative()
        .singlepart(SinglePart::plain(text_body))
        .singlepart(SinglePart::html(html_body));

    let email_message = message_builder
        .multipart(alt_part)
        .context("Failed to construct test email message")?;

    mailer
        .send(email_message)
        .await
        .context("Failed to deliver test email via SMTP")?;

    Ok(())
}

/// Helper function to escape basic HTML entities.
fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}
