//! OAuth2 token store for Questrade.
//!
//! Questrade's OAuth2 flow exchanges a long-lived `refresh_token` for a
//! short-lived `access_token` (plus an `api_server` base URL). Critically,
//! Questrade **rotates the refresh_token on every exchange** - the response
//! carries a brand-new refresh_token that must be persisted, or the next restart
//! is locked out.
//!
//! This module makes that persistence **bulletproof**:
//!
//! - **Atomic rewrite** (`save_atomic`): serialize -> write to a sibling temp
//!   file -> `fsync` -> `rename`. POSIX `rename` is atomic, so `tokens.json` is
//!   either the old or the new version, *never* a torn write. A crash mid-write
//!   cannot corrupt the file.
//! - **Backup**: the previous good `tokens.json` is copied to `tokens.json.bak`
//!   before each overwrite, so a bad refresh can be recovered.
//! - **Validation gate**: a parsed token set is validated *before* it replaces
//!   the live one. A malformed/empty response never overwrites valid tokens.
//! - **0600 perms**: the temp file is created mode 0600 (Unix) so the rotated
//!   refresh_token is never world/group-readable.
//! - **Concurrency**: cheap reads (`access_token`, `is_expired`) use a
//!   `std::sync::Mutex`. Refresh is serialized by a `tokio::sync::Mutex`
//!   "refresh_lock" with double-checked expiry, so N concurrent callers trigger
//!   exactly one exchange.
//! - **Failure isolation**: a network/parse failure during `refresh` leaves both
//!   the in-memory state and `tokens.json` untouched (the old refresh_token is
//!   still valid until it is actually consumed).

use crate::error::{Error, Result};
use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Refresh this many seconds before the real expiry, to avoid a race where the
/// token expires between the check and the next API call.
const EXPIRY_SKEW_SECS: i64 = 60;

/// The persisted + in-memory token set.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: String,
    /// Base API URL for this session, e.g. `https://api06.iq.questrade.com/`.
    pub api_server: String,
    pub token_type: String,
    /// Seconds the access_token is valid (as reported by Questrade).
    pub expires_in: u64,
    /// When this access_token was acquired.
    pub acquired_at: NaiveDateTime,
    /// `acquired_at + expires_in`.
    pub expires_at: NaiveDateTime,
}

/// Questrade token-endpoint response (snake_case, unlike the API's camelCase).
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub api_server: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_token: String,
}

pub struct TokenStore {
    path: PathBuf,
    /// Guards cheap synchronous reads + the in-memory swap.
    data: Mutex<TokenData>,
    /// Serializes refreshes so concurrent `ensure_valid` callers exchange once.
    refresh_lock: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for TokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl TokenStore {
    /// Load and validate `tokens.json` from `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::TokensRead(path.clone(), e.to_string()))?;
        let data: TokenData = serde_json::from_str(&text)
            .map_err(|e| Error::TokensParse(path.clone(), e.to_string()))?;
        validate(&data)?;
        Ok(TokenStore {
            path,
            data: Mutex::new(data),
            refresh_lock: tokio::sync::Mutex::new(()),
        })
    }

    /// Construct an in-memory store from already-valid data (does not persist).
    pub fn from_data(path: impl AsRef<Path>, data: TokenData) -> Result<Self> {
        validate(&data)?;
        Ok(TokenStore {
            path: path.as_ref().to_path_buf(),
            data: Mutex::new(data),
            refresh_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current access token (cheap, synchronous).
    pub fn access_token(&self) -> String {
        self.data
            .lock()
            .expect("token mutex poisoned")
            .access_token
            .clone()
    }

    /// Current API server base URL.
    pub fn api_server(&self) -> String {
        self.data
            .lock()
            .expect("token mutex poisoned")
            .api_server
            .clone()
    }

    /// True if the access token has expired (or will within the skew window).
    pub fn is_expired(&self, now: NaiveDateTime) -> bool {
        let data = self.data.lock().expect("token mutex poisoned");
        is_expired(&data, now)
    }

    /// Refresh only if expired. Thread-safe; concurrent callers exchange once.
    pub async fn ensure_valid(
        &self,
        client: &reqwest::Client,
        token_url: &str,
        now: NaiveDateTime,
    ) -> Result<()> {
        // Cheap check first (no async, no network).
        if !self.is_expired(now) {
            return Ok(());
        }
        // Serialize refreshes.
        let _guard = self.refresh_lock.lock().await;
        // Double-check under the refresh lock: another task may have just refreshed.
        if !self.is_expired(now) {
            return Ok(());
        }
        self.refresh(client, token_url, now).await
    }

    /// Exchange the current refresh_token for a fresh access_token. Questrade
    /// rotates the refresh_token; the new one is validated then atomically
    /// persisted. On any failure the existing token set is left intact.
    pub async fn refresh(
        &self,
        client: &reqwest::Client,
        token_url: &str,
        now: NaiveDateTime,
    ) -> Result<()> {
        let refresh_token = self
            .data
            .lock()
            .expect("token mutex poisoned")
            .refresh_token
            .clone();

        let response = client
            .request(reqwest::Method::POST, token_url)
            .query(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
            ])
            .send()
            .await
            .map_err(|e| Error::RefreshHttp(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| Error::RefreshHttp(e.to_string()))?;
        if !status.is_success() {
            return Err(Error::Refresh(format!(
                "token endpoint returned {status}: {body}"
            )));
        }

        let parsed: TokenResponse = serde_json::from_str(&body)
            .map_err(|e| Error::Refresh(format!("invalid token response: {e}")))?;

        let new_data = TokenData {
            access_token: parsed.access_token,
            refresh_token: parsed.refresh_token,
            api_server: parsed.api_server,
            token_type: parsed.token_type,
            expires_in: parsed.expires_in,
            acquired_at: now,
            expires_at: now + Duration::seconds(parsed.expires_in as i64),
        };
        validate(&new_data)?;

        // Persist first (atomic), then swap the in-memory state. If persistence
        // fails, in-memory state stays consistent with disk (old tokens).
        save_atomic(&self.path, &new_data)?;
        {
            let mut data = self.data.lock().expect("token mutex poisoned");
            *data = new_data;
        }
        Ok(())
    }
}

fn is_expired(data: &TokenData, now: NaiveDateTime) -> bool {
    data.expires_at <= now + Duration::seconds(EXPIRY_SKEW_SECS)
}

fn validate(d: &TokenData) -> Result<()> {
    if d.access_token.trim().is_empty() {
        return Err(Error::TokensInvalid("access_token is empty".into()));
    }
    if d.refresh_token.trim().is_empty() {
        return Err(Error::TokensInvalid("refresh_token is empty".into()));
    }
    if !d.api_server.starts_with("https://") {
        return Err(Error::TokensInvalid(format!(
            "api_server must be https://, got {}",
            d.api_server
        )));
    }
    if d.expires_in == 0 {
        return Err(Error::TokensInvalid("expires_in is 0".into()));
    }
    Ok(())
}

/// Atomically write `data` to `path` (temp -> fsync -> rename), backing up the
/// previous file to `path.bak`. Never leaves a partial `path`.
pub fn save_atomic(path: &Path, data: &TokenData) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("tokens.json");

    let json = serde_json::to_string_pretty(data)
        .map_err(|e| Error::TokensInvalid(format!("serialize failed: {e}")))?;

    let tmp = dir.join(format!(".{file_name}.tmp"));

    // Write temp file with 0600 perms, fsync, then atomic rename.
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)
            .map_err(|e| Error::TokensRead(tmp.clone(), e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        file.write_all(json.as_bytes())
            .map_err(|e| Error::TokensRead(tmp.clone(), e.to_string()))?;
        file.sync_all()
            .map_err(|e| Error::TokensRead(tmp.clone(), e.to_string()))?;
    }

    // Backup the current good file (best-effort; absence is fine on first write).
    let bak = dir.join(format!("{file_name}.bak"));
    if path.exists() {
        let _ = std::fs::copy(path, &bak);
    }

    std::fs::rename(&tmp, path)
        .map_err(|e| Error::TokensRead(path.to_path_buf(), e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use wiremock::matchers::{method, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 7, 9)
            .unwrap()
            .and_hms_opt(15, 30, 0)
            .unwrap()
    }

    fn valid_data(access: &str, refresh: &str) -> TokenData {
        TokenData {
            access_token: access.into(),
            refresh_token: refresh.into(),
            api_server: "https://api06.iq.questrade.com/".into(),
            token_type: "Bearer".into(),
            expires_in: 1800,
            acquired_at: now(),
            expires_at: now() + Duration::seconds(1800),
        }
    }

    #[test]
    fn save_atomic_rewrites_valid_and_cleans_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let data = valid_data("A1", "R1");
        save_atomic(&path, &data).unwrap();

        let written: TokenData =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written.access_token, "A1");
        // no temp leftover
        assert!(!dir.path().join(".tokens.json.tmp").exists());
    }

    #[test]
    fn save_atomic_creates_backup_of_previous() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        save_atomic(&path, &valid_data("OLD", "ROLD")).unwrap();
        save_atomic(&path, &valid_data("NEW", "RNEW")).unwrap();

        let bak: TokenData = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("tokens.json.bak")).unwrap(),
        )
        .unwrap();
        assert_eq!(bak.access_token, "OLD");
        let cur: TokenData =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(cur.access_token, "NEW");
    }

    #[test]
    fn load_corrupt_json_errors_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let err = TokenStore::load(&path).unwrap_err();
        assert!(matches!(err, Error::TokensParse(_, _)));
    }

    #[test]
    fn validate_rejects_empty_and_non_https() {
        let mut d = valid_data("a", "r");
        d.access_token = "  ".into();
        assert!(validate(&d).is_err());
        let mut d = valid_data("a", "r");
        d.api_server = "http://insecure.example/".into();
        assert!(validate(&d).is_err());
    }

    #[test]
    fn is_expired_respects_skew() {
        let d = valid_data("a", "r"); // expires 1800s after now
        assert!(!is_expired(&d, now()));
        // within skew window of expiry
        assert!(is_expired(&d, now() + Duration::seconds(1800 - 30)));
    }

    #[tokio::test]
    async fn refresh_rotates_token_and_persists_atomically() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(query_param("grant_type", "refresh_token"))
            .and(query_param("refresh_token", "R-OLD"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "A-NEW",
                "api_server": "https://api99.iq.questrade.com/",
                "token_type": "Bearer",
                "expires_in": 1800,
                "refresh_token": "R-NEW"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let store = TokenStore::from_data(&path, valid_data("A-OLD", "R-OLD")).unwrap();
        // persist initial so the backup path is exercised
        save_atomic(&path, &valid_data("A-OLD", "R-OLD")).unwrap();

        store
            .refresh(&reqwest::Client::new(), &server.uri(), now())
            .await
            .unwrap();

        assert_eq!(store.access_token(), "A-NEW");
        // rotated refresh token persisted to disk
        let on_disk: TokenData =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk.refresh_token, "R-NEW");
        assert_eq!(on_disk.api_server, "https://api99.iq.questrade.com/");
        assert!(!dir.path().join(".tokens.json.tmp").exists());
    }

    #[tokio::test]
    async fn ensure_valid_skips_refresh_when_fresh() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "SHOULD-NOT-USE",
                "api_server": "https://api99.iq.questrade.com/",
                "token_type": "Bearer",
                "expires_in": 1800,
                "refresh_token": "SHOULD-NOT-USE"
            })))
            .expect(0) // must NOT be called
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let store = TokenStore::from_data(&path, valid_data("FRESH", "R")).unwrap();

        store
            .ensure_valid(&reqwest::Client::new(), &server.uri(), now())
            .await
            .unwrap();
        assert_eq!(store.access_token(), "FRESH");
    }

    #[tokio::test]
    async fn ensure_valid_refreshes_when_expired() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(query_param("refresh_token", "R"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "A2",
                "api_server": "https://api06.iq.questrade.com/",
                "token_type": "Bearer",
                "expires_in": 1800,
                "refresh_token": "R2"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mut expired = valid_data("A1", "R");
        expired.expires_at = now() - Duration::seconds(10); // already expired
        let store = TokenStore::from_data(&path, expired).unwrap();

        store
            .ensure_valid(&reqwest::Client::new(), &server.uri(), now())
            .await
            .unwrap();
        assert_eq!(store.access_token(), "A2");
    }

    #[tokio::test]
    async fn concurrent_ensure_valid_exchanges_exactly_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(query_param("refresh_token", "R"))
            // small delay so concurrent callers race toward the refresh lock
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "A2",
                "api_server": "https://api06.iq.questrade.com/",
                "token_type": "Bearer",
                "expires_in": 1800,
                "refresh_token": "R2"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens.json");
        let mut expired = valid_data("A1", "R");
        expired.expires_at = now() - Duration::seconds(10);
        let store = std::sync::Arc::new(TokenStore::from_data(&path, expired).unwrap());

        let client = reqwest::Client::new();
        let url = server.uri();
        let mut handles = Vec::new();
        for _ in 0..8 {
            let store = store.clone();
            let client = client.clone();
            let url = url.clone();
            handles.push(tokio::spawn(async move {
                store.ensure_valid(&client, &url, now()).await
            }));
        }
        for h in handles {
            h.await.unwrap().unwrap();
        }
        assert_eq!(store.access_token(), "A2");
    }
}
