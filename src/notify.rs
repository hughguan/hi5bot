//! Hard-abort notifications (spec §5.3).
//!
//! On a panic/hard-abort (e.g. USD cash exhausted, currency hard-lock
//! violation), the daemon sends a notification before exiting. If no webhook is
//! configured, the reason is logged at `error` level.

pub async fn notify(webhook: &str, reason: &str) {
    if webhook.trim().is_empty() {
        tracing::error!("hard-abort (no webhook configured): {reason}");
        return;
    }
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "text": format!("[hi5bot hard-abort] {reason}"),
        "severity": "hard-abort",
    });
    match client.post(webhook).json(&payload).send().await {
        Ok(r) => tracing::info!("notify webhook accepted (status {})", r.status()),
        Err(e) => tracing::error!("notify webhook failed: {e}"),
    }
}
