//! Typed errors for Hi5-Bot.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read config at {0}: {1}")]
    ConfigRead(PathBuf, String),

    #[error("failed to parse config: {0}")]
    ConfigParse(String),

    #[error("failed to read tokens at {0}: {1}")]
    TokensRead(PathBuf, String),

    #[error("failed to parse tokens at {0}: {1}")]
    TokensParse(PathBuf, String),

    #[error("tokens invalid: {0}")]
    TokensInvalid(String),

    #[error("token refresh HTTP error: {0}")]
    RefreshHttp(String),

    #[error("token refresh failed: {0}")]
    Refresh(String),

    #[error("questrade api error (status {status}): {body}")]
    Api { status: u16, body: String },

    #[error("questrade http error: {0}")]
    Http(String),

    #[error("currency hard-lock violation: {0}")]
    CurrencyLock(String),

    #[error("USD cash exhausted (<= 0) before a trade; hard-abort triggered")]
    UsdCashExhausted,

    #[error("settlement preference is '{0}', must be 'Currency of Transaction'")]
    SettlementNotCurrencyOfTransaction(String),

    #[error("missing price/quote for ticker {0}")]
    MissingPrice(String),

    #[error("state store error: {0}")]
    State(String),

    #[error("database error: {0}")]
    Db(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
