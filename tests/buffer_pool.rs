//! Integration test: parse mock Questrade JSON (balances + positions + quotes)
//! into the DTOs, build a PortfolioState, and prove the Dynamic Buffer Pool
//! Algorithm floors share targets and survives asymmetric balances without
//! panicking.

use hi5bot::buffer_pool::{available_per_trade, fill_the_gap};
use hi5bot::types::*;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

/// Extract settled USD cash from a Questrade balances response.
fn usd_cash(bal: &BalancesResponse) -> UsdCash {
    bal.per_currency_balances
        .iter()
        .find(|c| c.currency == "USD")
        .map(|c| UsdCash(c.cash))
        .unwrap_or(UsdCash::ZERO)
}

/// Merge Questrade positions + quotes into a PortfolioState covering all 5
/// tickers (zero shares where unheld), using the bid (falling back to last
/// trade, then position price) as the limit price.
fn build_state(
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
        let price = quote_for(meta.ticker);
        let price = if price.0 <= Decimal::ZERO {
            // fall back to the position's current price if no quote
            dto.map(|p| UsdPrice(p.current_price))
                .unwrap_or(UsdPrice::ZERO)
        } else {
            price
        };
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

/// Assert the four core invariants on any order set.
fn assert_invariants(orders: &[AllocationOrder], budget: UsdCash, state: &PortfolioState) {
    let total: Decimal = orders.iter().map(|o| o.est_cost.0).sum();
    assert!(
        total <= budget.0,
        "total cost {total} exceeds budget {}",
        budget.0
    );
    let total_value = state.total_value().0;
    for o in orders {
        // whole shares + exact Decimal cost
        assert_eq!(
            o.est_cost.0,
            Decimal::from(o.shares.0) * o.limit_price.0,
            "non-integer or drifted cost"
        );
        // never overshoot 20% target
        let held = state.position(o.ticker).map(|p| p.shares.0).unwrap_or(0);
        let post_mv = Decimal::from(held + o.shares.0) * o.limit_price.0;
        let target_mv = total_value * TARGET_WEIGHT;
        assert!(
            post_mv <= target_mv,
            "{} post-buy mv {post_mv} overshoots target {target_mv}",
            o.ticker
        );
    }
}

const BALANCES_JSON: &str = r#"{
  "perCurrencyBalances": [
    { "currency": "CAD", "cash": 0, "marketValue": 0, "totalEquity": 0, "cashAvailableForTrade": 0 },
    { "currency": "USD", "cash": 5250.00, "marketValue": 42000.00, "totalEquity": 47250.00, "cashAvailableForTrade": 5250.00 }
  ]
}"#;

// Asymmetric: IWY heavily overweight, RSP/VNQ underweight, PFF at target.
const POSITIONS_JSON: &str = r#"{
  "positions": [
    { "symbol": "IWY",  "symbolId": 1, "openQuantity": 250, "currentPrice": 120.00, "currentMarketValue": 30000.00, "averageEntryPrice": 110.00, "side": "Long" },
    { "symbol": "SPMO", "symbolId": 2, "openQuantity": 40,  "currentPrice": 150.00, "currentMarketValue": 6000.00,  "averageEntryPrice": 140.00, "side": "Long" },
    { "symbol": "RSP",  "symbolId": 3, "openQuantity": 5,   "currentPrice": 180.00, "currentMarketValue": 900.00,   "averageEntryPrice": 175.00, "side": "Long" },
    { "symbol": "PFF",  "symbolId": 4, "openQuantity": 150, "currentPrice": 28.00,  "currentMarketValue": 4200.00,  "averageEntryPrice": 27.00,  "side": "Long" },
    { "symbol": "VNQ",  "symbolId": 5, "openQuantity": 10,  "currentPrice": 90.00,  "currentMarketValue": 900.00,   "averageEntryPrice": 88.00,  "side": "Long" }
  ]
}"#;

const QUOTES_JSON: &str = r#"{
  "quotes": [
    { "symbol": "IWY",  "symbolId": 1, "bidPrice": 119.80, "askPrice": 120.05, "lastTradePrice": 120.00 },
    { "symbol": "SPMO", "symbolId": 2, "bidPrice": 149.90, "askPrice": 150.10, "lastTradePrice": 150.00 },
    { "symbol": "RSP",  "symbolId": 3, "bidPrice": 179.75, "askPrice": 180.05, "lastTradePrice": 180.00 },
    { "symbol": "PFF",  "symbolId": 4, "bidPrice": 27.95,  "askPrice": 28.02,  "lastTradePrice": 28.00 },
    { "symbol": "VNQ",  "symbolId": 5, "bidPrice": 89.88,  "askPrice": 90.04,  "lastTradePrice": 90.00 }
  ]
}"#;

#[test]
fn buffer_pool_handles_mock_questrade_json_asymmetric() {
    let balances: BalancesResponse = serde_json::from_str(BALANCES_JSON).unwrap();
    let positions: PositionsResponse = serde_json::from_str(POSITIONS_JSON).unwrap();
    let quotes: QuotesResponse = serde_json::from_str(QUOTES_JSON).unwrap();

    let cash = usd_cash(&balances);
    assert_eq!(cash, UsdCash(Decimal::new(5250, 0)));

    let state = build_state(&positions, &quotes, cash);
    // total = settled cash + sum(bid_price * shares); positions are priced at
    // the bid (the limit price the daemon would trade at), not currentMarketValue.
    assert_eq!(
        state.total_value().0,
        "47186.05".parse::<Decimal>().unwrap()
    );

    let budget = available_per_trade(cash, 10); // 525
    assert_eq!(budget.0, Decimal::new(525, 0));

    let orders = fill_the_gap(&state, budget).unwrap();
    assert!(
        !orders.is_empty(),
        "should allocate to underweight laggards"
    );

    // IWY is overweight (~63.5%) -> must never be bought.
    assert!(orders.iter().all(|o| o.ticker != Ticker::Iwy));

    // Worst laggard first: RSP and VNQ are the most underweight.
    // RSP current weight ~1.9%, VNQ ~1.9%; both far below 20%.
    let first = orders[0].ticker;
    assert!(
        first == Ticker::Rsp || first == Ticker::Vnq,
        "worst laggard first, got {first}"
    );

    assert_invariants(&orders, budget, &state);

    // Every share count is a whole integer (floored).
    for o in &orders {
        let _whole: u64 = o.shares.0; // Shares is already u64
        assert!(o.shares.0 > 0);
    }
}

#[test]
fn buffer_pool_zero_usd_cash_no_panic() {
    let zero_balances = r#"{ "perCurrencyBalances": [ { "currency": "USD", "cash": 0, "marketValue": 0, "totalEquity": 0, "cashAvailableForTrade": 0 } ] }"#;
    let balances: BalancesResponse = serde_json::from_str(zero_balances).unwrap();
    let positions: PositionsResponse = serde_json::from_str(POSITIONS_JSON).unwrap();
    let quotes: QuotesResponse = serde_json::from_str(QUOTES_JSON).unwrap();

    let cash = usd_cash(&balances);
    assert_eq!(cash, UsdCash::ZERO);

    let state = build_state(&positions, &quotes, cash);
    let budget = available_per_trade(cash, 10);
    assert_eq!(budget, UsdCash::ZERO);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fill_the_gap(&state, budget).unwrap()
    }));
    assert!(result.is_ok(), "must not panic on zero USD cash");
    assert!(result.unwrap().is_empty());
}

#[test]
fn buffer_pool_division_and_flooring_exact() {
    // Budget that doesn't divide evenly by a price must floor, not round.
    let balances: BalancesResponse = serde_json::from_str(BALANCES_JSON).unwrap();
    let positions: PositionsResponse = serde_json::from_str(r#"{ "positions": [] }"#).unwrap();
    let quotes: QuotesResponse = serde_json::from_str(QUOTES_JSON).unwrap();

    let cash = usd_cash(&balances);
    let state = build_state(&positions, &quotes, cash);
    let budget = available_per_trade(cash, 10); // 525

    let orders = fill_the_gap(&state, budget).unwrap();
    // Every cost is shares*price exactly and <= budget.
    let total: Decimal = orders.iter().map(|o| o.est_cost.0).sum();
    assert!(total <= budget.0);
    for o in &orders {
        assert_eq!(o.est_cost.0, Decimal::from(o.shares.0) * o.limit_price.0);
    }
}
