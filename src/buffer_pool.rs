//! Dynamic Buffer Pool Algorithm (spec §4).
//!
//! Two responsibilities:
//!
//! 1. [`available_per_trade`] - size each incremental buy as `Current_USD_Cash
//!    / M`, where `M` is the rolling month safety buffer (default 10). This
//!    eliminates "cash exhaustion": whether 1 or 3 buys fire in a month, each
//!    only ever commits `1/M` of the settled USD cash.
//!
//! 2. [`fill_the_gap`] - "Fill-the-Gap" allocation filter. Spend the budget on
//!    the most-underweight tickers first (worst laggard first), flooring every
//!    allocation to whole integer shares and never pushing a ticker above its
//!    20% target. Deterministic and panic-free on any balance shape.
//!
//! ## Guarantees (proven by tests)
//!
//! - Every emitted share count is a whole integer (floored).
//! - `Σ est_cost ≤ budget` (exact Decimal; never overspends).
//! - No ticker is pushed above its 20% target.
//! - Never panics: zero/negative cash, zero total value, zero prices, and
//!   wildly asymmetric balances all yield a (possibly empty) order list.

use crate::error::Result;
use crate::types::*;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

/// `Available_Per_Trade = Current_USD_Cash / M`.
///
/// Returns [`UsdCash::ZERO`] for non-positive cash so the allocator never
/// receives a negative budget. `m == 0` falls back to [`SAFETY_BUFFER_M`].
pub fn available_per_trade(cash: UsdCash, m: u32) -> UsdCash {
    if cash.0 <= Decimal::ZERO {
        return UsdCash::ZERO;
    }
    let divisor = Decimal::from(if m == 0 { SAFETY_BUFFER_M } else { m });
    UsdCash(cash.0 / divisor)
}

struct Laggard {
    ticker: Ticker,
    gap: Decimal,
    price: UsdPrice,
    current_mv: UsdCash,
    target_mv: UsdCash,
}

/// Fill-the-Gap allocation: spend `budget` on underweight tickers, worst-first.
///
/// `state.positions` should carry an entry for every portfolio ticker (the
/// engine zero-fills unheld ones) so each has a current quote price. Tickers
/// with no usable price (`<= 0`) are skipped.
pub fn fill_the_gap(state: &PortfolioState, budget: UsdCash) -> Result<Vec<AllocationOrder>> {
    let total = state.total_value();
    // No computable weights, or nothing to spend -> nothing to do. No panic.
    if total.0 <= Decimal::ZERO || budget.0 <= Decimal::ZERO {
        return Ok(Vec::new());
    }

    // 1. Compute the gap (target - current weight) for each ticker with a price.
    let mut laggards: Vec<Laggard> = Vec::new();
    for meta in PORTFOLIO {
        let (shares, price) = match state.position(meta.ticker) {
            Some(p) => (p.shares, p.price),
            None => (Shares::ZERO, UsdPrice::ZERO),
        };
        if price.0 <= Decimal::ZERO {
            // Can't size a purchase without a price; skip cleanly.
            continue;
        }
        let current_mv = price * shares;
        let current_weight = current_mv.0 / total.0;
        let gap = meta.target_weight - current_weight;
        if gap <= Decimal::ZERO {
            continue; // at or above target; not a laggard
        }
        let target_mv = UsdCash(total.0 * meta.target_weight);
        laggards.push(Laggard {
            ticker: meta.ticker,
            gap,
            price,
            current_mv,
            target_mv,
        });
    }

    // 2. Worst laggard first. `sort_by_key` is stable, so equal gaps keep
    //    PORTFOLIO order (deterministic). Reverse => descending gap.
    laggards.sort_by_key(|l| std::cmp::Reverse(l.gap));

    // 3. Greedy: pour whole (floored) shares into each laggard up to its target,
    //    never exceeding the remaining budget.
    let mut budget_remaining = budget;
    let mut orders = Vec::new();
    for lag in laggards {
        // Shares needed to reach the 20% target (floored down -> never overshoots).
        let need_to_target = ((lag.target_mv.0 - lag.current_mv.0) / lag.price.0).floor();
        let need_to_target = if need_to_target < Decimal::ZERO {
            Decimal::ZERO
        } else {
            need_to_target
        };
        // Shares affordable with the remaining budget (floored down).
        let affordable = (budget_remaining.0 / lag.price.0).floor();
        if affordable <= Decimal::ZERO {
            continue; // can't afford even one share of this laggard; try the next
        }

        let buy = affordable.min(need_to_target);
        let buy = buy.to_u64().unwrap_or(0);
        if buy == 0 {
            continue;
        }

        let shares = Shares(buy);
        let cost = lag.price * shares;

        // Floor guarantees cost <= budget; guard against any Decimal rounding
        // edge by dropping one share if (impossibly) over.
        let (shares, cost) = if cost.0 > budget_remaining.0 && buy > 0 {
            let s = Shares(buy - 1);
            (s, lag.price * s)
        } else {
            (shares, cost)
        };
        if shares.0 == 0 {
            continue;
        }

        orders.push(AllocationOrder {
            ticker: lag.ticker,
            shares,
            est_cost: cost,
            limit_price: lag.price,
        });
        budget_remaining -= cost;
    }

    Ok(orders)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn dec(s: &str) -> Decimal {
        s.parse().unwrap()
    }
    fn price(s: &str) -> UsdPrice {
        UsdPrice(dec(s))
    }
    fn cash(s: &str) -> UsdCash {
        UsdCash(dec(s))
    }
    fn pos(ticker: Ticker, shares: u64, px: &str) -> Position {
        Position {
            ticker,
            shares: Shares(shares),
            price: price(px),
        }
    }

    /// Build a state with all 5 tickers present (zero shares where unheld).
    fn state(cash_amt: &str, specs: [(Ticker, u64, &str); 5]) -> PortfolioState {
        PortfolioState {
            cash_usd: cash(cash_amt),
            positions: specs.map(|(t, s, p)| pos(t, s, p)).to_vec(),
        }
    }

    const P: [(Ticker, u64, &str); 5] = [
        (Ticker::Iwy, 0, "120"),
        (Ticker::Spmo, 0, "150"),
        (Ticker::Rsp, 0, "180"),
        (Ticker::Pff, 0, "28"),
        (Ticker::Vnq, 0, "90"),
    ];

    // ---- available_per_trade -------------------------------------------------

    #[test]
    fn available_per_trade_divides_by_m() {
        assert_eq!(available_per_trade(cash("1000"), 10), cash("100"));
        assert_eq!(available_per_trade(cash("333"), 10), cash("33.3"));
    }

    #[test]
    fn available_per_trade_zero_and_negative_cash_returns_zero() {
        assert_eq!(available_per_trade(cash("0"), 10), UsdCash::ZERO);
        assert_eq!(available_per_trade(cash("-50"), 10), UsdCash::ZERO);
    }

    #[test]
    fn available_per_trade_m_zero_falls_back_to_default() {
        assert_eq!(available_per_trade(cash("1000"), 0), cash("100"));
    }

    // ---- fill_the_gap invariants --------------------------------------------

    #[test]
    fn floors_to_whole_shares() {
        // Budget 1000, single cheap laggard at 28 -> floor(1000/28)=35 shares, cost 980.
        let st = state("1000", P);
        let budget = available_per_trade(cash("1000"), 1); // budget = 1000
        let orders = fill_the_gap(&st, budget).unwrap();
        // PFF is the only laggard everything else is 0% too actually all are 0%
        // so worst-first tie -> PORTFOLIO order; cheapest among them still buys.
        for o in &orders {
            // whole shares
            assert_eq!(Decimal::from(o.shares.0) * o.limit_price.0, o.est_cost.0);
            assert!(o.est_cost.0 <= budget.0);
        }
    }

    #[test]
    fn never_exceeds_budget() {
        let st = state("5000", P);
        let budget = available_per_trade(st.cash_usd, 10); // 500
        let orders = fill_the_gap(&st, budget).unwrap();
        let total_cost: Decimal = orders.iter().map(|o| o.est_cost.0).sum();
        assert!(
            total_cost <= budget.0,
            "total {total_cost} > budget {}",
            budget.0
        );
        assert!(total_cost >= Decimal::ZERO);
    }

    #[test]
    fn laggards_prioritized_worst_first() {
        // Make RSP the most underweight (huge target gap) and PFF second.
        // Give IWY/SPMO/VNQ enough to be ~at target so they're excluded.
        // total = cash + positions; craft so RSP gap > PFF gap.
        let specs = [
            (Ticker::Iwy, 80, "120"),  // 9600
            (Ticker::Spmo, 64, "150"), // 9600
            (Ticker::Rsp, 0, "180"),   // 0   <- worst laggard
            (Ticker::Pff, 100, "28"),  // 2800
            (Ticker::Vnq, 0, "90"),    // 0   <- second laggard
        ];
        // total positions = 9600+9600+0+2800+0 = 22000; cash 5000 -> total 27000.
        // target_mv each = 5400. RSP gap = 5400 (0 -> 5400). PFF gap = 5400-2800=2600.
        // VNQ gap = 5400. RSP and VNQ tie (both 5400) -> RSP first (PORTFOLIO order).
        let st = state("5000", specs);
        let budget = cash("5000");
        let orders = fill_the_gap(&st, budget).unwrap();
        assert!(!orders.is_empty());
        // First order must be RSP (worst laggard, ties broken by portfolio order).
        assert_eq!(
            orders[0].ticker,
            Ticker::Rsp,
            "worst laggard first: {:?}",
            orders
        );
        // No order for the at-target tickers.
        assert!(orders.iter().all(|o| o.ticker != Ticker::Iwy));
        assert!(orders.iter().all(|o| o.ticker != Ticker::Spmo));
    }

    #[test]
    fn never_overshoots_target() {
        // Huge budget, one fully-underweight ticker: must cap at the target,
        // never buy more than needed to reach 20%.
        let specs = [
            (Ticker::Iwy, 100, "120"),  // 12000
            (Ticker::Spmo, 100, "150"), // 15000
            (Ticker::Rsp, 100, "180"),  // 18000
            (Ticker::Pff, 100, "28"),   // 2800
            (Ticker::Vnq, 0, "90"),     // 0  <- only laggard
        ];
        // total = 12000+15000+18000+2800+0 + cash 100000 = 147800
        // target_mv VNQ = 29560 -> shares to target = floor(29560/90) = 328
        let st = state("100000", specs);
        let total = st.total_value().0;
        let budget = cash("100000"); // plenty
        let orders = fill_the_gap(&st, budget).unwrap();
        let vnq = orders.iter().find(|o| o.ticker == Ticker::Vnq).unwrap();
        // post-buy VNQ market value must not exceed target (within one share).
        let post_mv = Decimal::from(vnq.shares.0) * dec("90");
        let target_mv = total * TARGET_WEIGHT;
        assert!(
            post_mv <= target_mv,
            "VNQ post-buy {post_mv} overshoots target {target_mv}"
        );
        // and at most one share below target (floor)
        assert!(post_mv + dec("90") >= target_mv || vnq.shares.0 == 0);
    }

    #[test]
    fn asymmetric_balances_no_panic() {
        // One ticker massively overweight (80%+), others tiny. Must not panic
        // and must only buy laggards (never the overweight one).
        let specs = [
            (Ticker::Iwy, 1000, "120"), // 120000 (~80%)
            (Ticker::Spmo, 5, "150"),
            (Ticker::Rsp, 5, "180"),
            (Ticker::Pff, 5, "28"),
            (Ticker::Vnq, 5, "90"),
        ];
        let st = state("2000", specs);
        let budget = available_per_trade(st.cash_usd, 10);
        let orders = std::panic::catch_unwind(|| fill_the_gap(&st, budget).unwrap());
        assert!(orders.is_ok(), "must not panic on asymmetric balances");
        let orders = orders.unwrap();
        // IWY is overweight -> never bought.
        assert!(orders.iter().all(|o| o.ticker != Ticker::Iwy));
        for o in &orders {
            assert_eq!(Decimal::from(o.shares.0) * o.limit_price.0, o.est_cost.0);
        }
    }

    #[test]
    fn zero_cash_no_panic() {
        let st = state("0", P);
        let budget = available_per_trade(st.cash_usd, 10);
        assert_eq!(budget, UsdCash::ZERO);
        let orders = fill_the_gap(&st, budget).unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn zero_total_value_no_panic() {
        // No cash, no positions -> total 0 -> no weights.
        let st = PortfolioState {
            cash_usd: UsdCash::ZERO,
            positions: P.map(|(t, _, p)| pos(t, 0, p)).to_vec(),
        };
        let orders = fill_the_gap(&st, cash("0")).unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn all_at_target_no_orders() {
        // Each ticker exactly 20%: cash 0, 5 equal market values.
        // 5 * (n * price) = total, each = total/5 = 20%.
        let specs = [
            (Ticker::Iwy, 90, "120"),  // 10800
            (Ticker::Spmo, 72, "150"), // 10800
            (Ticker::Rsp, 60, "180"),  // 10800
            (Ticker::Pff, 385, "28"),  // 10780  (floor; ~20%)
            (Ticker::Vnq, 120, "90"),  // 10800
        ];
        let st = state("0", specs);
        let budget = cash("1000");
        let orders = fill_the_gap(&st, budget).unwrap();
        // PFF is marginally under target (10780 vs 10800) so it may get 0 or 1 share;
        // the rest must get nothing.
        let buyers: Vec<_> = orders.iter().map(|o| o.ticker).collect();
        assert!(
            buyers.iter().all(|t| *t == Ticker::Pff),
            "only the marginally-underweight ticker may buy: {buyers:?}"
        );
    }

    #[test]
    fn decimal_precision_no_drift() {
        // cost == shares * price exactly for every order (no float bleeding).
        let st = state("7777", P);
        let budget = available_per_trade(st.cash_usd, 10);
        let orders = fill_the_gap(&st, budget).unwrap();
        for o in &orders {
            assert_eq!(o.est_cost.0, Decimal::from(o.shares.0) * o.limit_price.0);
        }
    }
}
