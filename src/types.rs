//! Core domain types for Hi5-Bot.
//!
//! ## Guardrails encoded here
//!
//! - **No floats.** Every monetary value, price, and unit count uses
//!   [`rust_decimal::Decimal`] (or whole-share [`Shares`]) to prevent IEEE 754
//!   precision bleeding.
//! - **Currency hard-lock (compile-time firewall).** [`UsdCash`], [`CadCash`],
//!   and [`UsdPrice`] are distinct newtypes. Adding CAD to USD is a *compile
//!   error*, which makes it impossible to accidentally spend CAD credit on a
//!   USD trade (Questrade's 1.5%+ automatic forex spread).
//! - **Fixed portfolio.** The 5-ETF equal-weight target is a compile-time
//!   constant ([`PORTFOLIO`], [`TARGET_WEIGHT`]).

use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Mul, Sub, SubAssign};

// ----------------------------------------------------------------------------
// Portfolio
// ----------------------------------------------------------------------------

/// The fixed 5-ETF universe. Order matches the spec's portfolio table.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Ticker {
    Iwy,
    Spmo,
    Rsp,
    Pff,
    Vnq,
}

impl Ticker {
    /// All tickers in canonical order.
    pub const ALL: [Ticker; 5] = [
        Ticker::Iwy,
        Ticker::Spmo,
        Ticker::Rsp,
        Ticker::Pff,
        Ticker::Vnq,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Ticker::Iwy => "IWY",
            Ticker::Spmo => "SPMO",
            Ticker::Rsp => "RSP",
            Ticker::Pff => "PFF",
            Ticker::Vnq => "VNQ",
        }
    }

    pub fn parse(s: &str) -> Option<Ticker> {
        match s.trim().to_ascii_uppercase().as_str() {
            "IWY" => Some(Ticker::Iwy),
            "SPMO" => Some(Ticker::Spmo),
            "RSP" => Some(Ticker::Rsp),
            "PFF" => Some(Ticker::Pff),
            "VNQ" => Some(Ticker::Vnq),
            _ => None,
        }
    }
}

impl std::fmt::Display for Ticker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Copy, Clone, Debug)]
pub struct AssetMeta {
    pub ticker: Ticker,
    pub name: &'static str,
    pub target_weight: Decimal,
}

/// Equal-weight target: 20.0% per ticker.
pub const TARGET_WEIGHT: Decimal = Decimal::from_parts(20, 0, 0, false, 2); // 0.20

/// Rolling month safety buffer `M`. `Available_Per_Trade = Current_USD_Cash / M`.
pub const SAFETY_BUFFER_M: u32 = 10;

/// The fixed portfolio: 5 U.S.-listed ETFs at 20.0% each.
pub const PORTFOLIO: &[AssetMeta] = &[
    AssetMeta {
        ticker: Ticker::Iwy,
        name: "iShares Russell Top 200 Growth ETF",
        target_weight: TARGET_WEIGHT,
    },
    AssetMeta {
        ticker: Ticker::Spmo,
        name: "Invesco S&P 500 Momentum ETF",
        target_weight: TARGET_WEIGHT,
    },
    AssetMeta {
        ticker: Ticker::Rsp,
        name: "Invesco S&P 500 Equal Weight ETF",
        target_weight: TARGET_WEIGHT,
    },
    AssetMeta {
        ticker: Ticker::Pff,
        name: "iShares Preferred & Income Securities ETF",
        target_weight: TARGET_WEIGHT,
    },
    AssetMeta {
        ticker: Ticker::Vnq,
        name: "Vanguard Real Estate ETF",
        target_weight: TARGET_WEIGHT,
    },
];

// ----------------------------------------------------------------------------
// State machine
// ----------------------------------------------------------------------------

/// The strict calendar-month execution state machine. The daemon is limited to
/// a maximum of 3 incremental buys per month; [`Self::trade_index`] maps each
/// signal to the buy slot (0, 1, 2) it occupies.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MarketSignal {
    /// Trigger 1: RSP daily drop <= -1.0% (buy #1).
    RegularLowSlip,
    /// Trigger 2: calendar fallback (3rd Friday) if Trigger 1 didn't fire (buy #1).
    GuaranteedThirdFriday,
    /// Trigger 3: RSP monthly drawdown <= -5.0% (buy #2).
    DeepRetracement,
    /// Trigger 4: "Humanity's Limit" — VIX >= 35 AND RSP daily <= -3.0% (buy #3).
    ExtremePanic,
    /// Trigger 5: fixed calendar reset on the last trading day of August.
    AnnualRebalance,
}

impl MarketSignal {
    /// Zero-based buy slot this signal occupies within the month, or `None` for
    /// the annual rebalance (which disregards monthly limits).
    pub fn trade_index(self) -> Option<u32> {
        match self {
            MarketSignal::RegularLowSlip | MarketSignal::GuaranteedThirdFriday => Some(0),
            MarketSignal::DeepRetracement => Some(1),
            MarketSignal::ExtremePanic => Some(2),
            MarketSignal::AnnualRebalance => None,
        }
    }

    pub fn is_rebalance(self) -> bool {
        matches!(self, MarketSignal::AnnualRebalance)
    }
}

// ----------------------------------------------------------------------------
// Strongly-typed money (currency hard-lock)
// ----------------------------------------------------------------------------

/// Settled USD cash. Cannot be combined with [`CadCash`].
#[derive(
    Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
pub struct UsdCash(pub Decimal);

impl UsdCash {
    pub const ZERO: UsdCash = UsdCash(Decimal::ZERO);

    pub fn new(d: Decimal) -> Self {
        UsdCash(d)
    }
}

impl std::str::FromStr for UsdCash {
    type Err = rust_decimal::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<Decimal>().map(UsdCash)
    }
}

/// Settled CAD cash. Kept distinct so CAD credit can never fund a USD trade.
#[derive(
    Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
pub struct CadCash(pub Decimal);

/// A USD price per share (e.g. bid price used as the limit price).
#[derive(
    Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
pub struct UsdPrice(pub Decimal);

impl UsdPrice {
    pub const ZERO: UsdPrice = UsdPrice(Decimal::ZERO);
}

impl std::str::FromStr for UsdPrice {
    type Err = rust_decimal::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<Decimal>().map(UsdPrice)
    }
}

/// Whole (integer) share count only. Allocation always floors to this.
#[derive(
    Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
pub struct Shares(pub u64);

impl Shares {
    pub const ZERO: Shares = Shares(0);
}

impl Add for UsdCash {
    type Output = UsdCash;
    fn add(self, rhs: UsdCash) -> UsdCash {
        UsdCash(self.0 + rhs.0)
    }
}

impl Sub for UsdCash {
    type Output = UsdCash;
    fn sub(self, rhs: UsdCash) -> UsdCash {
        UsdCash(self.0 - rhs.0)
    }
}

impl AddAssign for UsdCash {
    fn add_assign(&mut self, rhs: UsdCash) {
        self.0 += rhs.0;
    }
}

impl SubAssign for UsdCash {
    fn sub_assign(&mut self, rhs: UsdCash) {
        self.0 -= rhs.0;
    }
}

/// `price * shares -> UsdCash` (exact Decimal multiplication, no float drift).
impl Mul<Shares> for UsdPrice {
    type Output = UsdCash;
    fn mul(self, shares: Shares) -> UsdCash {
        UsdCash(self.0 * Decimal::from(shares.0))
    }
}

impl Mul<UsdPrice> for Shares {
    type Output = UsdCash;
    fn mul(self, price: UsdPrice) -> UsdCash {
        price * self
    }
}

// ----------------------------------------------------------------------------
// Normalized portfolio view (used by the strategy / buffer pool)
// ----------------------------------------------------------------------------

/// A normalized position. `shares` may be zero for an unheld ticker; `price` is
/// always the latest quote so the allocator can size a fresh purchase.
#[derive(Clone, Debug)]
pub struct Position {
    pub ticker: Ticker,
    pub shares: Shares,
    pub price: UsdPrice,
}

impl Position {
    /// Current market value = price * shares (exact Decimal).
    pub fn market_value(&self) -> UsdCash {
        self.price * self.shares
    }
}

/// A snapshot of an account used for sizing: settled USD cash plus positions.
#[derive(Clone, Debug)]
pub struct PortfolioState {
    pub cash_usd: UsdCash,
    pub positions: Vec<Position>,
}

impl PortfolioState {
    /// Total USD value = settled cash + sum of position market values.
    pub fn total_value(&self) -> UsdCash {
        let mut total = self.cash_usd;
        for p in &self.positions {
            total += p.market_value();
        }
        total
    }

    /// Look up a position by ticker.
    pub fn position(&self, ticker: Ticker) -> Option<&Position> {
        self.positions.iter().find(|p| p.ticker == ticker)
    }
}

/// A computed buy order from the Fill-the-Gap allocator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocationOrder {
    pub ticker: Ticker,
    pub shares: Shares,
    /// Estimated cash cost = shares * limit_price (exact Decimal).
    pub est_cost: UsdCash,
    /// Limit price = current bid/quote (eliminates Questrade ECN fees).
    pub limit_price: UsdPrice,
}

/// A buy or sell order direction for the rebalance path.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// A sized order (buy or sell) produced by the annual rebalance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebalanceOrder {
    pub ticker: Ticker,
    pub side: OrderSide,
    pub shares: Shares,
    pub limit_price: UsdPrice,
}

/// Questrade per-account settlement preference. The daemon hard-aborts unless
/// this is [`SettlementPreference::CurrencyOfTransaction`].
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SettlementPreference {
    CurrencyOfTransaction,
    Base,
    Other(String),
}

impl SettlementPreference {
    pub fn from_questrade(s: &str) -> Self {
        match s.trim() {
            "Currency of Transaction" => SettlementPreference::CurrencyOfTransaction,
            "Base" => SettlementPreference::Base,
            other => SettlementPreference::Other(other.to_string()),
        }
    }

    pub fn is_currency_of_transaction(&self) -> bool {
        matches!(self, SettlementPreference::CurrencyOfTransaction)
    }
}

// ----------------------------------------------------------------------------
// Questrade REST DTOs
// ----------------------------------------------------------------------------

/// `GET v1/accounts`
#[derive(Debug, Deserialize)]
pub struct AccountsResponse {
    pub accounts: Vec<AccountDto>,
    #[serde(default, rename = "userId")]
    pub user_id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    #[serde(rename = "type")]
    pub kind: String,
    pub number: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub is_primary: bool,
    #[serde(default)]
    pub client_account_type: String,
}

/// `GET v1/accounts/{id}/balances`
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalancesResponse {
    #[serde(default)]
    pub per_currency_balances: Vec<CurrencyBalances>,
    #[serde(default)]
    pub combined_balances: Vec<CurrencyBalances>,
    #[serde(default)]
    pub sod_per_currency_balances: Vec<CurrencyBalances>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyBalances {
    pub currency: String,
    #[serde(default)]
    pub cash: Decimal,
    #[serde(default)]
    pub market_value: Decimal,
    #[serde(default)]
    pub total_equity: Decimal,
    #[serde(default)]
    pub cash_available_for_trade: Decimal,
}

/// `GET v1/accounts/{id}/positions`
#[derive(Debug, Deserialize)]
pub struct PositionsResponse {
    pub positions: Vec<PositionDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionDto {
    pub symbol: String,
    #[serde(rename = "symbolId")]
    pub symbol_id: u64,
    pub open_quantity: Decimal,
    pub current_price: Decimal,
    #[serde(default)]
    pub current_market_value: Decimal,
    #[serde(default)]
    pub average_entry_price: Decimal,
    #[serde(default)]
    pub side: String,
}

/// `GET v1/markets/quotes?ids=...`
#[derive(Debug, Deserialize)]
pub struct QuotesResponse {
    pub quotes: Vec<QuoteDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteDto {
    pub symbol: String,
    #[serde(rename = "symbolId")]
    pub symbol_id: u64,
    #[serde(default)]
    pub bid_price: Option<Decimal>,
    #[serde(default)]
    pub ask_price: Option<Decimal>,
    #[serde(default)]
    pub last_trade_price: Option<Decimal>,
    #[serde(default)]
    pub low_price: Option<Decimal>,
    #[serde(default)]
    pub high_price: Option<Decimal>,
    #[serde(default)]
    pub vwap: Option<Decimal>,
}

/// `GET v1/markets/candles/{id}?startTime=...&endTime=...&interval=OneDay`
#[derive(Debug, Deserialize)]
pub struct CandlesResponse {
    pub candles: Vec<CandleDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandleDto {
    /// Questrade returns ISO-8601 with offset; deserialized via chrono's RFC3339 support.
    pub start: chrono::DateTime<chrono::FixedOffset>,
    pub end: chrono::DateTime<chrono::FixedOffset>,
    pub low: Decimal,
    pub high: Decimal,
    pub close: Decimal,
    #[serde(default)]
    pub open: Option<Decimal>,
    #[serde(default)]
    pub volume: Option<u64>,
}

/// `POST v1/accounts/{id}/orders` body. Always a Day Limit order at the bid.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderRequest {
    pub symbol_id: u64,
    pub quantity: u64,
    pub limit_price: Decimal,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub action: OrderAction,
    pub primary_route: String,
    pub secondary_route: String,
}

#[derive(Debug, Serialize)]
pub enum OrderType {
    Limit,
    #[allow(dead_code)]
    Market,
}

#[derive(Debug, Serialize)]
pub enum TimeInForce {
    Day,
    #[allow(dead_code)]
    Gtc,
}

#[derive(Debug, Serialize)]
pub enum OrderAction {
    Buy,
    Sell,
}

/// A helper to keep `acquired_at`/`expires_at` paired in [`auth::TokenData`].
/// Re-exported here so callers deal with a single time type.
#[allow(dead_code)]
pub type Timestamp = NaiveDateTime;
