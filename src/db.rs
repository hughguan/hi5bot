//! SQLite database layer for Hi5bot.
//!
//! Stores:
//! - Market signals (AAII, NAAIM, Market Breadth snapshots)
//! - Extreme zone history
//! - Order execution logs
//! - Backtest cache
//!
//! The database file lives at `<data_dir>/hi5bot.db` and uses WAL mode for
//! concurrent read safety with the web dashboard.

use crate::error::{Error, Result};
use chrono::NaiveDate;
use rusqlite::{Connection, params};
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Thin wrapper around a SQLite connection with WAL mode enabled.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open (or create) the database at `path`, run migrations, enable WAL.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())
            .map_err(|e| Error::Db(format!("open db {}: {e}", path.as_ref().display())))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| Error::Db(format!("pragma: {e}")))?;
        let db = Database {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Create tables if they don't exist.
    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS market_signals (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                date        TEXT NOT NULL UNIQUE,
                aaii_bulls  REAL,
                aaii_bears  REAL,
                naaim_exposure REAL,
                sp500_pct_above_200ma REAL,
                vix         REAL,
                extreme_zone TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS order_log (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                account     TEXT NOT NULL,
                ticker      TEXT NOT NULL,
                side        TEXT NOT NULL,  -- 'BUY' | 'SELL'
                shares      INTEGER NOT NULL,
                limit_price REAL NOT NULL,
                est_cost    REAL,
                signal      TEXT,
                placed_at   TEXT NOT NULL DEFAULT (datetime('now')),
                status      TEXT NOT NULL DEFAULT 'pending'  -- pending|filled|cancelled|failed
            );

            CREATE TABLE IF NOT EXISTS backtest_cache (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                strategy    TEXT NOT NULL,  -- 'hi5' | 'hi5e'
                start_date  TEXT NOT NULL,
                end_date    TEXT NOT NULL,
                cagr        REAL,
                max_drawdown REAL,
                sharpe      REAL,
                final_nav   REAL,
                raw_json    TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(strategy, start_date, end_date)
            );

            CREATE INDEX IF NOT EXISTS idx_market_signals_date ON market_signals(date);
            CREATE INDEX IF NOT EXISTS idx_order_log_account ON order_log(account);
            CREATE INDEX IF NOT EXISTS idx_order_log_placed ON order_log(placed_at);
            ",
        )
        .map_err(|e| Error::Db(format!("migrate: {e}")))?;
        Ok(())
    }

    // ---- Market Signals --------------------------------------------------

    pub fn insert_market_signal(&self, sig: &MarketSignalRecord) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO market_signals
             (date, aaii_bulls, aaii_bears, naaim_exposure, sp500_pct_above_200ma, vix, extreme_zone)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                sig.date.to_string(),
                sig.aaii_bulls,
                sig.aaii_bears,
                sig.naaim_exposure,
                sig.sp500_pct_above_200ma,
                sig.vix,
                sig.extreme_zone.as_deref(),
            ],
        )
        .map_err(|e| Error::Db(format!("insert market_signal: {e}")))?;
        Ok(())
    }

    pub fn latest_market_signal(&self) -> Result<Option<MarketSignalRecord>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT date, aaii_bulls, aaii_bears, naaim_exposure,
                        sp500_pct_above_200ma, vix, extreme_zone
                 FROM market_signals ORDER BY date DESC LIMIT 1",
            )
            .map_err(|e| Error::Db(format!("prepare: {e}")))?;
        let row = stmt
            .query_row([], |row| {
                Ok(MarketSignalRecord {
                    date: NaiveDate::parse_from_str(&row.get::<_, String>(0)?, "%Y-%m-%d")
                        .ok()
                        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()),
                    aaii_bulls: row.get(1)?,
                    aaii_bears: row.get(2)?,
                    naaim_exposure: row.get(3)?,
                    sp500_pct_above_200ma: row.get(4)?,
                    vix: row.get(5)?,
                    extreme_zone: row.get(6)?,
                })
            })
            .optional()
            .map_err(|e| Error::Db(format!("query: {e}")))?;
        Ok(row)
    }

    pub fn market_signals_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<MarketSignalRecord>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT date, aaii_bulls, aaii_bears, naaim_exposure,
                        sp500_pct_above_200ma, vix, extreme_zone
                 FROM market_signals
                 WHERE date >= ?1 AND date <= ?2
                 ORDER BY date ASC",
            )
            .map_err(|e| Error::Db(format!("prepare: {e}")))?;
        let rows = stmt
            .query_map(params![start.to_string(), end.to_string()], |row| {
                Ok(MarketSignalRecord {
                    date: NaiveDate::parse_from_str(&row.get::<_, String>(0)?, "%Y-%m-%d")
                        .ok()
                        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()),
                    aaii_bulls: row.get(1)?,
                    aaii_bears: row.get(2)?,
                    naaim_exposure: row.get(3)?,
                    sp500_pct_above_200ma: row.get(4)?,
                    vix: row.get(5)?,
                    extreme_zone: row.get(6)?,
                })
            })
            .map_err(|e| Error::Db(format!("query: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Db(format!("row: {e}")))?);
        }
        Ok(out)
    }

    // ---- Order Log -------------------------------------------------------

    pub fn log_order(&self, order: &OrderLogEntry) -> Result<i64> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO order_log (account, ticker, side, shares, limit_price, est_cost, signal, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                order.account,
                order.ticker,
                order.side,
                order.shares,
                order.limit_price,
                order.est_cost,
                order.signal.as_deref(),
                order.status.as_deref().unwrap_or("pending"),
            ],
        )
        .map_err(|e| Error::Db(format!("insert order_log: {e}")))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn recent_orders(&self, account: Option<&str>, limit: u32) -> Result<Vec<OrderLogEntry>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let sql = if account.is_some() {
            "SELECT id, account, ticker, side, shares, limit_price, est_cost, signal, placed_at, status
             FROM order_log WHERE account = ?1 ORDER BY id DESC LIMIT ?2"
        } else {
            "SELECT id, account, ticker, side, shares, limit_price, est_cost, signal, placed_at, status
             FROM order_log ORDER BY id DESC LIMIT ?1"
        };
        let mut stmt = conn.prepare(sql).map_err(|e| Error::Db(format!("prepare: {e}")))?;
        let rows = if let Some(acct) = account {
            stmt.query_map(params![acct, limit], row_to_order)?
        } else {
            stmt.query_map(params![limit], row_to_order)?
        };
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| Error::Db(format!("row: {e}")))?);
        }
        Ok(out)
    }

    // ---- Backtest Cache --------------------------------------------------

    pub fn get_backtest(
        &self,
        strategy: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Option<BacktestCacheEntry>> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT strategy, start_date, end_date, cagr, max_drawdown, sharpe, final_nav, raw_json
                 FROM backtest_cache
                 WHERE strategy = ?1 AND start_date = ?2 AND end_date = ?3",
            )
            .map_err(|e| Error::Db(format!("prepare: {e}")))?;
        let row = stmt.query_row(params![strategy, start.to_string(), end.to_string()], |row| {
            Ok(BacktestCacheEntry {
                strategy: row.get(0)?,
                start_date: row.get::<_, String>(1).ok().and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
                end_date: row.get::<_, String>(2).ok().and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
                cagr: row.get(3)?,
                max_drawdown: row.get(4)?,
                sharpe: row.get(5)?,
                final_nav: row.get(6)?,
                raw_json: row.get(7)?,
            })
        }).optional().map_err(|e| Error::Db(format!("query: {e}")))?;
        Ok(row)
    }

    pub fn upsert_backtest(&self, entry: &BacktestCacheEntry) -> Result<()> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT OR REPLACE INTO backtest_cache
             (strategy, start_date, end_date, cagr, max_drawdown, sharpe, final_nav, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.strategy,
                entry.start_date.map(|d| d.to_string()).unwrap_or_default(),
                entry.end_date.map(|d| d.to_string()).unwrap_or_default(),
                entry.cagr,
                entry.max_drawdown,
                entry.sharpe,
                entry.final_nav,
                entry.raw_json,
            ],
        )
        .map_err(|e| Error::Db(format!("upsert backtest: {e}")))?;
        Ok(())
    }
}

fn row_to_order(row: &rusqlite::Row<'_>) -> rusqlite::Result<OrderLogEntry> {
    Ok(OrderLogEntry {
        id: row.get(0)?,
        account: row.get(1)?,
        ticker: row.get(2)?,
        side: row.get(3)?,
        shares: row.get(4)?,
        limit_price: row.get(5)?,
        est_cost: row.get(6)?,
        signal: row.get(7)?,
        placed_at: row.get(8)?,
        status: row.get(9)?,
    })
}

// ---- Data transfer types ------------------------------------------------

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketSignalRecord {
    pub date: NaiveDate,
    pub aaii_bulls: Option<f64>,
    pub aaii_bears: Option<f64>,
    pub naaim_exposure: Option<f64>,
    pub sp500_pct_above_200ma: Option<f64>,
    pub vix: Option<f64>,
    #[serde(default)]
    pub extreme_zone: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OrderLogEntry {
    #[serde(default)]
    pub id: i64,
    pub account: String,
    pub ticker: String,
    pub side: String,
    pub shares: u64,
    pub limit_price: f64,
    #[serde(default)]
    pub est_cost: Option<f64>,
    #[serde(default)]
    pub signal: Option<String>,
    #[serde(default)]
    pub placed_at: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BacktestCacheEntry {
    pub strategy: String,
    #[serde(default)]
    pub start_date: Option<NaiveDate>,
    #[serde(default)]
    pub end_date: Option<NaiveDate>,
    #[serde(default)]
    pub cagr: Option<f64>,
    #[serde(default)]
    pub max_drawdown: Option<f64>,
    #[serde(default)]
    pub sharpe: Option<f64>,
    #[serde(default)]
    pub final_nav: Option<f64>,
    #[serde(default)]
    pub raw_json: Option<String>,
}

/// Resolve the database path from the data directory.
pub fn db_path() -> PathBuf {
    crate::config::data_dir().join("hi5bot.db")
}
