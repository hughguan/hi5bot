# Hi5-Bot

Unattended, mission-critical automation daemon (Rust) that executes a strict,
mechanical asset-allocation and grid rebalancing strategy on Questrade
registered accounts (e.g. RESP, TFSA, RRSP). A compile-time state machine translates
emotional investing rules into deterministic execution, eliminating human bias.

> ⚠️ This software places **real orders** with real money once configured with
> live Questrade credentials. Read the [Safety model](#safety-model) and run
> `--once --dry-run` first.

---

## The "Hi5" strategy

Five U.S.-listed ETFs, equal-weighted at 20.0% each:

| Ticker | Asset class | Target | Role |
|--------|-------------|--------|------|
| IWY  | Russell Top 200 Growth        | 20% | Core Growth (Offense) |
| SPMO | S&P 500 Momentum              | 20% | Trend/Momentum (Offense) |
| RSP  | S&P 500 Equal Weight          | 20% | Market Anchor / Signal Trigger (Defense) |
| PFF  | Preferred & Income Securities | 20% | High-Yield Cash (Logistics) |
| VNQ  | Real Estate                   | 20% | Tangible Yield (Logistics) |

The daemon wakes every trading day at **15:30 America/Toronto** (30 min before
US close) and evaluates a 5-signal state machine capped at **3 incremental buys
per calendar month**:

1. **Regular Low Slip** — RSP daily return ≤ −1.0% (buy #1)
2. **Guaranteed Third Friday** — calendar fallback if #1 didn't fire (buy #1)
3. **Deep Retracement** — RSP month drawdown ≤ −5.0% (buy #2)
4. **Extreme Panic** ("Humanity's Limit") — VIX ≥ 35 **and** RSP daily ≤ −3.0% (buy #3)
5. **Annual Rebalance** — last trading day of August: restore all 5 to 20% (buy+sell, ignores monthly cap)

Each buy is sized by the **Dynamic Buffer Pool** and allocated **Fill-the-Gap**
(worst-underweight ticker first, floored to whole shares, never above target).
The enhanced **Hi5e strategy** scales allocation dynamically based on the **Market Extreme Zone Radar** ($0.5\times$ in Normal/Caution, $2.0\times$ in Panic, $3.0\times$ in ExtremePanic).

> **Live vs backtest:** In production the multiplier applies to the per-tick buffer budget (`USD cash / M`). An **SGOV cash reservoir** is simulated **only in the backtest engine** (`sgov_pool`); the live daemon does not hold or unlock a separate SGOV pool. See `docs/arch.md` §4.3 and `docs/spec.md` §1.2–1.4.

---

## Safety model

These are guardrails, not aspirations — most are enforced at compile time or as
hard-aborts.

- **No floats.** Every monetary value, price, and share count uses
  `rust_decimal::Decimal` or whole-share `Shares(u64)`. No IEEE 754 bleeding.
- **Currency hard-lock (compile-time firewall).** `UsdCash`, `CadCash`, and
  `UsdPrice` are distinct newtypes — adding CAD to USD is a *compile error*.
  The daemon can never spend CAD credit on a USD trade (avoids Questrade's
  1.5%+ auto-forex spread). The settlement preference must be
  *"Currency of Transaction"* or the daemon hard-aborts before trading.
- **Zero external pull-in.** The Questrade client has **no** banking/EFT/PAD
  capability. It can only read account data and place Day Limit orders — it can
  never initiate a pull from a connected bank account, eliminating accidental
  CRA over-contribution.
- **USD cash hard-abort.** Before sizing an incremental buy, if USD cash ≤ 0
  the daemon notifies (webhook) and exits non-zero. It never places a USD trade
  against CAD credit.
- **Limit-at-bid only.** All orders are Day Limit orders at the bid, eliminating
  Questrade ECN fees.
- **Crash-safe tokens.** Questrade rotates the refresh_token on every exchange.
  `tokens.json` is rewritten atomically (temp → fsync → rename, 0600, with a
  `.bak`), so a crash or bad response can never corrupt or lose a token.
- **Max 3 buys/month.** Enforced by the state machine's `trade_count` gating +
  a per-account atomic counter (`state.json`).

---

## Architecture

```
src/
  types.rs        Domain types, Decimal money newtypes, Questrade DTOs
  error.rs        Typed errors (thiserror)
  config.rs       TOML settings + $HI5BOT_DATA_DIR resolution
  auth.rs         OAuth2 token store: atomic rewrite + refresh handshake
  accounts.rs     Dynamic Questrade account discovery at startup
  buffer_pool.rs  Dynamic Buffer Pool sizing + Fill-the-Gap allocation
  strategy.rs     Calendar-month state machine (5 signals) + market-state compute
  calendar.rs     Third-Friday / last-Aug-trading-day / 15:30 ET wake
  questrade.rs    Async REST client (accounts/balances/positions/quotes/candles/orders)
  state_store.rs  Atomic per-account monthly trade counter
  fetcher.rs      AAII & NAAIM web sentiment fetcher pipeline
  radar.rs        Market Extreme Zone Radar classification engine
  backtest.rs     Hi5 vs Hi5e historical backtest simulator
  web.rs          Axum REST API server (Port 8080) for dashboard UI
  db.rs           SQLite database layer (WAL mode, market_signals, order_log, backtest_cache)
  notify.rs       Multi-channel alerts (Generic Webhook + Telegram Bot)
  engine.rs       One evaluation tick: auth → discovery → radar → signal → allocate → order
  main.rs         Tokio runtime + background web server + cron loop (--once / --dry-run / --web-only)
```

The data directory (`$HI5BOT_DATA_DIR`, default `./data`) holds `config.toml`,
`tokens.json`, `hi5bot.db`, and `state.json` — one volume to mount in a container.

---

## Build & test

```sh
cargo build
cargo test            # 59 lib + 3 integration tests (62 total)
cargo test -- --nocapture
```

The headline integration test (`tests/buffer_pool.rs`) parses **mock Questrade
JSON** (balances + positions + quotes) and proves the buffer pool floors share
targets, never exceeds budget, never overshoots 20%, and does not panic on
asymmetric or zero balances.

---

## Configuration

Copy `config/hi5bot.example.toml` to `data/config.toml` and fill in:

- `account_types` — Whitelist of active Questrade account kinds (e.g. `["RESP", "TFSA", "RRSP", "Margin"]`). Accounts are dynamically discovered at startup.
- `[symbol_ids]` — Questrade symbol IDs for the 5 ETFs (lookup once via
  `v1/symbols/search?q=<TICKER>`).
- `vix_symbol_id` — VIX index symbol ID (for ExtremePanic signal).
- `safety_buffer_m` — `M` in `Available_Per_Trade = USD_Cash / M` (default 10).
- `settlement_pref` — must stay `"Currency of Transaction"` (hard-lock).
- `token_url`, `eval_time`, `notify_webhook`.

### Bootstrap `tokens.json`

Obtain an initial `refresh_token` from Questrade's OAuth consent (practice or
prod), then create `data/tokens.json` (chmod 0600):

```json
{
  "access_token": "INITIAL",
  "refresh_token": "<your-refresh-token>",
  "api_server": "https://api06.iq.questrade.com/",
  "token_type": "Bearer",
  "expires_in": 1,
  "acquired_at": "2026-01-01T00:00:00",
  "expires_at": "2026-01-01T00:00:00"
}
```

On first run `expires_at` is in the past, so the daemon immediately exchanges
the refresh_token for a live access_token and rewrites the file atomically
(rotating the refresh_token).

---

## Local run

```sh
HI5BOT_DATA_DIR=./data cargo run -- --once --dry-run   # compute, place no orders
HI5BOT_DATA_DIR=./data cargo run -- --once             # single tick, real orders
HI5BOT_DATA_DIR=./data cargo run --                     # long-running cron loop
```

---

## Deploy to Synology DS218+

The DS218+ CPU (Intel Celeron J3355) is **64-bit x86_64**; DSM is Linux. Target:
`x86_64-unknown-linux-musl` (static, so no DSM glibc coupling). Verify on the NAS
with `uname -m` (expect `x86_64`).

The daemon self-schedules, so it runs as a long-lived container under Container
Manager (DSM 7.2+) with `restart: unless-stopped`.

### Path A — Build the image on the Mac, transfer to the NAS (no registry)

```sh
# On the Mac (Docker Desktop with buildx / QEMU for linux/amd64):
docker buildx build --platform linux/amd64 -t hi5bot:latest \
  -o type=docker,dest=hi5bot.tar .

# Copy to the NAS:
scp hi5bot.tar docker-compose.yml <nas>:/volume1/docker/hi5bot/

# On the NAS:
cd /volume1/docker/hi5bot
docker load -i hi5bot.tar
mkdir -p data
# place data/config.toml and data/tokens.json (chmod 600) here
docker compose up -d
docker logs -f hi5bot
```

### Path B — Build directly on the NAS

Container Manager can build the multi-stage `Dockerfile` on-device (slower; the
J3355 compiles the Rust crate in a few minutes):

```sh
cd /volume1/docker/hi5bot
docker compose up -d --build
```

### Path C — Prebuilt static binary (no Docker on the Mac)

Cross-compile a static musl binary with `cargo-zigbuild`, then build a thin
image on the NAS:

```sh
# On the Mac:
brew install zig
cargo install cargo-zigbuild
rustup target add x86_64-unknown-linux-musl
cargo zigbuild --release --target x86_64-unknown-linux-musl
# -> target/x86_64-unknown-linux-musl/release/hi5bot (statically linked)
scp target/x86_64-unknown-linux-musl/release/hi5bot <nas>:/volume1/docker/hi5bot/
```

### Bootstrap checklist on the NAS

1. `uname -m` → `x86_64`.
2. Create `/volume1/docker/hi5bot/data/`.
3. Place `config.toml` (filled in) and `tokens.json` (chmod 0600) in `data/`.
4. `docker compose up -d` (or `--build` for Path B).
5. Confirm with `docker compose ps` and `docker logs -f hi5bot`.
6. First run with `RUST_LOG=debug` to watch the token refresh + a `--dry-run`
   tick (set the binary entrypoint args via compose `command:` if desired).

### Updating

Rebuild the image and `docker compose up -d`. The `./data` volume preserves
`tokens.json`, `state.json`, and `config.toml` across updates — tokens are
never baked into the image.

---

## Disclaimer

Educational/reference implementation. Not financial advice. Test thoroughly on
a Questrade **practice** account before pointing at live registered accounts.
