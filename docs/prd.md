# Product Requirement Document (PRD): Hi5bot & Hi5e Trading & Radar System

> **Last updated**: 2026-07-31 — synced with account discovery + Hi5e multiplier corrections.

## 1. Overview & Vision
**Hi5bot** is an automated quantitative ETF trading daemon, market sentiment radar, and backtesting system built in Rust. It automates the execution of the **Hi5 strategy** (dollar-cost averaging into 5 core ETFs with short-board balancing) and its enhanced variant **Hi5e** (dynamic market sentiment/breadth extreme-zone cash deployment).

The system runs 24/7 on local NAS/Docker infrastructure, connecting directly to Questrade API for execution while providing a lightweight Web Dashboard for real-time monitoring, signal visualization, backtest comparison, and manual override capabilities.

---

## 2. Customer & User Personas
* **Automated Long-Term Investor**: Wants hands-off execution of ETF monthly contributions with strict risk rules (USD hard lock, CAD collateral firewall).
* **Quant / Value Trader**: Seeks data-driven market extreme signal detection (AAII, NAAIM, S&P 500 200MA Breadth) to boost returns by deploying cash reserves during market panic.
* **NAS Home Lab Admin**: Requires low-memory footprint (~20MB RAM), single static binary deployment via Docker, and REST API access for custom web dashboards.

---

## 3. Core Product Requirements

### 3.1 Trading Daemon & Execution Engine (Hi5 Core)
* **Target Portfolio**: 5 ETF holdings (`IWY`, `SPMO`, `RSP`, `PFF`, `VNQ`) maintaining target weights (20% default baseline per ticker).
* **Account Discovery**: At startup, queries Questrade `GET v1/accounts` and filters by configured `account_types` (e.g. `["RESP", "TFSA", "RRSP", "Margin"]`). No hardcoded account numbers.
* **Execution Trigger**: Cron execution at 15:30 Eastern Time (ET) on trading calendar triggers.
  * Trigger 1: Third Friday of the month (Standard DCA cycle).
  * Trigger 2: RSP daily return $\le -1\%$ intraday trigger.
  * Max execution limit: Maximum 3 trade triggers per month.
* **Fill-the-Gap Allocation**: Dynamic allocation of available cash towards the most underweight ETF holdings ("short-board fill"). Integer share calculation only.
* **Annual Rebalance**: Hard rebalance on the last trading day of August unless extreme panic conditions delay execution by 15 trading days.
* **Security & Circuit Breakers**:
  * **USD Hard Lock**: Strictly enforce USD settled cash availability; prevent CAD margin/leverage usage.
  * **OAuth Security**: Questrade OAuth2 refresh token rotation with atomic local disk persistence.
  * **Emergency Notifications**: Multi-channel alerts (Generic Webhook + Telegram Bot) via `src/notify.rs` on trade executions, extreme radar escalations, and hard-abort circuit breakers.

### 3.2 Market Extreme Zone Radar (Hi5e Signal Radar)
The radar continuously monitors market sentiment and market structure to classify market state into four zones: `Normal`, `Caution`, `Panic`, `ExtremePanic`.

* **Pillar 1 (AAII Sentiment)**: AAII Bearish % $\ge 55\%$ OR Bullish % $\le 25\%$.
* **Pillar 2 (NAAIM Exposure)**: NAAIM Exposure Index $\le 40\%$.
* **Pillar 3 (Market Breadth)**: S&P 500 % of stocks above 200-day Moving Average $\le 30\%$.
* **Dynamic Budget Multiplier**:
  * `Normal` (0 pillars): $0.5\times$ — deploy 50% of base budget.
  * `Caution` (1 pillar): $0.5\times$ — same as Normal (single pillar treated as noise unless VIX ≥ 35).
  * `Panic` (2 pillars, or 1+VIX≥35): $2.0\times$ — aggressive allocation.
  * `ExtremePanic` (3 pillars + VIX≥35 + RSP≤-3%): $3.0\times$ — maximum aggression.

*Note: SGOV reservoir pool management (`sgov_pool`) is exclusive to the backtest simulator (allocating unspent monthly contribution). The live trading engine applies the dynamic multiplier ($0.5\times \sim 3.0\times$) directly to the per-tick cash buffer budget (`cash / M`), as the live engine manages settled USD cash rather than discrete monthly contributions.*

### 3.3 Strategy Backtesting Laboratory (Hi5 vs Hi5e)
* **Interactive Backtest Engine**: Compare benchmark Hi5 (uniform DCA + annual rebalance) against Hi5e (dynamic sentiment reservoir unlock).
* **Performance Metrics**:
  * Net Asset Value (NAV) growth curves.
  * Compound Annual Growth Rate (CAGR).
  * Maximum Drawdown (MaxDD) — computed from NAV peaks, not daily returns.
  * Sharpe Ratio.
* **Result Caching**: Store backtest results in local SQLite database for instant retrieval.
* **Historical Data**: Accepts `BacktestDay[]` JSON with per-ticker prices and optional sentiment indicators.

### 3.4 Web Dashboard & API Server
Provide a lightweight Axum REST API (Port `8080`) serving data for monitoring and web frontend integration:
* `/api/overview`: Current portfolio positions, estimated market values, and extreme zone status (prices/weights estimated from recent order log until live cache integration).
* `/api/radar`: Real-time market radar status, pillar metrics, and zone classification.
* `/api/radar/history`: Historical radar readings by date range.
* `/api/backtest`: Execute Hi5 vs Hi5e backtest comparison.
* `/api/backtest/cached`: Fetch cached backtest comparisons.
* `/api/orders`: Audit logs of executed and pending orders (filterable by account).
* `/api/orders/log`: Full order log without filters.
* `/api/health`: Service health check & uptime endpoint.

---

## 4. Non-Functional Requirements (NFR)
* **Performance & Footprint**: Rust static binary compiled with `tokio` + `axum`, keeping RAM usage $\le 30\text{MB}$.
* **Data Integrity**: Financial calculations strictly enforced using `rust_decimal::Decimal` (zero floating-point rounding errors).
* **Persistence**: SQLite database in WAL (Write-Ahead Logging) mode for thread-safe concurrent web reads and daemon writes.
* **Timezone Safety**: Strict timezone locking to `America/Toronto` to avoid daylight savings/cron errors.
* **Extensibility**: Account types configurable via TOML whitelist; ETF count fixed at 5 (Hi5 strategy constraint) with per-ticker weights in `AssetMeta`.
