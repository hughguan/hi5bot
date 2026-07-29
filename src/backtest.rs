//! Hi5 vs Hi5e Backtest Engine.
//!
//! Simulates two strategies over a user-specified historical period and
//! produces JSON-serializable results for the dashboard comparison chart.
//!
//! ## Strategies
//!
//! **Hi5 (Baseline)** — The original mechanical strategy:
//! - Monthly contribution of ≈$1,000 on the 3rd Friday
//! - Fill-the-Gap allocation: 100% to the most-underweight ticker
//! - Annual rebalance every last trading day of August
//! - No market-timing; purely calendar-driven
//!
//! **Hi5e (Dynamic)** — Extreme-zone enhanced:
//! - In Normal zone: only 50% deployed ($500), 50% reserved in SGOV pool
//! - In Panic zone: 2× deployment ($2,000), pulling from SGOV reserve
//! - In ExtremePanic zone: 3× deployment ($3,000)
//! - August rebalance deferred by 15 trading days if in a panic zone
//! - Uses the [`crate::radar::classify_zone`] classification on each trade date
//!
//! ## Output
//!
//! Both strategies produce a NAV series (net asset value over time) plus
//! summary statistics: CAGR, Max Drawdown, Sharpe Ratio.
//!
//! ## Precision
//!
//! All internal calculations use [`rust_decimal::Decimal`]; final outputs are
//! converted to f64 for JSON serialization.

use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::radar::{ExtremeZone, classify_zone};
use crate::types::{Position, PortfolioState, Shares, TARGET_WEIGHT, Ticker, UsdCash, UsdPrice};

// ---- Input types --------------------------------------------------------

/// One day of price data for all 5 ETFs + optional market indicators.
#[derive(Clone, Debug, Deserialize)]
pub struct BacktestDay {
    pub date: NaiveDate,
    pub iwy: f64,
    pub spmo: f64,
    pub rsp: f64,
    pub pff: f64,
    pub vnq: f64,
    /// Optional: AAII Bulls survey (%)
    pub aaii_bulls: Option<f64>,
    /// Optional: AAII Bears survey (%)
    pub aaii_bears: Option<f64>,
    /// Optional: NAAIM Exposure (%)
    pub naaim_exposure: Option<f64>,
    /// Optional: % S&P 500 stocks above 200-day MA
    pub sp500_pct_above_200ma: Option<f64>,
    /// Optional: VIX level
    pub vix: Option<f64>,
}

/// The backtest request from the web dashboard.
#[derive(Clone, Debug, Deserialize)]
pub struct BacktestRequest {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub monthly_contribution: Option<f64>, // default $1,000
    pub safety_buffer_m: Option<u32>,       // default 10
}

// ---- Output types -------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct BacktestResult {
    pub request: BacktestRequestSummary,
    pub hi5: StrategyResult,
    pub hi5e: StrategyResult,
    /// Daily NAV points, aligned by date, for chart rendering.
    pub nav_series: Vec<NavPoint>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BacktestRequestSummary {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub monthly_contribution: f64,
    pub safety_buffer_m: u32,
    pub trading_days: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct StrategyResult {
    pub strategy: &'static str,
    pub final_nav: f64,
    pub total_contributions: f64,
    pub total_return_pct: f64,
    pub cagr_pct: f64,
    pub max_drawdown_pct: f64,
    pub sharpe_ratio: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct NavPoint {
    pub date: NaiveDate,
    pub hi5_nav: f64,
    pub hi5_e_nav: f64,
}

// ---- Engine state -------------------------------------------------------

#[derive(Clone)]
struct PortfolioSnapshot {
    shares: [u64; 5],
    cash: Decimal,
    sgov_pool: Decimal, // Hi5e only: cash reserve
}

impl PortfolioSnapshot {
    fn new(initial_cash: Decimal) -> Self {
        PortfolioSnapshot {
            shares: [0; 5],
            cash: initial_cash,
            sgov_pool: Decimal::ZERO,
        }
    }

    fn total_value(&self, prices: &[Decimal; 5]) -> Decimal {
        let mut tv = self.cash + self.sgov_pool;
        for i in 0..5 {
            tv += Decimal::from(self.shares[i]) * prices[i];
        }
        tv
    }
}

// ---- Price helpers ------------------------------------------------------

fn prices_from_day(day: &BacktestDay) -> [Decimal; 5] {
    [
        Decimal::from_f64_retain(day.iwy).unwrap_or(Decimal::ZERO),
        Decimal::from_f64_retain(day.spmo).unwrap_or(Decimal::ZERO),
        Decimal::from_f64_retain(day.rsp).unwrap_or(Decimal::ZERO),
        Decimal::from_f64_retain(day.pff).unwrap_or(Decimal::ZERO),
        Decimal::from_f64_retain(day.vnq).unwrap_or(Decimal::ZERO),
    ]
}

/// True if this date is the 3rd Friday of its month.
fn is_third_friday(d: NaiveDate) -> bool {
    crate::calendar::third_friday(d.year() as i32, d.month()) == d
}

/// True if this date is the last trading day of August.
fn is_last_trading_day_of_august(d: NaiveDate) -> bool {
    crate::calendar::last_trading_day_of_august(d.year() as i32) == d
}

// ---- Core simulator -----------------------------------------------------

/// Run the backtest over the provided daily data.
///
/// Returns a [`BacktestResult`] with NAV series and summary stats, or an error
/// if there are fewer than 2 data points.
pub fn run_backtest(req: &BacktestRequest, data: &[BacktestDay]) -> Result<BacktestResult, String> {
    if data.len() < 2 {
        return Err("need at least 2 data points for backtest".into());
    }

    let monthly_contribution =
        Decimal::from_f64_retain(req.monthly_contribution.unwrap_or(1000.0))
            .unwrap_or(Decimal::new(1000, 0));
    let m = req.safety_buffer_m.unwrap_or(10);

    // ---- Hi5 Baseline simulation ----
    let mut hi5 = PortfolioSnapshot::new(Decimal::ZERO);
    // ---- Hi5e Dynamic simulation ----
    let mut hi5e = PortfolioSnapshot::new(Decimal::ZERO);

    let mut nav_series: Vec<NavPoint> = Vec::with_capacity(data.len());
    let mut hi5_daily_returns: Vec<f64> = Vec::with_capacity(data.len());
    let mut hi5e_daily_returns: Vec<f64> = Vec::with_capacity(data.len());

    let mut hi5_total_contrib = Decimal::ZERO;
    let mut hi5e_total_contrib = Decimal::ZERO;

    // Track whether the Hi5e August rebalance has been deferred
    let mut hi5e_august_deferred_until: Option<NaiveDate> = None;

    let mut prev_hi5_nav: Option<Decimal> = None;
    let mut prev_hi5e_nav: Option<Decimal> = None;

    for (i, day) in data.iter().enumerate() {
        let prices = prices_from_day(day);

        // ---- Hi5 logic ----
        let is_trade_day = is_third_friday(day.date);
        let is_august_rebalance = is_last_trading_day_of_august(day.date);

        if is_trade_day && !is_august_rebalance {
            // Monthly contribution
            hi5.cash += monthly_contribution;
            hi5_total_contrib += monthly_contribution;
            // Fill-the-gap with full monthly contribution budget
            execute_fill_the_gap_with_budget(&mut hi5, &prices, monthly_contribution);
        }

        if is_august_rebalance {
            execute_rebalance(&mut hi5, &prices);
        }

        // ---- Hi5e logic ----
        // Classify zone from the day's data
        let rsp_daily_return = if i > 0 {
            let prev_rsp = Decimal::from_f64_retain(data[i - 1].rsp).unwrap_or(Decimal::ZERO);
            let cur_rsp = Decimal::from_f64_retain(day.rsp).unwrap_or(Decimal::ZERO);
            if prev_rsp != Decimal::ZERO {
                Some(((cur_rsp - prev_rsp) / prev_rsp).to_f64().unwrap_or(0.0))
            } else {
                None
            }
        } else {
            None
        };

        let rsp_mtd_drawdown = compute_mtd_drawdown(data, i);

        let zone_snap = classify_zone(
            day.aaii_bulls,
            day.aaii_bears,
            day.naaim_exposure,
            day.sp500_pct_above_200ma,
            day.vix,
            rsp_daily_return,
            rsp_mtd_drawdown,
        );

        let trade_day_hi5e = is_trade_day;
        let august_reb = is_august_rebalance;

        if trade_day_hi5e && !august_reb {
            // Hi5e dynamic budget: pass full monthly_contribution (e.g. $1000)
            let base_budget = monthly_contribution;
            let dynamic_budget = crate::radar::hi5e_dynamic_budget(base_budget, zone_snap.zone);

            // In Normal/Caution, deploy 50% ($500); rest ($500) goes to SGOV pool
            // In Panic/ExtremePanic, deploy multiplier × base budget from cash + SGOV
            let (deploy, to_sgov) = match zone_snap.zone {
                ExtremeZone::Normal | ExtremeZone::Caution => {
                    let deploy = dynamic_budget; // = 0.5 × base_budget ($500)
                    let reserve = monthly_contribution - deploy;
                    (deploy, reserve)
                }
                ExtremeZone::Panic | ExtremeZone::ExtremePanic => {
                    // Pull from SGOV pool to fund the extra ($2000-$3000)
                    let extra_needed = dynamic_budget - base_budget;
                    let from_sgov = if extra_needed > Decimal::ZERO && hi5e.sgov_pool >= extra_needed {
                        hi5e.sgov_pool -= extra_needed;
                        extra_needed
                    } else if extra_needed > Decimal::ZERO {
                        let available = hi5e.sgov_pool;
                        hi5e.sgov_pool = Decimal::ZERO;
                        available
                    } else {
                        Decimal::ZERO
                    };
                    (base_budget + from_sgov, Decimal::ZERO)
                }
            };

            hi5e.cash += deploy;
            hi5e.sgov_pool += to_sgov;
            hi5e_total_contrib += monthly_contribution;
            execute_fill_the_gap_with_budget(&mut hi5e, &prices, deploy);
        }

        // August rebalance for Hi5e: defer by 15 trading days if in panic zone
        if august_reb {
            let in_panic = matches!(
                zone_snap.zone,
                ExtremeZone::Panic | ExtremeZone::ExtremePanic
            );
            if in_panic {
                // Defer 15 trading days
                hi5e_august_deferred_until = Some(day.date + chrono::Duration::days(21)); // ~15 trading days
            } else if hi5e_august_deferred_until.is_none() {
                execute_rebalance(&mut hi5e, &prices);
            }
        }
        // Execute deferred rebalance
        if let Some(deferred_date) = hi5e_august_deferred_until {
            if day.date >= deferred_date {
                execute_rebalance(&mut hi5e, &prices);
                hi5e_august_deferred_until = None;
            }
        }

        // Record NAV
        let hi5_nav = hi5.total_value(&prices);
        let hi5e_nav = hi5e.total_value(&prices);

        // Daily returns for Sharpe
        if let Some(prev) = prev_hi5_nav {
            if prev != Decimal::ZERO {
                let ret = ((hi5_nav - prev) / prev).to_f64().unwrap_or(0.0);
                hi5_daily_returns.push(ret);
            }
        }
        if let Some(prev) = prev_hi5e_nav {
            if prev != Decimal::ZERO {
                let ret = ((hi5e_nav - prev) / prev).to_f64().unwrap_or(0.0);
                hi5e_daily_returns.push(ret);
            }
        }

        nav_series.push(NavPoint {
            date: day.date,
            hi5_nav: hi5_nav.to_f64().unwrap_or(0.0),
            hi5_e_nav: hi5e_nav.to_f64().unwrap_or(0.0),
        });

        prev_hi5_nav = Some(hi5_nav);
        prev_hi5e_nav = Some(hi5e_nav);
    }

    // Compute summary statistics
    let hi5_final = nav_series
        .last()
        .map(|p| p.hi5_nav)
        .unwrap_or(0.0);
    let hi5e_final = nav_series
        .last()
        .map(|p| p.hi5_e_nav)
        .unwrap_or(0.0);
    let hi5_contrib = hi5_total_contrib.to_f64().unwrap_or(1.0);
    let hi5e_contrib = hi5e_total_contrib.to_f64().unwrap_or(1.0);

    let years = (data.last().unwrap().date - data[0].date).num_days() as f64 / 365.25;
    let years = if years < 0.5 { 0.5 } else { years };

    let hi5_nav_list: Vec<f64> = nav_series.iter().map(|p| p.hi5_nav).collect();
    let hi5e_nav_list: Vec<f64> = nav_series.iter().map(|p| p.hi5_e_nav).collect();

    Ok(BacktestResult {
        request: BacktestRequestSummary {
            start_date: req.start_date,
            end_date: req.end_date,
            monthly_contribution: req.monthly_contribution.unwrap_or(1000.0),
            safety_buffer_m: m,
            trading_days: data.len(),
        },
        hi5: compute_stats("Hi5 Baseline", hi5_final, hi5_contrib, years, &hi5_daily_returns, &hi5_nav_list),
        hi5e: compute_stats("Hi5e Dynamic", hi5e_final, hi5e_contrib, years, &hi5e_daily_returns, &hi5e_nav_list),
        nav_series,
    })
}

fn compute_stats(
    name: &'static str,
    final_nav: f64,
    total_contrib: f64,
    years: f64,
    daily_returns: &[f64],
    nav_series: &[f64],
) -> StrategyResult {
    let total_return_pct = if total_contrib > 0.0 {
        ((final_nav - total_contrib) / total_contrib) * 100.0
    } else {
        0.0
    };

    let cagr_pct = if total_contrib > 0.0 && years > 0.0 {
        ((final_nav / total_contrib).powf(1.0 / years) - 1.0) * 100.0
    } else {
        0.0
    };

    // Max drawdown computed directly from NAV series
    let (max_dd, _) = compute_max_drawdown(nav_series);

    // Sharpe ratio (annualized, assuming 0% risk-free rate for simplicity)
    let sharpe = if daily_returns.len() > 1 {
        let mean = daily_returns.iter().sum::<f64>() / daily_returns.len() as f64;
        let variance = daily_returns
            .iter()
            .map(|r| (r - mean).powi(2))
            .sum::<f64>()
            / (daily_returns.len() - 1) as f64;
        let std_dev = variance.sqrt();
        if std_dev > 0.0 {
            (mean / std_dev) * (252.0_f64).sqrt() // annualize
        } else {
            0.0
        }
    } else {
        0.0
    };

    StrategyResult {
        strategy: name,
        final_nav: (final_nav * 100.0).round() / 100.0,
        total_contributions: total_contrib,
        total_return_pct: (total_return_pct * 100.0).round() / 100.0,
        cagr_pct: (cagr_pct * 100.0).round() / 100.0,
        max_drawdown_pct: (max_dd * 100.0).round() / 100.0,
        sharpe_ratio: (sharpe * 1000.0).round() / 1000.0,
    }
}

fn compute_max_drawdown(nav_series: &[f64]) -> (f64, usize) {
    let mut peak = f64::MIN;
    let mut max_dd = 0.0_f64;
    let mut dd_start = 0;
    for (i, &v) in nav_series.iter().enumerate() {
        if v > peak {
            peak = v;
        }
        if peak > 0.0 {
            let dd = (peak - v) / peak;
            if dd > max_dd {
                max_dd = dd;
                dd_start = i;
            }
        }
    }
    (-max_dd, dd_start)
}

fn compute_mtd_drawdown(data: &[BacktestDay], current_idx: usize) -> Option<f64> {
    if current_idx == 0 {
        return None;
    }
    let current_month = data[current_idx].date.month();
    let current_year = data[current_idx].date.year();

    // Find the first day of the current month
    let mut month_start = current_idx;
    while month_start > 0 {
        let d = data[month_start - 1].date;
        if d.month() != current_month || d.year() != current_year {
            break;
        }
        month_start -= 1;
    }

    let mut peak: f64 = f64::MIN;
    let mut max_dd: f64 = 0.0;
    for i in month_start..=current_idx {
        let price = data[i].rsp;
        if price > peak {
            peak = price;
        }
        if peak > 0.0 {
            let dd = (price - peak) / peak;
            if dd < max_dd {
                max_dd = dd;
            }
        }
    }
    if max_dd < 0.0 {
        Some(max_dd)
    } else {
        None
    }
}

// ---- Allocation helpers -------------------------------------------------

/// Execute Fill-the-Gap on a portfolio snapshot, spending available cash up to specified budget.
fn execute_fill_the_gap_with_budget(snap: &mut PortfolioSnapshot, prices: &[Decimal; 5], budget: Decimal) {
    if budget <= Decimal::ZERO || snap.cash <= Decimal::ZERO {
        return;
    }
    let budget_usd = UsdCash(budget.min(snap.cash));

    // Construct a canonical PortfolioState for unified allocation
    let positions: Vec<Position> = Ticker::ALL
        .iter()
        .enumerate()
        .map(|(i, &ticker)| Position {
            ticker,
            shares: Shares(snap.shares[i]),
            price: UsdPrice(prices[i]),
        })
        .collect();

    let state = PortfolioState {
        cash_usd: UsdCash(snap.cash),
        positions,
    };

    if let Ok(orders) = crate::buffer_pool::fill_the_gap(&state, budget_usd) {
        for order in orders {
            if let Some(idx) = Ticker::ALL.iter().position(|&t| t == order.ticker) {
                snap.shares[idx] += order.shares.0;
                snap.cash -= order.est_cost.0;
            }
        }
    }
}

/// Execute the annual 20% equal-weight rebalance.
fn execute_rebalance(snap: &mut PortfolioSnapshot, prices: &[Decimal; 5]) {
    let total = snap.total_value(prices);
    if total <= Decimal::ZERO {
        return;
    }
    // First sell overweight tickers
    for i in 0..5 {
        let price = prices[i];
        if price <= Decimal::ZERO {
            continue;
        }
        let target_mv = total * TARGET_WEIGHT;
        let current_mv = Decimal::from(snap.shares[i]) * price;
        if current_mv > target_mv {
            let excess = current_mv - target_mv;
            let sell_shares = (excess / price).floor().to_u64().unwrap_or(0);
            if sell_shares > 0 && sell_shares <= snap.shares[i] {
                snap.shares[i] -= sell_shares;
                snap.cash += Decimal::from(sell_shares) * price;
            }
        }
    }
    // Then buy underweight tickers
    for i in 0..5 {
        let price = prices[i];
        if price <= Decimal::ZERO {
            continue;
        }
        let target_mv = total * TARGET_WEIGHT;
        let current_mv = Decimal::from(snap.shares[i]) * price;
        if current_mv < target_mv && snap.cash > Decimal::ZERO {
            let need = target_mv - current_mv;
            let affordable = (need / price).floor().min((snap.cash / price).floor());
            let buy = affordable.to_u64().unwrap_or(0);
            if buy > 0 {
                let cost = Decimal::from(buy) * price;
                if cost <= snap.cash {
                    snap.shares[i] += buy;
                    snap.cash -= cost;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, NaiveDate};

    fn make_test_data() -> Vec<BacktestDay> {
        // Generate daily data for Q1 2020 (~60 trading days) with a third Friday.
        // Jan 2020: third Friday = Jan 17.
        let mut days = Vec::new();
        let start = NaiveDate::from_ymd_opt(2020, 1, 2).unwrap();
        let end = NaiveDate::from_ymd_opt(2020, 3, 31).unwrap();
        let mut d = start;
        let mut i = 0;
        while d <= end {
            // Skip weekends
            if d.weekday().num_days_from_monday() < 5 {
                let base = 100.0 + (i as f64) * 0.5;
                days.push(BacktestDay {
                    date: d,
                    iwy: base * 1.2,
                    spmo: base * 1.1,
                    rsp: base * 1.0,
                    pff: 28.0 + i as f64 * 0.02,
                    vnq: 90.0 + i as f64 * 0.1,
                    aaii_bulls: Some(40.0),
                    aaii_bears: Some(30.0),
                    naaim_exposure: Some(80.0),
                    sp500_pct_above_200ma: Some(70.0),
                    vix: Some(18.0),
                });
                i += 1;
            }
            d += chrono::Duration::days(1);
        }
        days
    }

    #[test]
    fn backtest_produces_result() {
        let data = make_test_data();
        let req = BacktestRequest {
            start_date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
            monthly_contribution: Some(1000.0),
            safety_buffer_m: Some(10),
        };
        let result = run_backtest(&req, &data).expect("backtest should succeed");
        assert!(!result.nav_series.is_empty());
        assert!(result.hi5.final_nav > 0.0);
        assert!(result.hi5e.final_nav > 0.0);
        // NAV series length should match data length
        assert_eq!(result.nav_series.len(), data.len());
    }

    #[test]
    fn backtest_rejects_too_few_points() {
        let data = vec![BacktestDay {
            date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            iwy: 100.0,
            spmo: 100.0,
            rsp: 100.0,
            pff: 28.0,
            vnq: 90.0,
            aaii_bulls: None,
            aaii_bears: None,
            naaim_exposure: None,
            sp500_pct_above_200ma: None,
            vix: None,
        }];
        let req = BacktestRequest {
            start_date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
            monthly_contribution: Some(1000.0),
            safety_buffer_m: Some(10),
        };
        assert!(run_backtest(&req, &data).is_err());
    }
}
