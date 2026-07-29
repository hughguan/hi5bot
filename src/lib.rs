//! Hi5-Bot: unattended mechanical asset-allocation daemon for Questrade.
//!
//! Module map:
//! - [`types`]   — domain types, Decimal money newtypes, Questrade DTOs
//! - [`error`]   — typed errors
//! - [`config`]  — TOML settings + data-dir resolution
//! - [`auth`]    — OAuth2 token store (atomic, crash-safe) + refresh handshake
//! - [`buffer_pool`] — Dynamic Buffer Pool sizing + Fill-the-Gap allocation
//! - [`strategy`]    — calendar-month state machine (5 signals)
//! - [`calendar`]    — trading-calendar helpers + 15:30 ET wake scheduling
//! - [`questrade`]   — async REST client (accounts/balances/positions/quotes/candles/orders)
//! - [`state_store`] — atomic per-account monthly trade counter
//! - [`notify`]      — hard-abort webhook
//! - [`engine`]      — single evaluation tick orchestration
//! - [`db`]          — SQLite database (market signals, order log, backtest cache)
//! - [`radar`]       — Market Radar extreme zone detection (AAII/NAAIM/breadth)
//! - [`backtest`]    — Hi5 vs Hi5e backtest engine
//! - [`web`]         — Axum REST API server for the dashboard

//! - [`accounts`]     — Questrade API-driven account discovery (replaces hardcoded RESP/TFSA)

pub mod accounts;
pub mod auth;
pub mod backtest;
pub mod buffer_pool;
pub mod calendar;
pub mod config;
pub mod db;
pub mod engine;
pub mod error;
pub mod fetcher;
pub mod notify;
pub mod questrade;
pub mod radar;
pub mod state_store;
pub mod strategy;
pub mod types;
pub mod web;

pub use error::{Error, Result};
