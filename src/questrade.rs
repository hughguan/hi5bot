//! Async Questrade REST client.
//!
//! Surface is deliberately minimal and **excludes any banking/EFT/PAD
//! capability** (the "Zero External Pull-In" firewall, spec §5.3). The daemon
//! can only read account/balance/position/quote/candle data and place Day Limit
//! buy/sell orders - it can never initiate a financial pull from an external
//! bank account, eliminating accidental CRA over-contribution.

use crate::error::{Error, Result};
use crate::types::*;
use chrono::{DateTime, FixedOffset, NaiveDateTime};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::de::DeserializeOwned;

pub struct QuestradeClient {
    http: reqwest::Client,
    base_url: String,
    access_token: String,
}

impl QuestradeClient {
    pub fn new(http: reqwest::Client, api_server: String, access_token: String) -> Self {
        let base_url = if api_server.ends_with('/') {
            api_server
        } else {
            format!("{api_server}/")
        };
        QuestradeClient {
            http,
            base_url,
            access_token,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path.trim_start_matches('/'))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self
            .http
            .get(self.url(path))
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;
        Self::decode(resp).await
    }

    async fn decode<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        let body = resp.text().await.map_err(|e| Error::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                body,
            });
        }
        serde_json::from_str(&body).map_err(|e| Error::Api {
            status: status.as_u16(),
            body: format!("decode error: {e}"),
        })
    }

    /// `GET v1/accounts`
    pub async fn accounts(&self) -> Result<AccountsResponse> {
        self.get("v1/accounts").await
    }

    /// `GET v1/accounts/{id}/balances`
    pub async fn balances(&self, account: &str) -> Result<BalancesResponse> {
        self.get(&format!("v1/accounts/{account}/balances")).await
    }

    /// `GET v1/accounts/{id}/positions`
    pub async fn positions(&self, account: &str) -> Result<PositionsResponse> {
        self.get(&format!("v1/accounts/{account}/positions")).await
    }

    /// `GET v1/markets/quotes?ids=a,b,c`
    pub async fn quotes(&self, symbol_ids: &[u64]) -> Result<QuotesResponse> {
        let ids: Vec<String> = symbol_ids.iter().map(|i| i.to_string()).collect();
        self.get(&format!("v1/markets/quotes?ids={}", ids.join(",")))
            .await
    }

    /// `GET v1/markets/candles/{id}?startTime=...&endTime=...&interval=OneDay`
    pub async fn candles(
        &self,
        symbol_id: u64,
        start: DateTime<FixedOffset>,
        end: DateTime<FixedOffset>,
        interval: &str,
    ) -> Result<CandlesResponse> {
        let path = format!(
            "v1/markets/candles/{symbol_id}?startTime={start}&endTime={end}&interval={interval}"
        );
        self.get(&path).await
    }

    /// `GET v1/symbols/search?q=...` (used to discover symbol ids once).
    pub async fn search_symbols(&self, query: &str) -> Result<SymbolsSearchResponse> {
        self.get(&format!("v1/symbols/search?q={query}")).await
    }

    /// `POST v1/accounts/{id}/orders` - place a Day Limit buy at the bid.
    pub async fn place_buy_limit(
        &self,
        account: &str,
        symbol_id: u64,
        shares: u64,
        limit_price: Decimal,
    ) -> Result<()> {
        self.place_order(account, OrderAction::Buy, symbol_id, shares, limit_price)
            .await
    }

    /// `POST v1/accounts/{id}/orders` - place a Day Limit sell at the ask.
    pub async fn place_sell_limit(
        &self,
        account: &str,
        symbol_id: u64,
        shares: u64,
        limit_price: Decimal,
    ) -> Result<()> {
        self.place_order(account, OrderAction::Sell, symbol_id, shares, limit_price)
            .await
    }

    async fn place_order(
        &self,
        account: &str,
        action: OrderAction,
        symbol_id: u64,
        shares: u64,
        limit_price: Decimal,
    ) -> Result<()> {
        let order = OrderRequest {
            symbol_id,
            quantity: shares,
            limit_price,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Day,
            action,
            primary_route: "AUTO".to_string(),
            secondary_route: "AUTO".to_string(),
        };
        let resp = self
            .http
            .post(self.url(&format!("v1/accounts/{account}/orders")))
            .bearer_auth(&self.access_token)
            .json(&order)
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| Error::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                body,
            });
        }
        Ok(())
    }
}

/// `GET v1/symbols/search`
#[derive(Debug, Deserialize)]
pub struct SymbolsSearchResponse {
    pub symbols: Vec<SymbolDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolDto {
    #[serde(rename = "symbolId")]
    pub symbol_id: u64,
    pub symbol: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub security_type: String,
}

/// Format a [`NaiveDateTime`] (UTC) as Questrade's expected ISO-8601 input for
/// candle queries, e.g. `2026-07-09T00:00:00-04:00`.
pub fn qt_timestamp(naive_utc: NaiveDateTime) -> DateTime<FixedOffset> {
    DateTime::<FixedOffset>::from_naive_utc_and_offset(
        naive_utc,
        FixedOffset::west_opt(4 * 3600).expect("valid offset"),
    )
}
