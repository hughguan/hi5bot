# Architecture Decision Records (adr.md)

## Record Summary

| ADR ID | Title | Status | Date |
| :--- | :--- | :--- | :--- |
| **ADR-001** | Adoption of Rust (Tokio + Axum) for Unified Binary Architecture | Accepted | 2026-07-29 |
| **ADR-002** | SQLite with Write-Ahead Logging (WAL) Mode for Local Persistence | Accepted | 2026-07-29 |
| **ADR-003** | Use of `rust_decimal::Decimal` for Financial Calculations | Accepted | 2026-07-29 |
| **ADR-004** | 3-Pillar Sentiment & Breadth Classification for Dynamic Cash Reservoir | Accepted | 2026-07-29 |
| **ADR-005** | Restricting Questrade Orders to Limit Orders on USD Settled Cash | Accepted | 2026-07-29 |
| **ADR-006** | Questrade API-Driven Account Discovery (No Hardcoded Account Numbers) | Accepted | 2026-07-31 |

---

## ADR-001: Adoption of Rust (Tokio + Axum) for Unified Binary Architecture

### Context
The Hi5bot system needs to run continuously 24/7 on local NAS hardware (Synology / Unraid / QNAP) with low system resource availability, serving both background cron execution tasks and a web REST API.

### Decision
We choose Rust with `tokio` (async runtime) and `axum` (web framework) to build a single static binary (~4.2MB).

### Consequences
* **Positive**: Extremely low memory consumption (~20MB RAM), zero runtime overhead, high execution speed, single binary deployment inside Docker.
* **Negative**: Requires strict compile-time safety and type management compared to dynamic scripting languages.

---

## ADR-002: SQLite with Write-Ahead Logging (WAL) Mode for Local Persistence

### Context
The system requires persistent storage for market signals, order logs, and backtest results. Both the trading daemon (writer) and the Axum API server (reader) need concurrent database access.

### Decision
Use SQLite (`rusqlite`) configured with `PRAGMA journal_mode=WAL;`.

### Consequences
* **Positive**: Thread-safe concurrent non-blocking reads during background writes. Zero external database daemon dependencies; data stored as a single file on NAS storage for easy backup.
* **Negative**: SQLite requires direct local volume mounting in Docker containers.

---

## ADR-003: Use of `rust_decimal::Decimal` for Financial Calculations

### Context
Floating-point arithmetic (`f34`, `f64`) causes precision loss and cumulative rounding errors in financial transactions and share quantity calculations.

### Decision
Enforce `rust_decimal::Decimal` across all portfolio models, trading signals, buffer pool math, and order execution logic.

### Consequences
* **Positive**: Exact 128-bit fixed-point precision with zero rounding ambiguity in cash allocation and share flooring calculations.
* **Negative**: Requires explicit conversions when interfacing with JSON APIs.

---

## ADR-004: 3-Pillar Sentiment & Breadth Classification for Dynamic Cash Reservoir

### Context
Standard dollar-cost averaging (Hi5 Base) allocates cash uniformly regardless of market conditions. Market panic offers superior risk-adjusted buying opportunities.

### Decision
Introduce Hi5e Market Radar based on 3 pillars:
1. AAII Sentiment (Bears $\ge 55\%$ or Bulls $\le 25\%$)
2. NAAIM Exposure Index ($\le 40\%$)
3. S&P 500 200-day MA Market Breadth ($\le 30\%$)

Dynamic deployment multipliers: `Normal` ($0.5\times$), `Caution` ($0.5\times$), `Panic` ($2.0\times$), `ExtremePanic` ($3.0\times$).

### Consequences
* **Positive**: Reserves cash in SGOV during normal/overbought markets and unleashes $2\times \sim 3\times$ buying power during extreme market drawdowns.
* **Negative**: Requires reliable upstream market data fetching.

---

## ADR-005: Restricting Questrade Orders to Limit Orders on USD Settled Cash

### Context
Brokerage accounts on Questrade can incur high margin interest fees if Canadian Dollar (CAD) funds are inadvertently leveraged to buy US Dollar (USD) ETFs.

### Decision
Implement a USD Hard Lock Circuit Breaker. Check available USD settled cash before order submission and place strictly Limit Orders at the current Bid price.

### Consequences
* **Positive**: Absolute prevention of unintended CAD margin borrowing or currency auto-conversion fees.
* **Negative**: Limit orders at Bid may occasionally fail to fill if prices move rapidly upward, requiring fallback alerts.

---

## ADR-006: Questrade API-Driven Account Discovery (No Hardcoded Account Numbers)

### Context
The original design hardcoded two account numbers (`resp_account`, `tfsa_account`) in `config.toml`. Users with RRSP or Margin accounts had to modify Rust code to add support. Questrade's `GET /v1/accounts` API returns all accounts with their types, numbers, and statuses.

### Decision
Replace hardcoded account fields with a dynamic discovery flow:
1. `config.toml` declares only `account_types = ["RESP", "TFSA"]` (type whitelist).
2. At startup, the daemon calls `GET /v1/accounts`, filters by `status == "Active"` and `kind` in the whitelist.
3. Discovered accounts are sorted (primary first) and locked in for the process lifetime.
4. If no matching accounts are found, the daemon exits with a clear error.

### Consequences
* **Positive**: Supports any combination of RESP, TFSA, RRSP, Margin, Cash accounts with zero code changes. Multi-account families need only edit one TOML line.
* **Positive**: Account numbers are never stored in config — they come from Questrade, eliminating manual entry errors.
* **Negative**: Adds a startup dependency on the Questrade API. If Questrade is unreachable at boot, the daemon cannot start. (Mitigation: the existing token refresh retry logic applies.)
