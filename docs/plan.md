# Development Plan & Milestone Tracker (plan.md)

> **Last updated**: 2026-07-31 — M4 fetcher implemented behind feature gate; test count updated.

## 1. Executive Roadmap & Status

Overall System Completion: **Phase 1 & Core Phase 2 Complete (62/62 Tests Passing)**. Next phases focus on Frontend Dashboard development, Alert Channels, and Cloudflare Tunnel integration.

```mermaid
gantt
    title Hi5bot Development & Milestone Plan
    dateFormat  YYYY-MM-DD
    section Phase 1: Core System
    SQLite DB & Core Engine       :done, m1, 2026-07-01, 2026-07-15
    Radar & Backtest Simulator   :done, m2, 2026-07-15, 2026-07-25
    Axum REST API & CLI Modes     :done, m3, 2026-07-25, 2026-07-29
    section Phase 2: Live Ingestion & Account Discovery
    AAII/NAAIM Web Scraper        :done, m4, 2026-07-29, 2026-07-30
    Account Discovery (Questrade) :done, m4b, 2026-07-31, 2026-07-31
    Next.js / Recharts Dashboard  :done, m5, 2026-08-01, 2026-08-15
    section Phase 3: Notifications & Ops
    Telegram / Webhook Alerts     :done, m6, 2026-08-15, 2026-08-20
    Cloudflare Tunnel & NAS Ops   :done, m7, 2026-08-20, 2026-08-25
```

---

## 2. Milestone Breakdown & Deliverables

### Milestone 1: Core Daemon & Financial Storage Layer [COMPLETED ✅]
* **Unit M1.1**: Implemented `src/db.rs` SQLite layer (`market_signals`, `order_log`, `backtest_cache` tables) using WAL mode (~290 lines).
* **Unit M1.2**: Questrade OAuth2 refresh token rotation & atomic storage in `src/auth.rs`.
* **Unit M1.3**: Buffer pool integer share allocation flooring and invariant validation in `src/buffer_pool.rs`.

### Milestone 2: Market Extreme Radar & Backtest Laboratory [COMPLETED ✅]
* **Unit M2.1**: Implemented 3-Pillar Market Sentiment & Breadth classification in `src/radar.rs` (~230 lines).
* **Unit M2.2**: Implemented Hi5 vs Hi5e historical simulator calculating CAGR, MaxDD (from NAV), and Sharpe Ratio in `src/backtest.rs` (~530 lines).
* **Unit M2.3**: Built cache layer for storing backtest simulation runs in SQLite.

### Milestone 3: Axum REST API & Multi-Mode CLI [COMPLETED ✅]
* **Unit M3.1**: Implemented Axum Web API with 8 REST endpoints in `src/web.rs` (~290 lines).
* **Unit M3.2**: Extended `Cargo.toml`, `src/main.rs` with `--web-only`, `--once`, `--dry-run` CLI modes.
* **Unit M3.3**: Configured `docker-compose.yml` with port 8080 binding and `HI5BOT_BIND` environment configuration.

---

### Milestone 4: Data Ingestion & Account Discovery [COMPLETED ✅]
* **Unit M4.1**: Implemented `src/fetcher.rs` with `reqwest` + `scraper` (gated behind `web-scraper` Cargo feature) to fetch AAII Sentiment Survey and NAAIM Exposure Index. Returns `None` on scrape failure to avoid false signals.
* **Unit M4.2**: Engine tick (`src/engine.rs`) calls `fetch_market_sentiment()` and persists results to `market_signals` table.
* **Unit M4.3**: Live engine reads `latest_market_signal` from DB and applies `hi5e_dynamic_budget()` zone multiplier.
* **Unit M4.4**: Implemented `src/accounts.rs` — Questrade API-driven account discovery at startup. Filters by `account_types` whitelist, locks in active accounts. Replaced hardcoded `resp_account`/`tfsa_account` fields.
* **Unit M4.5**: Order logging to SQLite on every buy/sell execution with `status: "submitted"`.
* **Unit M4.6**: Implemented `fetch_sp500_breadth()` in `src/fetcher.rs` — S&P 500 % above 200MA Market Breadth fetcher via Yahoo Finance chart API (`^S5TW`).

### Milestone 5: Web Frontend Dashboard (`web/`) [COMPLETED ✅]
* **Unit M5.1**: Initialized Next.js 16 + TypeScript + TailwindCSS + Recharts project in `web/` directory.
* **Unit M5.2**: Built Real-time Market Extreme Radar Gauge & 4-Pillar Status Widget (`RadarWidget.tsx`).
* **Unit M5.3**: Built Portfolio Target vs Actual allocation donut charts & position table (`AllocationChart.tsx`).
* **Unit M5.4**: Built Hi5 vs Hi5e Backtest Comparison interactive area chart (`BacktestChart.tsx`).
* **Unit M5.5**: Built Real-time Audit & Execution Order Log table (`OrderLogTable.tsx`).
* **Unit M5.6**: Added graceful REST API client (`api.ts`) connecting to Axum backend (Port 8080) with fallback UI support.

### Milestone 6: Multi-channel Alerting & Cloudflare Tunnel [COMPLETED ✅]
* **Unit M6.1**: Extended `src/notify.rs` and `src/config.rs` with Telegram Bot (`telegram_bot_token`, `telegram_chat_id`) and Webhook alerts for trade executions, extreme radar escalations, and hard-abort circuit breakers.
* **Unit M6.2**: Configured `cloudflared` Zero-Trust container service in `docker-compose.yml` and multi-stage `Dockerfile` (Rust musl binary + Next.js web standalone bundle) for zero-trust NAS deployment.

---

## 3. Project Roadmap Completion Summary

```
Total Roadmap Milestones: 6 / 6 (100% Completed)
├── ✅ Milestone 1: Core Daemon & Financial Storage Layer
├── ✅ Milestone 2: Market Extreme Radar & Backtest Laboratory
├── ✅ Milestone 3: Axum REST API & Multi-Mode CLI
├── ✅ Milestone 4: Automated Data Ingestion & Account Discovery
├── ✅ Milestone 5: Web Frontend Dashboard (Next.js + Recharts)
└── ✅ Milestone 6: Multi-channel Alerting & Cloudflare Tunnel Ops

Test Suite Verification:
- 62/62 unit & integration tests passing
- Frontend TypeScript build clean (0 errors)
```
