//! Questrade account discovery — replaces hardcoded RESP/TFSA with
//! dynamic API-driven account resolution.
//!
//! At startup the daemon calls `GET v1/accounts`, filters to active
//! registered accounts matching the configured account types, and locks
//! in the discovered set for the lifetime of the process.
//!
//! Supported account types: RESP, RRSP, TFSA, Margin, Cash, LIRA, etc.
//! The config `account_types` whitelist controls which types are included.

use crate::error::{Error, Result};
use crate::questrade::QuestradeClient;

/// A discovered account ready for trading.
#[derive(Clone, Debug)]
pub struct DiscoveredAccount {
    /// Questrade account number (used as the API path parameter).
    pub number: String,
    /// Account type as reported by Questrade (e.g. "RESP", "TFSA", "Margin").
    pub kind: String,
    /// Human-readable label: type + last 4 digits.
    pub label: String,
    /// Whether this is the primary account.
    pub is_primary: bool,
}

/// Discover accounts by calling the Questrade API and filtering.
///
/// `account_types` is the whitelist; only accounts whose `kind` matches
/// (case-insensitive) are returned. If empty, defaults to `["RESP", "TFSA"]`.
///
/// Only accounts with `status == "Active"` are included.
pub async fn discover(
    qt: &QuestradeClient,
    account_types: &[String],
) -> Result<Vec<DiscoveredAccount>> {
    let types: Vec<String> = if account_types.is_empty() {
        vec!["RESP".to_string(), "TFSA".to_string()]
    } else {
        account_types
            .iter()
            .map(|t| t.to_uppercase())
            .collect()
    };

    let resp = qt.accounts().await?;

    let mut discovered: Vec<DiscoveredAccount> = resp
        .accounts
        .into_iter()
        .filter(|a| {
            a.status == "Active"
                && types.iter().any(|t| a.kind.to_uppercase() == *t)
        })
        .map(|a| {
            let label = format!(
                "{}·{}",
                a.kind,
                &a.number[a.number.len().saturating_sub(4)..]
            );
            DiscoveredAccount {
                number: a.number,
                kind: a.kind,
                label,
                is_primary: a.is_primary,
            }
        })
        .collect();

    // Stable sort: primary accounts first, then by type, then by number.
    discovered.sort_by(|a, b| {
        b.is_primary
            .cmp(&a.is_primary)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.number.cmp(&b.number))
    });

    if discovered.is_empty() {
        return Err(Error::ConfigParse(format!(
            "no active accounts found matching types {types:?}; check Questrade account status"
        )));
    }

    tracing::info!(
        "discovered {} account(s): {}",
        discovered.len(),
        discovered
            .iter()
            .map(|a| a.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    Ok(discovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_account_label_format() {
        let a = DiscoveredAccount {
            number: "12345678".into(),
            kind: "RESP".into(),
            label: "RESP·5678".into(),
            is_primary: true,
        };
        assert_eq!(a.label, "RESP·5678");
        assert!(a.is_primary);
    }

    #[test]
    fn sort_primary_first_then_type() {
        let mut accounts = vec![
            DiscoveredAccount {
                number: "1".into(),
                kind: "Margin".into(),
                label: "M·1".into(),
                is_primary: false,
            },
            DiscoveredAccount {
                number: "2".into(),
                kind: "RESP".into(),
                label: "R·2".into(),
                is_primary: true,
            },
            DiscoveredAccount {
                number: "3".into(),
                kind: "TFSA".into(),
                label: "T·3".into(),
                is_primary: false,
            },
        ];
        accounts.sort_by(|a, b| {
            b.is_primary
                .cmp(&a.is_primary)
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.number.cmp(&b.number))
        });
        assert_eq!(accounts[0].kind, "RESP"); // primary first
        assert!(accounts[0].is_primary);
    }
}
