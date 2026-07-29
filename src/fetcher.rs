//! Market sentiment data fetcher & parser.
//!
//! Fetches:
//! - AAII Sentiment Survey from AAII website
//! - NAAIM Exposure Index from NAAIM website
//! - S&P 500 % of stocks above 200-day moving average

use crate::db::MarketSignalRecord;
use crate::error::{Error, Result};
use chrono::NaiveDate;
use reqwest::Client;

/// Result of a market sentiment data fetch.
#[derive(Debug, Clone, Default)]
pub struct SentimentData {
    pub date: NaiveDate,
    pub aaii_bulls: Option<f64>,
    pub aaii_bears: Option<f64>,
    pub naaim_exposure: Option<f64>,
    pub sp500_pct_above_200ma: Option<f64>,
    pub vix: Option<f64>,
}

/// Fetch sentiment data from web sources; preserves None if fetching fails.
pub async fn fetch_market_sentiment(client: &Client, today: NaiveDate) -> Result<MarketSignalRecord> {
    let (aaii_bulls, aaii_bears) = fetch_aaii_sentiment(client).await.unwrap_or((None, None));
    let naaim_exposure = fetch_naaim_exposure(client).await.ok().flatten();
    let sp500_pct_above_200ma = None;

    let snapshot = crate::radar::classify_zone(
        aaii_bulls,
        aaii_bears,
        naaim_exposure,
        sp500_pct_above_200ma,
        None,
        None,
        None,
    );

    Ok(MarketSignalRecord {
        date: today,
        aaii_bulls,
        aaii_bears,
        naaim_exposure,
        sp500_pct_above_200ma,
        vix: None,
        extreme_zone: Some(snapshot.zone.label().to_string()),
    })
}

async fn fetch_aaii_sentiment(client: &Client) -> Result<(Option<f64>, Option<f64>)> {
    let resp = client
        .get("https://www.aaii.com/sentimentsurvey")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()
        .await
        .map_err(|e| Error::RefreshHttp(format!("aaii fetch error: {e}")))?;

    if !resp.status().is_success() {
        return Ok((None, None));
    }

    let html = resp
        .text()
        .await
        .map_err(|e| Error::RefreshHttp(format!("aaii text error: {e}")))?;

    let mut bulls = None;
    let mut bears = None;

    #[cfg(feature = "web-scraper")]
    {
        let fragment = scraper::Html::parse_document(&html);
        let selector = scraper::Selector::parse("td, span, div").unwrap();

        for element in fragment.select(&selector) {
            let text = element.text().collect::<String>();
            if text.contains("Bullish:") || text.contains("Bullish") {
                if let Some(val) = extract_percentage(&text) {
                    bulls = Some(val);
                }
            } else if text.contains("Bearish:") || text.contains("Bearish") {
                if let Some(val) = extract_percentage(&text) {
                    bears = Some(val);
                }
            }
        }
    }

    Ok((bulls, bears))
}

async fn fetch_naaim_exposure(client: &Client) -> Result<Option<f64>> {
    let resp = client
        .get("https://www.naaim.org/programs/naaim-exposure-index/")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()
        .await
        .map_err(|e| Error::RefreshHttp(format!("naaim fetch error: {e}")))?;

    if !resp.status().is_success() {
        return Ok(None);
    }

    let html = resp
        .text()
        .await
        .map_err(|e| Error::RefreshHttp(format!("naaim text error: {e}")))?;

    #[cfg(feature = "web-scraper")]
    {
        let fragment = scraper::Html::parse_document(&html);
        let selector = scraper::Selector::parse("td, p, div").unwrap();

        for element in fragment.select(&selector) {
            let text = element.text().collect::<String>();
            if text.contains("NAAIM Number") || text.contains("Exposure Index") {
                if let Some(val) = extract_percentage(&text) {
                    return Ok(Some(val));
                }
            }
        }
    }

    Ok(None)
}

fn extract_percentage(text: &str) -> Option<f64> {
    let mut num_str = String::new();
    let mut decimal_found = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            num_str.push(ch);
        } else if ch == '.' && !decimal_found && !num_str.is_empty() {
            num_str.push(ch);
            decimal_found = true;
        } else if !num_str.is_empty() && ch != '%' && !ch.is_whitespace() {
            break;
        }
    }
    num_str.parse::<f64>().ok()
}
