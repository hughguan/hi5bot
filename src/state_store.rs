//! Atomic per-account monthly trade counter (spec §3 - max 3 buys/month).
//!
//! Persists `state.json` with the same crash-safe temp+fsync+rename pattern as
//! [`crate::auth`]. The annual rebalance does *not* touch this counter (it
//! disregards monthly limits); only incremental buys increment it.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct PersistedState {
    #[serde(default)]
    pub accounts: HashMap<String, AccountEntry>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct AccountEntry {
    pub year: i32,
    pub month: u32,
    pub trade_count: u32,
}

pub struct MonthlyStateStore {
    path: PathBuf,
    data: Mutex<PersistedState>,
}

impl MonthlyStateStore {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(MonthlyStateStore {
                path,
                data: Mutex::new(PersistedState::default()),
            });
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::State(format!("read state {}: {e}", path.display())))?;
        let data: PersistedState = if text.trim().is_empty() {
            PersistedState::default()
        } else {
            serde_json::from_str(&text).map_err(|e| Error::State(format!("parse state: {e}")))?
        };
        Ok(MonthlyStateStore {
            path,
            data: Mutex::new(data),
        })
    }

    /// Effective trade count for the current month: the stored count if the
    /// stored (year, month) matches, else 0 (a new month has rolled over).
    pub fn effective(&self, account: &str, year: i32, month: u32) -> u32 {
        let data = self.data.lock().expect("state mutex poisoned");
        match data.accounts.get(account) {
            Some(e) if e.year == year && e.month == month => e.trade_count,
            _ => 0,
        }
    }

    /// Record one executed buy for `account` in (year, month), rolling over on
    /// a new month. Persists atomically. Returns the new trade count.
    pub fn record_trade(&self, account: &str, year: i32, month: u32) -> Result<u32> {
        let mut data = self.data.lock().expect("state mutex poisoned");
        let entry = data
            .accounts
            .entry(account.to_string())
            .or_insert(AccountEntry {
                year,
                month,
                trade_count: 0,
            });
        if entry.year != year || entry.month != month {
            entry.year = year;
            entry.month = month;
            entry.trade_count = 1;
        } else {
            entry.trade_count += 1;
        }
        let new_count = entry.trade_count;
        save_atomic(&self.path, &data)?;
        Ok(new_count)
    }
}

fn save_atomic(path: &Path, data: &PersistedState) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("state.json");
    let json =
        serde_json::to_string_pretty(data).map_err(|e| Error::State(format!("serialize: {e}")))?;
    let tmp = dir.join(format!(".{file_name}.tmp"));
    {
        use std::io::Write;
        let mut file =
            std::fs::File::create(&tmp).map_err(|e| Error::State(format!("create tmp: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        file.write_all(json.as_bytes())
            .map_err(|e| Error::State(format!("write tmp: {e}")))?;
        file.sync_all()
            .map_err(|e| Error::State(format!("fsync tmp: {e}")))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| Error::State(format!("rename: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_trade_increments_within_month() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let s = MonthlyStateStore::load(&path).unwrap();
        assert_eq!(s.effective("ACC", 2026, 7), 0);
        assert_eq!(s.record_trade("ACC", 2026, 7).unwrap(), 1);
        assert_eq!(s.record_trade("ACC", 2026, 7).unwrap(), 2);
        assert_eq!(s.effective("ACC", 2026, 7), 2);
    }

    #[test]
    fn record_trade_rolls_over_on_new_month() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let s = MonthlyStateStore::load(&path).unwrap();
        s.record_trade("ACC", 2026, 7).unwrap();
        s.record_trade("ACC", 2026, 7).unwrap();
        // August -> counter resets to 0 effective, then becomes 1 after a trade.
        assert_eq!(s.effective("ACC", 2026, 8), 0);
        assert_eq!(s.record_trade("ACC", 2026, 8).unwrap(), 1);
    }

    #[test]
    fn reload_preserves_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        {
            let s = MonthlyStateStore::load(&path).unwrap();
            s.record_trade("ACC", 2026, 7).unwrap();
            s.record_trade("ACC", 2026, 7).unwrap();
        }
        let s = MonthlyStateStore::load(&path).unwrap();
        assert_eq!(s.effective("ACC", 2026, 7), 2);
    }

    #[test]
    fn per_account_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let s = MonthlyStateStore::load(&path).unwrap();
        s.record_trade("RESP", 2026, 7).unwrap();
        s.record_trade("TFSA", 2026, 7).unwrap();
        s.record_trade("TFSA", 2026, 7).unwrap();
        assert_eq!(s.effective("RESP", 2026, 7), 1);
        assert_eq!(s.effective("TFSA", 2026, 7), 2);
    }
}
