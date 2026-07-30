//! Axum web server providing the REST API for the Hi5bot dashboard.
//!
//! ## Endpoints
//!
//! ### Dashboard
//! - `GET  /api/overview`               — Portfolio positions, cash, SGOV pool
//! - `GET  /api/radar`                  — Latest extreme zone snapshot
//! - `GET  /api/radar/history`          — Historical radar snapshots (query: start, end)
//!
//! ### Backtest
//! - `POST /api/backtest`              — Run Hi5 vs Hi5e backtest (body: BacktestRequest)
//! - `GET  /api/backtest/cached`       — List cached backtest results
//!
//! ### Orders
//! - `GET  /api/orders`                — Recent orders (query: account, limit)
//! - `GET  /api/orders/log`            — Full order log
//!
//! ### Health
//! - `GET  /api/health`                — Health check + uptime
//!
//! ### Static
//! - `GET  /`                          — Serves the dashboard HTML (if embedded)
//!                                      or redirects to the Next.js frontend.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

use crate::backtest::{BacktestDay, BacktestRequest, run_backtest};
use crate::db::Database;
use crate::radar::{classify_zone, RadarSnapshot};
use crate::types::PORTFOLIO;

// ---- Application state --------------------------------------------------

pub struct AppState {
    pub db: Database,
    pub start_time: Instant,
}

impl AppState {
    pub fn new(db: Database) -> Self {
        AppState {
            db,
            start_time: Instant::now(),
        }
    }
}

pub type SharedState = Arc<AppState>;

// ---- Router -------------------------------------------------------------

pub fn build_router(state: SharedState) -> Router {
    Router::new()
        // Dashboard
        .route("/api/overview", get(get_overview))
        .route("/api/radar", get(get_radar))
        .route("/api/radar/history", get(get_radar_history))
        // Backtest
        .route("/api/backtest", post(post_backtest))
        .route("/api/backtest/cached", get(get_backtest_cached))
        // Orders
        .route("/api/orders", get(get_orders))
        .route("/api/orders/log", get(get_orders_log))
        // Health
        .route("/api/health", get(health))
        // CORS
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

// ---- Handlers -----------------------------------------------------------

/// GET /api/overview
///
/// **Estimated shell** until a live portfolio cache exists.
/// Positions are inferred from recent `order_log` rows (last order per ticker),
/// not full Questrade marks. `cash_usd` / `sgov_pool` / PnL are placeholders
/// (`sgov_pool` is backtest-only and always 0.0 here).
async fn get_overview(State(state): State<SharedState>) -> impl IntoResponse {
    let latest_signal = state.db.latest_market_signal().ok().flatten();
    let recent_orders = state.db.recent_orders(None, 50).unwrap_or_default();

    let positions: Vec<PortfolioPosition> = PORTFOLIO
        .iter()
        .map(|meta| {
            let ticker_str = meta.ticker.as_str();
            let last_order = recent_orders.iter().find(|o| o.ticker == ticker_str);
            PortfolioPosition {
                ticker: ticker_str.to_string(),
                name: meta.name.to_string(),
                target_weight_pct: 20.0,
                current_weight_pct: Some(20.0),
                shares: last_order.map(|o| o.shares as u64),
                price: last_order.map(|o| o.limit_price),
                market_value: last_order.map(|o| o.limit_price * (o.shares as f64)),
            }
        })
        .collect();

    Json(serde_json::json!({
        "positions": positions,
        "cash_usd": 0.0,
        "sgov_pool": 0.0,
        "total_value": positions.iter().map(|p| p.market_value.unwrap_or(0.0)).sum::<f64>(),
        "total_invested": positions.iter().map(|p| p.market_value.unwrap_or(0.0)).sum::<f64>(),
        "total_pnl_pct": 0.0,
        "last_updated": latest_signal.as_ref().map(|s| s.date.to_string()),
        "extreme_zone": latest_signal.and_then(|s| s.extreme_zone),
    }))
}

#[derive(Serialize)]
struct PortfolioPosition {
    ticker: String,
    name: String,
    target_weight_pct: f64,
    current_weight_pct: Option<f64>,
    shares: Option<u64>,
    price: Option<f64>,
    market_value: Option<f64>,
}

/// GET /api/radar
///
/// Latest extreme zone classification with all three pillar values.
async fn get_radar(State(state): State<SharedState>) -> impl IntoResponse {
    let latest = state.db.latest_market_signal().ok().flatten();

    match latest {
        Some(sig) => {
            let snapshot = classify_zone(
                sig.aaii_bulls,
                sig.aaii_bears,
                sig.naaim_exposure,
                sig.sp500_pct_above_200ma,
                sig.vix,
                None, // RSP daily return not in DB
                None, // RSP monthly drawdown not in DB
            );
            Json(serde_json::to_value(&snapshot).unwrap()).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "No market signal data available yet. The daemon collects data daily at 15:30 ET."
            })),
        )
            .into_response(),
    }
}

/// GET /api/radar/history?start=YYYY-MM-DD&end=YYYY-MM-DD
#[derive(Deserialize)]
struct RadarHistoryQuery {
    start: Option<String>,
    end: Option<String>,
}

async fn get_radar_history(
    State(state): State<SharedState>,
    Query(q): Query<RadarHistoryQuery>,
) -> impl IntoResponse {
    let start = q
        .start
        .as_deref()
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| {
            chrono::Utc::now().date_naive() - chrono::Duration::days(90)
        });
    let end = q
        .end
        .as_deref()
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| chrono::Utc::now().date_naive());

    match state.db.market_signals_range(start, end) {
        Ok(records) => {
            let snapshots: Vec<RadarSnapshot> = records
                .iter()
                .map(|sig| {
                    classify_zone(
                        sig.aaii_bulls,
                        sig.aaii_bears,
                        sig.naaim_exposure,
                        sig.sp500_pct_above_200ma,
                        sig.vix,
                        None,
                        None,
                    )
                })
                .collect();
            Json(serde_json::json!({
                "count": snapshots.len(),
                "snapshots": snapshots,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /api/backtest
///
/// Body: `BacktestRequest` + `data: Vec<BacktestDay>`
async fn post_backtest(
    State(state): State<SharedState>,
    Json(body): Json<BacktestPayload>,
) -> impl IntoResponse {
    let req = BacktestRequest {
        start_date: body.start_date,
        end_date: body.end_date,
        monthly_contribution: body.monthly_contribution,
        safety_buffer_m: body.safety_buffer_m,
    };

    match run_backtest(&req, &body.data) {
        Ok(result) => {
            // Cache the result
            let _ = state.db.upsert_backtest(&crate::db::BacktestCacheEntry {
                strategy: "hi5".into(),
                start_date: Some(body.start_date),
                end_date: Some(body.end_date),
                cagr: Some(result.hi5.cagr_pct),
                max_drawdown: Some(result.hi5.max_drawdown_pct),
                sharpe: Some(result.hi5.sharpe_ratio),
                final_nav: Some(result.hi5.final_nav),
                raw_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            });
            let _ = state.db.upsert_backtest(&crate::db::BacktestCacheEntry {
                strategy: "hi5e".into(),
                start_date: Some(body.start_date),
                end_date: Some(body.end_date),
                cagr: Some(result.hi5e.cagr_pct),
                max_drawdown: Some(result.hi5e.max_drawdown_pct),
                sharpe: Some(result.hi5e.sharpe_ratio),
                final_nav: Some(result.hi5e.final_nav),
                raw_json: None,
            });
            Json(serde_json::to_value(&result).unwrap()).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct BacktestPayload {
    start_date: chrono::NaiveDate,
    end_date: chrono::NaiveDate,
    #[serde(default)]
    monthly_contribution: Option<f64>,
    #[serde(default)]
    safety_buffer_m: Option<u32>,
    data: Vec<BacktestDay>,
}

/// GET /api/backtest/cached
async fn get_backtest_cached(State(state): State<SharedState>) -> impl IntoResponse {
    // Returns the most recent cached results for hi5 and hi5e
    let now = chrono::Utc::now().date_naive();
    let start = now - chrono::Duration::days(365 * 5);

    let hi5 = state
        .db
        .get_backtest("hi5", start, now)
        .ok()
        .flatten()
        .and_then(|c| c.raw_json)
        .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok());

    Json(serde_json::json!({
        "cached": hi5.is_some(),
        "result": hi5,
    }))
    .into_response()
}

/// GET /api/orders?account=RESP&limit=20
#[derive(Deserialize)]
struct OrdersQuery {
    account: Option<String>,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    50
}

async fn get_orders(
    State(state): State<SharedState>,
    Query(q): Query<OrdersQuery>,
) -> impl IntoResponse {
    match state.db.recent_orders(q.account.as_deref(), q.limit) {
        Ok(orders) => Json(serde_json::json!({
            "count": orders.len(),
            "orders": orders,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/orders/log — alias for /api/orders with default params
async fn get_orders_log(State(state): State<SharedState>) -> impl IntoResponse {
    match state.db.recent_orders(None, 100) {
        Ok(orders) => Json(serde_json::json!({
            "count": orders.len(),
            "orders": orders,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /api/health
async fn health(State(state): State<SharedState>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    Json(serde_json::json!({
        "status": "ok",
        "uptime_secs": uptime,
        "version": env!("CARGO_PKG_VERSION"),
        "service": "hi5bot",
    }))
    .into_response()
}
