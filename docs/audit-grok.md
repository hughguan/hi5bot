# Docs Deep Audit (Grok Pass) — RESOLVED ARCHIVE

> **Date**: 2026-07-31 (findings) → **2026-07-31 (closed)**  
> **Status**: **ALL ADDRESSED ✅**  
> **Code baseline**: 18 modules, **62/62 tests** (59 lib + 3 integration)  
> **Purpose**: Historical audit record + final resolution matrix. Do not treat open-issue language below the fold as current state.

---

## Final status matrix (current)

| ID | Finding | Resolution |
|----|---------|------------|
| **A1** | Live Hi5e ≠ backtest SGOV reservoir | ✅ Documented in `prd.md`, `spec.md` §1.2–1.4, `arch.md` §4.3, `README.md`. Live = `cash/M × multiplier` only; `sgov_pool` backtest-only. |
| **A2** | Zone string dual-track (`EXTREME_BUY_NOW` vs `EXTREME_PANIC`) | ✅ `ExtremeZone::label()` → `EXTREME_PANIC` (matches serde). Engine keeps legacy `EXTREME_BUY_NOW` fallback for old DB rows. |
| **A3** | Stale root `README.md` | ✅ `account_types`, full module map, Hi5e, test count 62, SGOV live/backtest note. |
| **D1** | `audit.md` dual narrative | ✅ Historical `audit.md` removed; this file is the archived record. |
| **D2** | `extensibility-audit.md` §4 contradiction | ✅ File removed; account discovery is current contract (`account_types` + `accounts.rs`). |
| **D3** | `radar.rs` header comment wrong multipliers | ✅ Normal/Caution documented as 0.5×. |
| **D4** | `multiplier()` vs `hi5e_dynamic_budget` conflict | ✅ Both 0.5/0.5/2/3; aligned. |
| **D5** | Overview API oversold | ✅ Spec/prd/arch/web comments mark **estimated shell** from `order_log`; cash/sgov/pnl placeholders. |
| **D6** | Plan M4 overstated complete | ✅ `plan.md` → PARTIALLY COMPLETE; M4.6 breadth pending. |
| **D7** | Test counts | ✅ 62/62 consistent across README/spec/plan. |
| **D8** | Notifications | ✅ Webhook now; Telegram/Email Phase 3 in plan/prd. |

### Remaining backlog (explicitly out of this audit’s “must fix” set)

| Item | Notes |
|------|--------|
| Live portfolio cache for `/api/overview` | Still estimated from order log |
| Pillar 3 breadth calculator | `plan.md` M4.6 |
| Unify `rebalance_orders` vs backtest `execute_rebalance` | Same-class debt as old M1 fill_the_gap |
| Account re-discovery without restart | Lock-at-startup by design |
| Default-on `web-scraper` feature | Optional; None on failure is intentional |

---

## Document alignment (post-fix)

| Document | Alignment |
|----------|-----------|
| `README.md` | 🟢 |
| `prd.md` | 🟢 |
| `spec.md` | 🟢 |
| `arch.md` | 🟢 |
| `plan.md` | 🟢 (M4 partial called out) |
| `adr.md` | 🟢 |
| `contract-questrade.md` | 🟢 |
| `audit-grok.md` | 🟢 (this archive) |

---

## Original findings (historical — closed)

The sections below are the **original audit narrative** retained for traceability. Every item is closed per the matrix above.

### A1. Live Hi5e ≠ Backtest Hi5e (SGOV pool only in backtest) — CLOSED

| Path | Normal / Caution | Panic / ExtremePanic |
|------|------------------|----------------------|
| **Backtest** | Deploy 50% of monthly contribution; **50% → `sgov_pool`** | Pull from `sgov_pool`; deploy 2× / 3× |
| **Live engine** | Scale `available_per_trade(cash, M)` by **0.5×** | Scale budget by 2× / 3×; **no SGOV unlock** |

### A2. Zone string dual-track — CLOSED

Was: `label()` → `EXTREME_BUY_NOW`, serde → `EXTREME_PANIC`.  
Now: both `EXTREME_PANIC`; legacy string accepted on read.

### A3. Root README drift — CLOSED

Was: `resp_account` / `tfsa_account`.  
Now: `account_types` + discovery modules documented.

### D1–D8 — CLOSED

See final status matrix. Old `audit.md` / `extensibility-audit.md` removed to end dual narratives.

---

## Architecture anchors (current code)

| Concern | Anchor |
|---------|--------|
| Account types | `src/config.rs` `account_types` |
| Discovery | `src/accounts.rs` `discover()` |
| Live Hi5e budget | `src/engine.rs` `hi5e_dynamic_budget(available_per_trade(...))` |
| Backtest SGOV | `src/backtest.rs` `sgov_pool` |
| Zone label | `src/radar.rs` `label()` → `EXTREME_PANIC` |
| fill_the_gap unify | `src/backtest.rs` → `buffer_pool::fill_the_gap` |
| Overview shell | `src/web.rs` `get_overview` (order_log estimates) |
| Docs boundary | `docs/arch.md` §4.3, `docs/spec.md` §1.2–1.4, `README.md` Hi5e note |

---

## One-line verdict (closed)

**audit-grok findings are fully addressed in docs and the related code touchpoints; remaining items are tracked backlog (overview cache, breadth, rebalance unify), not open audit defects.**
