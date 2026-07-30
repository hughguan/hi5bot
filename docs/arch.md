# System Architecture Document (arch.md)

> **Last updated**: 2026-07-31 — account discovery + live vs backtest Hi5e boundary documented.

## 1. System Context & Overview

The **Hi5bot** system is structured as a unified static binary combining a **Trading Daemon** (cron/scheduler worker), an **Axum Web API Server**, an **SQLite Data Storage Layer**, and a **Market Extreme Radar & Backtest Engine**.

```mermaid
graph TD
    subgraph External Systems
        Q[Questrade API]
        M[AAII / NAAIM / Macro Data Sources]
        U[User Web Browser / Client]
    end

    subgraph Hi5bot Core System (Docker / NAS Container)
        API[Axum Web API Server :8080]
        CRON[Trading Daemon Cron 15:30 ET]
        RADAR[Market Extreme Radar Engine]
        BT[Backtest Engine]
        DISC[Account Discovery]
        FETCH[Market Sentiment Fetcher]
        DB[(SQLite DB - WAL Mode)]
    end

    Q <-->|OAuth2 & Order REST API| CRON
    Q -->|GET /v1/accounts| DISC
    DISC -->|Lock in active accounts| CRON
    M -->|Fetch AAII / NAAIM| FETCH
    FETCH -->|Write market signals| DB
    RADAR -->|Classify zone| CRON
    CRON -->|Log Executions & Signals| DB
    API <-->|Read / Write State| DB
    API <-->|Trigger On-demand Backtest| BT
    U <-->|JSON REST API| API
```

---

## 2. Component Architecture

### 2.1 Core Modules Breakdown
* **`src/main.rs`**: Application entrypoint & CLI argument parser (`--web-only`, `--once`, `--dry-run`). Performs account discovery at startup, then spawns background trading daemon and Axum web server concurrently via Tokio.
* **`src/lib.rs`**: Module hierarchy declaration and core re-exports.
* **`src/accounts.rs`**: Questrade API-driven account discovery. Calls `GET v1/accounts`, filters by `account_types` whitelist, and locks in active accounts.
* **`src/db.rs`**: Thread-safe SQLite abstraction layer built on `rusqlite` with WAL mode. Manages tables: `market_signals`, `order_log`, `backtest_cache`.
* **`src/fetcher.rs`**: Market sentiment data fetcher. Scrapes AAII sentiment survey and NAAIM exposure index (gated behind `web-scraper` feature). Falls back to `None` on failure.
* **`src/radar.rs`**: Market Extreme Zone Radar classification engine evaluating 3 pillars to compute `Zone` (`Normal`, `Caution`, `Panic`, `ExtremePanic`) and dynamic budget multipliers ($0.5\times \sim 3.0\times$).
* **`src/backtest.rs`**: Historical simulator computing NAV, CAGR, Max Drawdown, and Sharpe Ratio for Hi5 vs Hi5e strategies. Manages an in-memory `sgov_pool` cash reservoir for unspent monthly contributions (note: `sgov_pool` is backtest-only; the live engine applies the dynamic multiplier directly to the USD cash buffer budget `cash / M`).
* **`src/web.rs`**: Axum HTTP API routing server with permissive CORS and JSON handlers for 8 primary endpoints.
* **`src/strategy.rs`**: 5-signal state machine implementation for signal priority, gating, and Fill-the-Gap calculations.
* **`src/buffer_pool.rs`**: Cash pool management enforcing integer share flooring and invariant checks.
* **`src/engine.rs`**: Single evaluation tick orchestration — auth → market data → signal → allocate → safety → order. Live Hi5e applies `hi5e_dynamic_budget` to `available_per_trade(cash, M)` only (no SGOV ledger).
* **`src/auth.rs`**: Questrade OAuth2 token refresh & atomic persistence layer.
* **`src/error.rs`**: Centralized system error enum implementing `From<rusqlite::Error>`.
* **`src/calendar.rs`**: Trading calendar helpers (third Friday, last trading day of August, US holidays).
* **`src/questrade.rs`**: Async Questrade REST client (accounts, balances, positions, quotes, candles, orders).
* **`src/state_store.rs`**: Atomic per-account monthly trade counter.
* **`src/notify.rs`**: Multi-channel alerts (Generic Webhook + Telegram Bot) for trade executions, extreme radar escalations, and hard-abort circuit breakers.
* **`src/types.rs`**: Domain types, `Decimal` money newtypes (`UsdCash`, `CadCash`, `UsdPrice`), Questrade DTOs.

---

## 3. Data Architecture (SQLite Schema)

SQLite runs in WAL mode (`PRAGMA journal_mode=WAL;`), enabling concurrent non-blocking reads by the Web API while the Trading Daemon writes daily logs and market signals.

```sql
-- 1. Market Signals & Radar Snapshot Table
CREATE TABLE IF NOT EXISTS market_signals (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    date        TEXT NOT NULL UNIQUE,
    aaii_bulls  REAL,
    aaii_bears  REAL,
    naaim_exposure REAL,
    sp500_pct_above_200ma REAL,
    vix         REAL,
    extreme_zone TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 2. Order Execution Log Table
CREATE TABLE IF NOT EXISTS order_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account     TEXT NOT NULL,
    ticker      TEXT NOT NULL,
    side        TEXT NOT NULL,   -- 'BUY' | 'SELL'
    shares      INTEGER NOT NULL,
    limit_price REAL NOT NULL,
    est_cost    REAL,
    signal      TEXT,
    placed_at   TEXT NOT NULL DEFAULT (datetime('now')),
    status      TEXT NOT NULL DEFAULT 'submitted'
);

-- 3. Backtest Simulation Cache Table
CREATE TABLE IF NOT EXISTS backtest_cache (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    strategy    TEXT NOT NULL,   -- 'hi5' | 'hi5e'
    start_date  TEXT NOT NULL,
    end_date    TEXT NOT NULL,
    cagr        REAL,
    max_drawdown REAL,
    sharpe      REAL,
    final_nav   REAL,
    raw_json    TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(strategy, start_date, end_date)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_market_signals_date ON market_signals(date);
CREATE INDEX IF NOT EXISTS idx_order_log_account ON order_log(account);
CREATE INDEX IF NOT EXISTS idx_order_log_placed ON order_log(placed_at);
```

---

## 4. Operational & Deployment Architecture

### 4.1 NAS Docker Deployment Diagram
```
┌────────────────────────────────────────────────────────┐
│                   NAS Host / Server                    │
│                                                        │
│  ┌──────────────────────────────────────────────────┐  │
│  │ docker-compose: hi5bot                           │  │
│  │                                                  │  │
│  │  ┌────────────────────────────────────────────┐  │  │
│  │  │ hi5bot-core (Container)                    │  │  │
│  │  │  - Static Rust Binary (~4MB) :8080 (Axum API)│  │  │
│  │  │  - Next.js Standalone Bundle :3000 (UI)    │  │  │
│  │  │  - Env: TZ=America/Toronto                 │  │  │
│  │  │  - Port Mappings: 8080 & 3000              │  │  │
│  │  └─────────────────────┬──────────────────────┘  │  │
│  │                        │ Volume Mounts           │  │
│  │                        ▼                         │  │
│  │  ┌────────────────────────────────────────────┐  │  │
│  │  │ Host Directory ./data                      │  │  │
│  │  │  - hi5bot.db (SQLite WAL)                  │  │  │
│  │  │  - tokens.json (OAuth Credentials)         │  │  │
│  │  │  - state.json (Monthly trade counter)      │  │  │
│  │  │  - config.toml (Strategy & account config) │  │  │
│  │  └────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────┘
```

### 4.2 Account Discovery Flow
```
config.toml (account_types=["RESP","TFSA"])
  → token refresh (OAuth2)
    → Questrade GET /v1/accounts
      → filter: status=Active ∧ kind∈account_types
        → stable sort: primary first, then type, then number
          → lock in discovered accounts for trading
```

### 4.3 Live vs Backtest Hi5e (SGOV boundary)

```
                    ┌─────────────────────────────┐
                    │ ExtremeZone multiplier      │
                    │ 0.5× / 0.5× / 2× / 3×       │
                    └─────────────┬───────────────┘
                                  │
              ┌───────────────────┴───────────────────┐
              ▼                                       ▼
     Live engine (engine.rs)              Backtest (backtest.rs)
     ─────────────────────                ─────────────────────
     base = cash / M                      base = monthly_contribution
     budget = base × multiplier           deploy = base × multiplier
     no SGOV position/ledger              remainder ↔ in-memory sgov_pool
     settled USD cash only                simulated reservoir unlock on panic
```

`sgov_pool` exists **only** inside the backtest simulator. Production never buys/sells SGOV or tracks a separate cash reservoir beyond ordinary USD settled cash.

`GET /api/overview` is an **estimated shell** (recent `order_log` + radar zone) until a live portfolio cache is wired; do not treat weights/cash fields as authoritative marks.

### 4.4 Command Execution Modes
```bash
# Full Mode (Cron Daemon + Axum Web API Server)
HI5BOT_DATA_DIR=./data cargo run

# Web-only Mode (No live trading daemon executed)
HI5BOT_DATA_DIR=./data cargo run -- --web-only

# Single Execution Mode (Dry-run test, no Web server)
HI5BOT_DATA_DIR=./data cargo run -- --once --dry-run
```
