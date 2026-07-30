# Technical & Detailed Specification Document (spec.md)

> **Last updated**: 2026-07-31 — synced after account discovery refactor, Hi5e multiplier correction, API corrections.

## 1. Feature Specifications

### 1.1 Account Discovery (`src/accounts.rs`)
At startup, the daemon queries Questrade `GET v1/accounts` and filters to active registered accounts matching `account_types` in `config.toml`.

| Config Key | Type | Default | Description |
|:---|:---|:---|:---|
| `account_types` | `Vec<String>` | `["RESP", "TFSA"]` | Questrade account type whitelist (case-insensitive) |

Supported types: `RESP`, `TFSA`, `RRSP`, `Margin`, `Cash`, `LIRA`.

Discovered accounts are sorted: primary first, then by type, then by number. If no matching active accounts are found, the daemon exits with `Error::ConfigParse`.

### 1.2 Market Radar Engine (`src/radar.rs`)
The Market Radar evaluates market sentiment and breadth indicators according to three primary pillars:

| Pillar | Metric Description | Threshold Criteria |
| :--- | :--- | :--- |
| **Pillar 1** | AAII Sentiment Survey | Bearish % $\ge 55\%$ **OR** Bullish % $\le 25\%$ |
| **Pillar 2** | NAAIM Exposure Index | Active Manager Exposure $\le 40\%$ |
| **Pillar 3** | S&P 500 Market Breadth | % of S&P 500 stocks above 200-day MA $\le 30\%$ |

#### Zone Classification & Dynamic Budget Multipliers
* **`Normal`**: 0 Pillars triggered. Dynamic budget multiplier = $0.5\times$.
* **`Caution`**: 1 Pillar triggered. Dynamic budget multiplier = $0.5\times$ (same as Normal; one flashing pillar is treated as noise unless VIX ≥ 35, which escalates to Panic).
* **`Panic`**: 2 Pillars triggered, or 1 pillar + VIX ≥ 35. Dynamic budget multiplier = $2.0\times$.
* **`ExtremePanic`**: 3 Pillars triggered + VIX ≥ 35 + RSP daily return ≤ -3%. Dynamic budget multiplier = $3.0\times$.

> ⚠️ **Architecture Note (Live vs Backtest SGOV Boundary)**:
> SGOV cash reservoir tracking (`sgov_pool`) is **exclusive to the backtest simulator**, where unspent monthly contributions are accumulated in SGOV. The **live execution engine** does NOT hold or manage an SGOV pool — it applies the dynamic budget multiplier ($0.5\times \sim 3.0\times$) directly to the account's per-tick USD cash buffer budget (`cash / M`).

### 1.3 Market Sentiment Fetcher (`src/fetcher.rs`)
Fetches AAII sentiment and NAAIM exposure via web scraping (gated behind `web-scraper` Cargo feature). On scraper failure, returns `None` for affected pillars — the radar treats missing data as non-extreme (no false signalling).

### 1.4 Backtest Simulator (`src/backtest.rs`)
Simulates portfolio execution over daily historical candles for the 5 target ETFs (`IWY`, `SPMO`, `RSP`, `PFF`, `VNQ`) plus an **in-memory cash-proxy SGOV pool** (backtest-only; see Architecture Note above).

> Live engine does **not** maintain SGOV. Live Hi5e only multiplies the per-tick buffer budget `cash / M` by the zone factor ($0.5\times \sim 3.0\times$).

* **Hi5 Base Model**:
  * Fixed monthly DCA deposit of $\$1,000$ on the 3rd Friday of each month.
  * Fill-the-Gap allocation with full contribution budget (not buffer-pool `cash/M`).
  * Uniform short-board allocation across 5 ETFs.
  * Hard annual rebalance on the last trading day of August.
* **Hi5e Dynamic Model (backtest semantics)**:
  * In `Normal` / `Caution` zone: Deploy $\$500$ ($0.5\times$), route remaining $\$500$ to simulated SGOV reservoir.
  * In `Panic` zone: Deploy $\$2,000$ ($2.0\times$), pulling from simulated SGOV reservoir.
  * In `ExtremePanic` zone: Deploy $\$3,000$ ($3.0\times$), pulling from simulated SGOV reservoir.
  * In `ExtremePanic` zone during August: Delay annual rebalance by 15 trading days to avoid liquidating depressed positions.
* **Metrics Calculated** (computed from NAV series, not daily returns):
  $$\text{CAGR} = \left(\frac{\text{NAV}_{\text{end}}}{\text{Contributions}}\right)^{\frac{1}{\text{Years}}} - 1$$
  $$\text{MaxDD} = \max_{t} \left(\frac{\text{Peak}_t - \text{NAV}_t}{\text{Peak}_t}\right)$$
  $$\text{Sharpe} = \frac{\bar{R}}{\sigma_R} \times \sqrt{252}$$

### 1.5 Axum REST API Specifications (`src/web.rs`)

| Method | Endpoint | Description | Query / Body | Response Schema |
| :--- | :--- | :--- | :--- | :--- |
| `GET` | `/api/overview` | **Estimated** portfolio shell (from recent `order_log` until live position cache). Not full Questrade marks. | None | `{ positions (estimated shares/price/value; weights often placeholder 20%), cash_usd: 0.0, sgov_pool: 0.0, total_value, extreme_zone }` |
| `GET` | `/api/radar` | Current Market Radar status | None | `{ date, zone, pillars, vix, extreme_pillar_count }` |
| `GET` | `/api/radar/history` | Historical radar logs | `?start=YYYY-MM-DD&end=YYYY-MM-DD` | `{ count, snapshots[] }` |
| `POST` | `/api/backtest` | Trigger strategy backtest | `{ start_date, end_date, monthly_contribution?, safety_buffer_m?, data: [BacktestDay] }` | `{ request, hi5: { cagr, max_dd, sharpe, nav }, hi5e: {...}, nav_series }` |
| `GET` | `/api/backtest/cached` | Retrieve cached backtest | None | `{ cached, result }` |
| `GET` | `/api/orders` | Execution order logs | `?account=X&limit=50` | `{ count, orders[] }` |
| `GET` | `/api/orders/log` | Full order log (no filter) | None | `{ count, orders[] }` |
| `GET` | `/api/health` | Service health & uptime | None | `{ status: "ok", version, uptime_secs }` |

`BacktestDay` JSON schema:
```json
{
  "date": "2020-01-17",
  "iwy": 120.0, "spmo": 150.0, "rsp": 180.0, "pff": 28.0, "vnq": 90.0,
  "aaii_bulls": 40.0, "aaii_bears": 30.0,
  "naaim_exposure": 80.0,
  "sp500_pct_above_200ma": 70.0,
  "vix": 18.0
}
```

---

## 2. Test Specifications

The codebase maintains a 62/62 test suite pass rate across 11 functional modules:

```
62 tests passed (0 failed):
├── accounts      2  (label format, sort order)
├── auth          8  (OAuth2 refresh, atomic save, concurrent refresh)
├── buffer_pool  11  (flooring, no-panic, invariants, mock JSON)
├── calendar      7  (third Friday, last Aug trading day)
├── strategy      9  (all 5 signals, gating, precedence)
├── config        4  (parse, validate, reject, default account_types)
├── state_store   4  (increment, rollover, reload, per-account)
├── engine        3  (market state compute, rebalance orders)
├── radar         6  (zone classification, multiplier, dynamic budget)
├── backtest      2  (produces result, rejects too few points)
└── integration   3  (mock Questrade JSON → buffer pool)
```

### Verification Execution
Execute test suite locally with:
```bash
cargo test --all-targets
```

---

## 3. Deployment Specifications

### 3.1 Docker & Environment Config
* **Base Container**: Alpine-based multi-stage Docker build with static Rust binary (~4MB).
* **Port Bindings**: `8080:8080` (HTTP Web API & Dashboard).
* **Environment Variables**:
  * `HI5BOT_DATA_DIR`: Directory path for SQLite storage and token cache (Default: `./data`).
  * `HI5BOT_BIND`: Address binding for Axum server (Default: `0.0.0.0:8080`).
  * `TZ`: Execution timezone lock (Must be `America/Toronto`).
  * `RUST_LOG`: Logging verbosity level (Default: `info`).

### 3.2 Command Execution Modes
```bash
# Full Mode (Account Discovery + Cron Daemon + Axum Web API)
HI5BOT_DATA_DIR=./data cargo run

# Web-only Mode (No live trading daemon)
HI5BOT_DATA_DIR=./data cargo run -- --web-only

# Single Execution Mode (Dry-run test, no Web server)
HI5BOT_DATA_DIR=./data cargo run -- --once --dry-run
```

### 3.3 Configuration File (`config.toml`)
```toml
# Account discovery via Questrade API (replaces hardcoded account numbers)
account_types = ["RESP", "TFSA"]

# ETF symbol IDs + VIX
vix_symbol_id = 0

[symbol_ids]
IWY = 0
SPMO = 0
RSP = 0
PFF = 0
VNQ = 0

# Strategy
safety_buffer_m = 10
settlement_pref = "Currency of Transaction"

# OAuth2
token_url = "https://login.questrade.com/oauth2/token"

# Runtime
eval_time = "15:30"
notify_webhook = ""
```
