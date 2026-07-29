//! Configuration loading + data-directory resolution.
//!
//! The daemon resolves a single data directory from `$HI5BOT_DATA_DIR` (default
//! `./data`) and looks for `config.toml`, `tokens.json`, and `state.json`
//! underneath it. This lets the Synology container mount exactly one volume.
//!
//! ## Account discovery
//!
//! Instead of hardcoding account numbers, the daemon calls Questrade
//! `GET v1/accounts` at startup, filters by `account_types`, and locks in
//! the discovered set. See [`crate::accounts::discover`].

use crate::error::{Error, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    /// Questrade account types to include (case-insensitive).
    /// Common values: "RESP", "TFSA", "RRSP", "Margin", "Cash", "LIRA".
    /// Defaults to ["RESP", "TFSA"] if empty.
    #[serde(default = "default_account_types")]
    pub account_types: Vec<String>,
    /// VIX index symbol id (for the ExtremePanic signal).
    pub vix_symbol_id: u64,
    /// ETF symbol ids keyed by uppercase ticker.
    #[serde(default)]
    pub symbol_ids: HashMap<String, u64>,
    /// Rolling month safety buffer `M`. Defaults to 10.
    #[serde(default = "default_m")]
    pub safety_buffer_m: u32,
    /// Questrade OAuth2 token endpoint.
    #[serde(default = "default_token_url")]
    pub token_url: String,
    /// Required settlement preference; anything other than
    /// "Currency of Transaction" triggers a hard-abort.
    #[serde(default = "default_settlement")]
    pub settlement_pref: String,
    /// Daily evaluation time (HH:MM) in America/Toronto.
    #[serde(default = "default_eval_time")]
    pub eval_time: String,
    /// Optional webhook for hard-abort notifications.
    #[serde(default)]
    pub notify_webhook: String,
}

fn default_account_types() -> Vec<String> {
    vec!["RESP".to_string(), "TFSA".to_string()]
}
fn default_m() -> u32 {
    10
}
fn default_token_url() -> String {
    "https://login.questrade.com/oauth2/token".to_string()
}
fn default_settlement() -> String {
    "Currency of Transaction".to_string()
}
fn default_eval_time() -> String {
    "15:30".to_string()
}

impl Settings {
    /// Load from the default path (`<data_dir>/config.toml`).
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path())
    }

    /// Load from an explicit path.
    pub fn load_from(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| Error::ConfigRead(path.to_path_buf(), e.to_string()))?;
        let settings: Settings =
            toml::from_str(&text).map_err(|e| Error::ConfigParse(e.to_string()))?;
        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<()> {
        if self.safety_buffer_m == 0 {
            return Err(Error::ConfigParse(
                "safety_buffer_m must be > 0".to_string(),
            ));
        }
        for ticker in crate::types::Ticker::ALL {
            let key = ticker.as_str();
            if !self.symbol_ids.contains_key(key) {
                return Err(Error::ConfigParse(format!(
                    "symbol_ids missing entry for {key}"
                )));
            }
        }
        Ok(())
    }

    /// The symbol id for a portfolio ticker, or an error if unconfigured.
    pub fn symbol_id(&self, ticker: crate::types::Ticker) -> Result<u64> {
        self.symbol_ids
            .get(ticker.as_str())
            .copied()
            .ok_or_else(|| Error::ConfigParse(format!("missing symbol id for {ticker}")))
    }
}

// ---- data-directory resolution --------------------------------------------

/// The data directory: `$HI5BOT_DATA_DIR` or `./data`.
pub fn data_dir() -> PathBuf {
    std::env::var("HI5BOT_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data"))
}

pub fn tokens_path() -> PathBuf {
    data_dir().join("tokens.json")
}

pub fn state_path() -> PathBuf {
    data_dir().join("state.json")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_toml() -> &'static str {
        r#"
account_types = ["RESP", "TFSA"]
vix_symbol_id = 999
safety_buffer_m = 10
settlement_pref = "Currency of Transaction"
token_url = "https://login.questrade.com/oauth2/token"
eval_time = "15:30"
notify_webhook = ""

[symbol_ids]
IWY = 1
SPMO = 2
RSP = 3
PFF = 4
VNQ = 5
"#
    }

    #[test]
    fn parses_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, sample_toml()).unwrap();
        let s = Settings::load_from(&path).expect("valid config");
        assert_eq!(s.account_types, vec!["RESP", "TFSA"]);
        assert_eq!(s.symbol_id(crate::types::Ticker::Rsp).unwrap(), 3);
        assert_eq!(s.safety_buffer_m, 10);
    }

    #[test]
    fn defaults_account_types_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, sample_toml().replace("account_types", "#account_types")).unwrap();
        let s = Settings::load_from(&path).expect("valid config");
        assert_eq!(s.account_types, vec!["RESP", "TFSA"]);
    }

    #[test]
    fn rejects_missing_symbol_id() {
        let toml = r#"
account_types = ["RESP"]
vix_symbol_id = 0
[symbol_ids]
IWY = 1
SPMO = 2
RSP = 3
PFF = 4
# VNQ missing
"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, toml).unwrap();
        let err = Settings::load_from(&path).unwrap_err();
        assert!(matches!(err, Error::ConfigParse(_)));
    }

    #[test]
    fn rejects_zero_buffer() {
        let mut t = sample_toml().to_string();
        t = t.replace("safety_buffer_m = 10", "safety_buffer_m = 0");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, t).unwrap();
        assert!(Settings::load_from(&path).is_err());
    }
}
