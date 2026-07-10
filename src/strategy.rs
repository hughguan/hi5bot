//! Calendar-month execution state machine (spec §3).
//!
//! Every trading day at 15:30 America/Toronto the daemon evaluates
//! [`evaluate_signal`] against the current [`MarketState`] and the account's
//! [`MonthlyState`] (how many buys have already fired this month). The daemon is
//! capped at 3 incremental buys per month; the signal preconditions encode that
//! cap by gating on `trade_count == 0|1|2`.

use crate::types::{CandleDto, MarketSignal};
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;

/// Signal 1: RSP intra-day return <= -1.0%.
pub const RSP_DAILY_LOW_SLIP: Decimal = Decimal::from_parts(1, 0, 0, true, 2); // -0.01
/// Signal 3: RSP monthly peak-to-trough drawdown <= -5.0%.
pub const RSP_MONTHLY_DRAWDOWN: Decimal = Decimal::from_parts(5, 0, 0, true, 2); // -0.05
/// Signal 4: VIX >= 35.
pub const VIX_PANIC: Decimal = Decimal::from_parts(35, 0, 0, false, 0); // 35
/// Signal 4: RSP intra-day return <= -3.0%.
pub const RSP_DAILY_PANIC: Decimal = Decimal::from_parts(3, 0, 0, true, 2); // -0.03
/// Hard cap on incremental buys per calendar month.
pub const MAX_BUYS_PER_MONTH: u32 = 3;

/// Market observations needed to evaluate the state machine.
#[derive(Clone, Debug)]
pub struct MarketState {
    /// RSP intra-day return (e.g. -0.012 = -1.2%).
    pub rsp_daily_return: Decimal,
    /// RSP month-to-date peak-to-trough drawdown (e.g. -0.06 = -6%).
    pub rsp_monthly_drawdown: Decimal,
    /// VIX index level.
    pub vix: Decimal,
    /// True if today is the 3rd Friday of the month.
    pub is_third_friday: bool,
    /// True if today is the last trading day of August.
    pub is_last_trading_day_of_august: bool,
}

/// Per-account monthly execution state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonthlyState {
    pub year: i32,
    pub month: u32,
    pub trade_count: u32,
}

/// Compute the [`MarketState`] from RSP daily candles + a VIX quote.
///
/// - `rsp_daily_return` = (last close - prev close) / prev close.
/// - `rsp_monthly_drawdown` = the worst (most negative) peak-to-trough close
///   drawdown across the supplied candles.
/// - `is_third_friday` / `is_last_trading_day_of_august` from `today`.
///
/// Pure and panic-free: empty candles yield zero returns; zero prices are
/// guarded against division-by-zero.
pub fn compute_market_state_value(
    rsp_candles: &[CandleDto],
    vix: Option<Decimal>,
    today: NaiveDate,
) -> MarketState {
    let mut daily_return = Decimal::ZERO;
    if rsp_candles.len() >= 2 {
        let last = rsp_candles.last().expect("len >= 2").close;
        let prev = rsp_candles[rsp_candles.len() - 2].close;
        if prev != Decimal::ZERO {
            daily_return = (last - prev) / prev;
        }
    }

    let mut drawdown = Decimal::ZERO;
    let mut high = Decimal::ZERO;
    for c in rsp_candles {
        if c.close > high {
            high = c.close;
        }
        if high != Decimal::ZERO {
            let dd = (c.close - high) / high; // <= 0
            if dd < drawdown {
                drawdown = dd;
            }
        }
    }

    MarketState {
        rsp_daily_return: daily_return,
        rsp_monthly_drawdown: drawdown,
        vix: vix.unwrap_or(Decimal::ZERO),
        is_third_friday: crate::calendar::third_friday(today.year(), today.month()) == today,
        is_last_trading_day_of_august: crate::calendar::last_trading_day_of_august(today.year())
            == today,
    }
}

/// Evaluate the state machine. Returns the signal to act on, if any.
///
/// Order matters: the annual rebalance overrides monthly limits, then the
/// incremental-buy signals are gated by the current `trade_count`.
pub fn evaluate_signal(market: &MarketState, monthly: &MonthlyState) -> Option<MarketSignal> {
    // Signal 5: Annual rebalance (disregards monthly limits).
    if market.is_last_trading_day_of_august {
        return Some(MarketSignal::AnnualRebalance);
    }

    // Signal 1 / 2: Buy #1 (trade_count == 0).
    if monthly.trade_count == 0 {
        if market.rsp_daily_return <= RSP_DAILY_LOW_SLIP {
            return Some(MarketSignal::RegularLowSlip);
        }
        if market.is_third_friday {
            return Some(MarketSignal::GuaranteedThirdFriday);
        }
    }

    // Signal 3: Buy #2 (trade_count == 1).
    if monthly.trade_count == 1 && market.rsp_monthly_drawdown <= RSP_MONTHLY_DRAWDOWN {
        return Some(MarketSignal::DeepRetracement);
    }

    // Signal 4: Buy #3 (trade_count == 2).
    if monthly.trade_count == 2
        && market.vix >= VIX_PANIC
        && market.rsp_daily_return <= RSP_DAILY_PANIC
    {
        return Some(MarketSignal::ExtremePanic);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn dec(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    fn market() -> MarketState {
        MarketState {
            rsp_daily_return: dec("0.0"),
            rsp_monthly_drawdown: dec("0.0"),
            vix: dec("0"),
            is_third_friday: false,
            is_last_trading_day_of_august: false,
        }
    }

    fn monthly(trade_count: u32) -> MonthlyState {
        MonthlyState {
            year: 2026,
            month: 7,
            trade_count,
        }
    }

    #[test]
    fn signal1_regular_low_slip() {
        let mut m = market();
        m.rsp_daily_return = dec("-0.012"); // -1.2%
        assert_eq!(
            evaluate_signal(&m, &monthly(0)),
            Some(MarketSignal::RegularLowSlip)
        );
    }

    #[test]
    fn signal1_boundary_inclusive() {
        let mut m = market();
        m.rsp_daily_return = dec("-0.01"); // exactly -1.0%
        assert_eq!(
            evaluate_signal(&m, &monthly(0)),
            Some(MarketSignal::RegularLowSlip)
        );
    }

    #[test]
    fn signal2_third_friday_fallback() {
        let mut m = market();
        m.is_third_friday = true;
        // No daily drop, but it's the 3rd Friday and no trade yet.
        assert_eq!(
            evaluate_signal(&m, &monthly(0)),
            Some(MarketSignal::GuaranteedThirdFriday)
        );
    }

    #[test]
    fn signal1_takes_precedence_over_signal2() {
        let mut m = market();
        m.rsp_daily_return = dec("-0.02");
        m.is_third_friday = true;
        assert_eq!(
            evaluate_signal(&m, &monthly(0)),
            Some(MarketSignal::RegularLowSlip)
        );
    }

    #[test]
    fn no_buy1_if_already_traded() {
        let mut m = market();
        m.rsp_daily_return = dec("-0.05");
        m.is_third_friday = true;
        // trade_count == 1: buy #1 signals must not re-fire.
        assert_eq!(evaluate_signal(&m, &monthly(1)), None);
    }

    #[test]
    fn signal3_deep_retracement() {
        let mut m = market();
        m.rsp_monthly_drawdown = dec("-0.06"); // -6%
        assert_eq!(
            evaluate_signal(&m, &monthly(1)),
            Some(MarketSignal::DeepRetracement)
        );
    }

    #[test]
    fn signal4_extreme_panic() {
        let mut m = market();
        m.vix = dec("40");
        m.rsp_daily_return = dec("-0.035"); // -3.5%
        assert_eq!(
            evaluate_signal(&m, &monthly(2)),
            Some(MarketSignal::ExtremePanic)
        );
    }

    #[test]
    fn signal4_requires_both_vix_and_rsp() {
        let mut m = market();
        m.vix = dec("40");
        m.rsp_daily_return = dec("-0.02"); // not <= -3%
        assert_eq!(evaluate_signal(&m, &monthly(2)), None);
    }

    #[test]
    fn signal5_annual_rebalance_overrides() {
        let mut m = market();
        m.is_last_trading_day_of_august = true;
        // Even with trade_count == 3, the annual rebalance fires.
        assert_eq!(
            evaluate_signal(&m, &monthly(3)),
            Some(MarketSignal::AnnualRebalance)
        );
    }

    #[test]
    fn no_signal_when_conditions_unmet() {
        let m = market();
        assert_eq!(evaluate_signal(&m, &monthly(0)), None);
    }
}
