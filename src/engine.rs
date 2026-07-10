//! Single evaluation tick: the orchestration that ties auth -> market data ->
//! state machine -> buffer pool -> safety gate -> order placement.

use crate::auth::TokenStore;
use crate::buffer_pool::{available_per_trade, fill_the_gap};
use crate::config::Settings;
use crate::error::{Error, Result};
use crate::notify;
use crate::questrade::{QuestradeClient, qt_timestamp};
use crate::state_store::MonthlyStateStore;
use crate::strategy::{MonthlyState, compute_market_state_value, evaluate_signal};
use crate::types::*;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

/// Summary of one account's evaluation.
#[derive(Debug)]
pub struct TickSummary {
    pub account: String,
    pub signal: Option<MarketSignal>,
    pub orders_placed: usize,
}

/// Extract settled USD cash from a balances response.
pub fn usd_cash(bal: &BalancesResponse) -> UsdCash {
    bal.per_currency_balances
        .iter()
        .find(|c| c.currency == "USD")
        .map(|c| UsdCash(c.cash))
        .unwrap_or(UsdCash::ZERO)
}

/// Build a [`PortfolioState`] from Questrade positions + quotes, covering all 5
/// tickers (zero shares where unheld). Limit price = bid (fallback last, then
/// position current price).
pub fn build_portfolio_state(
    positions: &PositionsResponse,
    quotes: &QuotesResponse,
    cash: UsdCash,
) -> PortfolioState {
    let quote_for = |ticker: Ticker| -> UsdPrice {
        quotes
            .quotes
            .iter()
            .find(|q| q.symbol == ticker.as_str())
            .and_then(|q| q.bid_price.or(q.last_trade_price))
            .map(UsdPrice)
            .unwrap_or(UsdPrice::ZERO)
    };

    let mut out = Vec::new();
    for meta in PORTFOLIO {
        let dto = positions
            .positions
            .iter()
            .find(|p| p.symbol == meta.ticker.as_str());
        let shares = dto.and_then(|p| p.open_quantity.to_u64()).unwrap_or(0);
        let mut price = quote_for(meta.ticker);
        if price.0 <= Decimal::ZERO {
            // fall back to the position's own current price
            if let Some(p) = dto {
                price = UsdPrice(p.current_price);
            }
        }
        out.push(Position {
            ticker: meta.ticker,
            shares: Shares(shares),
            price,
        });
    }
    PortfolioState {
        cash_usd: cash,
        positions: out,
    }
}

/// Compute the annual-rebalance orders: target 20% equal weight for every
/// ticker. Returns sells first (to free cash), then buys. Share counts are
/// floored; tickers already at target produce no order.
pub fn rebalance_orders(state: &PortfolioState) -> Vec<RebalanceOrder> {
    let total = state.total_value();
    if total.0 <= Decimal::ZERO {
        return Vec::new();
    }
    let mut sells = Vec::new();
    let mut buys = Vec::new();
    for meta in PORTFOLIO {
        let (shares, price) = match state.position(meta.ticker) {
            Some(p) => (p.shares, p.price),
            None => (Shares::ZERO, UsdPrice::ZERO),
        };
        if price.0 <= Decimal::ZERO {
            continue;
        }
        let target_mv = total.0 * meta.target_weight;
        let target_shares = (target_mv / price.0).floor().to_u64().unwrap_or(0);
        let current = shares.0;
        if target_shares > current {
            buys.push(RebalanceOrder {
                ticker: meta.ticker,
                side: OrderSide::Buy,
                shares: Shares(target_shares - current),
                limit_price: price,
            });
        } else if current > target_shares {
            sells.push(RebalanceOrder {
                ticker: meta.ticker,
                side: OrderSide::Sell,
                shares: Shares(current - target_shares),
                limit_price: price,
            });
        }
    }
    sells.into_iter().chain(buys).collect()
}

/// Run one evaluation tick across all configured accounts.
///
/// On a hard-abort condition (USD cash exhausted, currency hard-lock violated)
/// the webhook is notified and an [`Error`] is returned so the caller can exit.
pub async fn run_tick(
    settings: &Settings,
    tokens: &TokenStore,
    state_store: &MonthlyStateStore,
    http: &reqwest::Client,
    now: NaiveDateTime,
    dry_run: bool,
) -> Result<Vec<TickSummary>> {
    // 1. Ensure a valid access token (refreshes if expired).
    tokens.ensure_valid(http, &settings.token_url, now).await?;
    let qt = QuestradeClient::new(http.clone(), tokens.api_server(), tokens.access_token());

    // 2. Compute the shared MarketState once (RSP candles + VIX).
    let today: NaiveDate = now.date();
    let rsp_id = settings.symbol_id(Ticker::Rsp).unwrap_or(0);
    let candles = if rsp_id != 0 {
        let end = qt_timestamp(now);
        let start = qt_timestamp(now - Duration::days(45));
        qt.candles(rsp_id, start, end, "OneDay")
            .await
            .map(|c| c.candles)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let vix = if settings.vix_symbol_id != 0 {
        qt.quotes(&[settings.vix_symbol_id])
            .await
            .ok()
            .and_then(|q| q.quotes.into_iter().next())
            .and_then(|q| q.last_trade_price.or(q.bid_price))
    } else {
        None
    };
    let market = compute_market_state_value(&candles, vix, today);

    // 3. Evaluate per account.
    let mut summaries = Vec::new();
    for account in settings.accounts() {
        let summary =
            evaluate_account(&qt, settings, state_store, account, &market, today, dry_run).await?;
        summaries.push(summary);
    }
    Ok(summaries)
}

async fn evaluate_account(
    qt: &QuestradeClient,
    settings: &Settings,
    state_store: &MonthlyStateStore,
    account: &str,
    market: &crate::strategy::MarketState,
    today: NaiveDate,
    dry_run: bool,
) -> Result<TickSummary> {
    let balances = qt.balances(account).await?;
    let cash = usd_cash(&balances);
    let positions = qt.positions(account).await?;

    let ids: Vec<u64> = Ticker::ALL
        .iter()
        .map(|t| settings.symbol_id(*t))
        .collect::<Result<Vec<_>>>()?;
    let quotes = qt.quotes(&ids).await?;
    let state = build_portfolio_state(&positions, &quotes, cash);

    let trade_count = state_store.effective(account, today.year(), today.month());
    let monthly = MonthlyState {
        year: today.year(),
        month: today.month(),
        trade_count,
    };
    let signal = evaluate_signal(market, &monthly);

    let mut orders_placed = 0usize;
    if let Some(sig) = signal {
        // Settlement hard-lock: must be "Currency of Transaction".
        let pref = SettlementPreference::from_questrade(&settings.settlement_pref);
        if !pref.is_currency_of_transaction() {
            let reason = format!(
                "account {account}: settlement pref is '{}' (must be Currency of Transaction)",
                settings.settlement_pref
            );
            notify::notify(&settings.notify_webhook, &reason).await;
            return Err(Error::SettlementNotCurrencyOfTransaction(
                settings.settlement_pref.clone(),
            ));
        }

        if sig.is_rebalance() {
            // Annual rebalance: sells first, then buys capped by settled USD cash.
            let orders = rebalance_orders(&state);
            let mut cash_avail = cash;
            for o in &orders {
                let sym_id = settings.symbol_id(o.ticker)?;
                if dry_run {
                    tracing::info!(
                        "[dry-run] {account} {:?} {} {} @ {}",
                        o.side,
                        o.ticker,
                        o.shares.0,
                        o.limit_price.0
                    );
                    orders_placed += 1;
                    continue;
                }
                match o.side {
                    OrderSide::Sell => {
                        qt.place_sell_limit(account, sym_id, o.shares.0, o.limit_price.0)
                            .await?;
                        orders_placed += 1;
                        // Best-effort: proceeds are T+1; we do not assume same-day cash.
                    }
                    OrderSide::Buy => {
                        let cost = o.limit_price * o.shares;
                        if cost.0 > cash_avail.0 {
                            tracing::warn!(
                                "annual rebalance: skip {} buy (cost {} > settled cash {}); T+1 settlement",
                                o.ticker,
                                cost.0,
                                cash_avail.0
                            );
                            continue;
                        }
                        qt.place_buy_limit(account, sym_id, o.shares.0, o.limit_price.0)
                            .await?;
                        cash_avail -= cost;
                        orders_placed += 1;
                    }
                }
            }
        } else {
            // Incremental buy: hard-abort if USD cash <= 0 (never spend CAD credit).
            if cash.0 <= Decimal::ZERO {
                let reason = format!(
                    "account {account}: USD cash <= 0 before {:?} buy; hard-abort",
                    sig
                );
                notify::notify(&settings.notify_webhook, &reason).await;
                return Err(Error::UsdCashExhausted);
            }
            let budget = available_per_trade(cash, settings.safety_buffer_m);
            let alloc = fill_the_gap(&state, budget)?;
            for o in &alloc {
                let sym_id = settings.symbol_id(o.ticker)?;
                if dry_run {
                    tracing::info!(
                        "[dry-run] {account} BUY {} {} @ {} (cost {})",
                        o.ticker,
                        o.shares.0,
                        o.limit_price.0,
                        o.est_cost.0
                    );
                    orders_placed += 1;
                    continue;
                }
                qt.place_buy_limit(account, sym_id, o.shares.0, o.limit_price.0)
                    .await?;
                orders_placed += 1;
            }
            if !alloc.is_empty() && !dry_run {
                state_store.record_trade(account, today.year(), today.month())?;
            } else if !alloc.is_empty() && dry_run {
                tracing::info!("[dry-run] would record trade for {account}");
            }
        }
    }

    Ok(TickSummary {
        account: account.to_string(),
        signal,
        orders_placed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    fn candle(close: i64) -> CandleDto {
        let start = chrono::DateTime::parse_from_rfc3339("2026-07-01T00:00:00-04:00").unwrap();
        CandleDto {
            start,
            end: start,
            low: Decimal::new(close - 1, 0),
            high: Decimal::new(close + 1, 0),
            close: Decimal::new(close, 0),
            open: Some(Decimal::new(close, 0)),
            volume: Some(1000),
        }
    }

    #[test]
    fn compute_market_state_daily_return_and_drawdown() {
        // closes: 100, 98, 102, 96
        let candles = vec![candle(100), candle(98), candle(102), candle(96)];
        let today = NaiveDate::from_ymd_opt(2026, 7, 9).unwrap();
        let ms = compute_market_state_value(&candles, Some(Decimal::new(18, 0)), today);
        // daily return = (96 - 102)/102 = -0.0588...
        assert!(ms.rsp_daily_return < Decimal::ZERO);
        // drawdown: high 102, min close 96 -> (96-102)/102 = -0.0588...
        assert!(ms.rsp_monthly_drawdown <= Decimal::new(-5, 2));
        assert_eq!(ms.vix, Decimal::new(18, 0));
    }

    #[test]
    fn compute_market_state_empty_candles_safe() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 9).unwrap();
        let ms = compute_market_state_value(&[], None, today);
        assert_eq!(ms.rsp_daily_return, Decimal::ZERO);
        assert_eq!(ms.rsp_monthly_drawdown, Decimal::ZERO);
        assert_eq!(ms.vix, Decimal::ZERO);
    }

    #[test]
    fn rebalance_orders_generates_sells_and_buys() {
        // IWY overweight, VNQ underweight; others ~at target.
        let positions = vec![
            Position {
                ticker: Ticker::Iwy,
                shares: Shares(200),
                price: UsdPrice(Decimal::new(120, 0)),
            },
            Position {
                ticker: Ticker::Spmo,
                shares: Shares(72),
                price: UsdPrice(Decimal::new(150, 0)),
            },
            Position {
                ticker: Ticker::Rsp,
                shares: Shares(60),
                price: UsdPrice(Decimal::new(180, 0)),
            },
            Position {
                ticker: Ticker::Pff,
                shares: Shares(385),
                price: UsdPrice(Decimal::new(28, 0)),
            },
            Position {
                ticker: Ticker::Vnq,
                shares: Shares(10),
                price: UsdPrice(Decimal::new(90, 0)),
            },
        ];
        let state = PortfolioState {
            cash_usd: UsdCash::ZERO,
            positions,
        };
        let orders = rebalance_orders(&state);
        let sells: Vec<_> = orders
            .iter()
            .filter(|o| o.side == OrderSide::Sell)
            .collect();
        let buys: Vec<_> = orders.iter().filter(|o| o.side == OrderSide::Buy).collect();
        assert!(!sells.is_empty(), "IWY overweight should produce a sell");
        assert!(!buys.is_empty(), "VNQ underweight should produce a buy");
        // IWY is the overweight one -> sold
        assert!(sells.iter().any(|o| o.ticker == Ticker::Iwy));
        // VNQ is underweight -> bought
        assert!(buys.iter().any(|o| o.ticker == Ticker::Vnq));
    }
}
