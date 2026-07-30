//! Multi-channel Alerting & Notifications (spec §5.3 & Unit M6.1).
//!
//! Supports:
//! - Webhook notifications (Slack/Discord/Generic)
//! - Telegram Bot notifications
//!
//! On trade execution, market radar zone escalation, or hard-abort panics
//! (e.g. USD cash exhausted, currency hard-lock violation), notifications are sent
//! asynchronously without blocking the main event loop.

use reqwest::Client;

/// Severity level for notification routing and formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl AlertSeverity {
    pub fn emoji(self) -> &'static str {
        match self {
            AlertSeverity::Info => "ℹ️",
            AlertSeverity::Warning => "⚠️",
            AlertSeverity::Critical => "🚨",
        }
    }
}

/// Dispatch notification payload across configured channels (Webhook & Telegram).
pub async fn notify(
    webhook: &str,
    telegram_bot_token: Option<&str>,
    telegram_chat_id: Option<&str>,
    title: &str,
    message: &str,
    severity: AlertSeverity,
) {
    let client = Client::new();

    // 1. Webhook Notification
    if !webhook.trim().is_empty() {
        let payload = serde_json::json!({
            "text": format!("{} [{}] {}", severity.emoji(), title, message),
            "title": title,
            "message": message,
            "severity": format!("{:?}", severity).to_lowercase(),
        });
        match client.post(webhook).json(&payload).send().await {
            Ok(r) => tracing::info!("webhook alert sent (status {})", r.status()),
            Err(e) => tracing::error!("webhook alert failed: {e}"),
        }
    }

    // 2. Telegram Bot Notification
    if let (Some(token), Some(chat_id)) = (telegram_bot_token, telegram_chat_id) {
        if !token.trim().is_empty() && !chat_id.trim().is_empty() {
            let tg_url = format!("https://api.telegram.org/bot{token}/sendMessage");
            let text = format!("{} *{}*\n{}", severity.emoji(), title, message);
            let payload = serde_json::json!({
                "chat_id": chat_id,
                "text": text,
                "parse_mode": "Markdown",
            });
            match client.post(&tg_url).json(&payload).send().await {
                Ok(r) => tracing::info!("telegram alert sent (status {})", r.status()),
                Err(e) => tracing::error!("telegram alert failed: {e}"),
            }
        }
    }

    if webhook.trim().is_empty() && (telegram_bot_token.is_none() || telegram_chat_id.is_none()) {
        tracing::info!("alert log [{:?}] {}: {}", severity, title, message);
    }
}

/// Send trade execution alert.
pub async fn notify_trade_executed(
    webhook: &str,
    bot_token: Option<&str>,
    chat_id: Option<&str>,
    account: &str,
    ticker: &str,
    side: &str,
    shares: u64,
    price: f64,
) {
    let title = "Hi5bot Trade Executed";
    let msg = format!(
        "Account: {}\nAction: {} {} shares of {} @ ${:.2}\nEst. Total: ${:.2}",
        account, side, shares, ticker, price, price * (shares as f64)
    );
    notify(webhook, bot_token, chat_id, title, &msg, AlertSeverity::Info).await;
}

/// Send extreme radar zone escalation alert.
pub async fn notify_radar_escalation(
    webhook: &str,
    bot_token: Option<&str>,
    chat_id: Option<&str>,
    zone: &str,
    multiplier: f64,
) {
    let title = "Market Radar Zone Alert";
    let msg = format!(
        "Market regime escalated to *{}* zone!\nDynamic budget multiplier: {:.1}×",
        zone, multiplier
    );
    notify(webhook, bot_token, chat_id, title, &msg, AlertSeverity::Warning).await;
}

/// Send hard-abort emergency alert.
pub async fn notify_hard_abort(
    webhook: &str,
    bot_token: Option<&str>,
    chat_id: Option<&str>,
    reason: &str,
) {
    let title = "CRITICAL: Hi5bot Hard Abort";
    let msg = format!("Daemon execution aborted!\nReason: {}", reason);
    notify(webhook, bot_token, chat_id, title, &msg, AlertSeverity::Critical).await;
}
