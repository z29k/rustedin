//! Reading and writing the `linkedin` section of the config file.
//!
//! Token model: LinkedIn issues a 60-day access token alongside a one-year
//! refresh token. rustedin renews the access token automatically once it is
//! within a week of expiry; the refresh token itself has to be re-obtained with
//! `auth` about once a year.

use crate::core::config::{self, now_ms};
use serde::Serialize;

pub use crate::core::config::LinkedInAccount as Account;

/// Refresh the access token when it expires within 7 days.
const REFRESH_THRESHOLD_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// The two app kinds LinkedIn forces us to keep apart.
pub fn app_label(account_type: &str) -> &'static str {
    if account_type == "organization" {
        "organization"
    } else {
        "personal"
    }
}

pub fn get_app_credentials(account_type: &str) -> Result<(String, String), String> {
    let data = config::load()?;
    let label = app_label(account_type);
    let creds = match label {
        "organization" => &data.linkedin.app.organization,
        _ => &data.linkedin.app.personal,
    };
    if creds.client_id.is_empty() || creds.client_secret.is_empty() {
        return Err(format!(
            "LinkedIn app \"{label}\" not configured. Run: \
             rustedin linkedin setup --app={label} --client-id=XXX --client-secret=YYY"
        ));
    }
    Ok((creds.client_id.clone(), creds.client_secret.clone()))
}

pub fn set_app_credentials(
    app_type: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(), String> {
    let mut data = config::load()?;
    let creds = match app_label(app_type) {
        "organization" => &mut data.linkedin.app.organization,
        _ => &mut data.linkedin.app.personal,
    };
    creds.client_id = client_id.to_string();
    creds.client_secret = client_secret.to_string();
    config::save(&data)
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

pub fn upsert_account(account: Account) -> Result<(), String> {
    let mut data = config::load()?;
    data.linkedin
        .accounts
        .insert(account.alias.clone(), account);
    config::save(&data)
}

pub fn get_account(alias: &str) -> Result<Account, String> {
    let data = config::load()?;
    match data.linkedin.accounts.get(alias) {
        Some(a) => Ok(a.clone()),
        None => {
            let mut available: Vec<&String> = data.linkedin.accounts.keys().collect();
            available.sort();
            Err(if available.is_empty() {
                format!(
                    "LinkedIn account \"{alias}\" not found. No LinkedIn account configured. \
                     Run: rustedin linkedin auth --account={alias}"
                )
            } else {
                format!(
                    "LinkedIn account \"{alias}\" not found. Available: {}",
                    available
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
        }
    }
}

pub fn list_aliases() -> Result<Vec<String>, String> {
    let mut aliases: Vec<String> = config::load()?.linkedin.accounts.keys().cloned().collect();
    aliases.sort();
    Ok(aliases)
}

/// Aliases of every personal account — what `reshare --accounts=*` expands to.
pub fn list_person_aliases() -> Result<Vec<String>, String> {
    let data = config::load()?;
    let mut aliases: Vec<String> = data
        .linkedin
        .accounts
        .values()
        .filter(|a| a.account_type == "person")
        .map(|a| a.alias.clone())
        .collect();
    aliases.sort();
    Ok(aliases)
}

/// The URN a post is authored as.
pub fn resolve_urn(alias: &str) -> Result<String, String> {
    let account = get_account(alias)?;
    account.urn.ok_or_else(|| {
        format!("No URN stored for \"{alias}\". Run: rustedin linkedin auth --account={alias}")
    })
}

// ---------------------------------------------------------------------------
// Token lifecycle
// ---------------------------------------------------------------------------

async fn refresh_token(account: &Account) -> Result<Account, String> {
    let (client_id, client_secret) = get_app_credentials(&account.account_type)?;

    let data = super::auth::exchange_refresh_token(
        &account.refresh_token,
        &client_id,
        &client_secret,
        &account.alias,
    )
    .await?;
    let now = now_ms();

    let mut updated = account.clone();
    updated.access_token = data["access_token"]
        .as_str()
        .ok_or("Missing access_token in refresh response")?
        .to_string();
    if let Some(rt) = data["refresh_token"].as_str() {
        updated.refresh_token = rt.to_string();
    }
    updated.expires_at = now + data["expires_in"].as_u64().unwrap_or(5_184_000) * 1000;
    if let Some(rt_exp) = data["refresh_token_expires_in"].as_u64() {
        updated.refresh_expires_at = now + rt_exp * 1000;
    }

    upsert_account(updated.clone())?;
    Ok(updated)
}

pub async fn get_valid_token(alias: &str) -> Result<String, String> {
    let account = get_account(alias)?;
    let now = now_ms();

    if now >= account.refresh_expires_at {
        return Err(format!(
            "The refresh token for \"{alias}\" expired on {}. \
             Re-authenticate with: rustedin linkedin auth --account={alias}",
            config::format_epoch_ms(account.refresh_expires_at)
        ));
    }

    if now >= account.expires_at.saturating_sub(REFRESH_THRESHOLD_MS) {
        crate::note!("Refreshing token for \"{alias}\"...");
        let updated = refresh_token(&account).await?;
        crate::note!("Token refreshed for \"{alias}\".");
        return Ok(updated.access_token);
    }

    Ok(account.access_token)
}

/// Refresh every listed alias's token **sequentially**, up front.
///
/// A fan-out that publishes from several accounts at once would otherwise have
/// two tasks refresh their token simultaneously, each rewriting the whole
/// config file from its own stale copy and losing the other's new token. Doing
/// it here, before anything is spawned, keeps the concurrent phase read-only.
pub async fn warm_tokens(aliases: &[String]) -> Result<(), String> {
    for alias in aliases {
        get_valid_token(alias).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Status reporting
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AccountStatus {
    pub platform: &'static str,
    pub alias: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub urn: Option<String>,
    pub access_token_status: String,
    pub access_token_expires_in_days: i64,
    pub access_token_expires_on: String,
    pub refresh_token_expires_in_days: i64,
    pub refresh_token_expires_on: String,
    pub needs_reauth: bool,
}

pub fn account_status(account: &Account) -> AccountStatus {
    let now = now_ms();
    let access_days = (account.expires_at as i64 - now as i64) / 86_400_000;
    let refresh_days = (account.refresh_expires_at as i64 - now as i64) / 86_400_000;
    AccountStatus {
        platform: "linkedin",
        alias: account.alias.clone(),
        account_type: account.account_type.clone(),
        urn: account.urn.clone().or_else(|| account.person_urn.clone()),
        access_token_status: if (now as i64) < account.expires_at as i64 {
            "active".to_string()
        } else {
            "expired".to_string()
        },
        access_token_expires_in_days: access_days,
        access_token_expires_on: config::format_epoch_ms(account.expires_at),
        refresh_token_expires_in_days: refresh_days,
        refresh_token_expires_on: config::format_epoch_ms(account.refresh_expires_at),
        needs_reauth: refresh_days <= 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(kind: &str, access_in_days: i64, refresh_in_days: i64) -> Account {
        let now = now_ms() as i64;
        Account {
            alias: "acme".to_string(),
            account_type: kind.to_string(),
            urn: Some("urn:li:person:x".to_string()),
            person_urn: Some("urn:li:person:x".to_string()),
            access_token: "tok".to_string(),
            refresh_token: "ref".to_string(),
            expires_at: (now + access_in_days * 86_400_000) as u64,
            refresh_expires_at: (now + refresh_in_days * 86_400_000) as u64,
        }
    }

    #[test]
    fn app_label_only_knows_two_kinds() {
        assert_eq!(app_label("organization"), "organization");
        assert_eq!(app_label("person"), "personal");
        assert_eq!(app_label("anything else"), "personal");
    }

    #[test]
    fn a_live_token_reports_active() {
        let s = account_status(&account("person", 30, 300));
        assert_eq!(s.access_token_status, "active");
        // 29 or 30, depending on how many milliseconds elapsed since `now_ms()`.
        assert!((29..=30).contains(&s.access_token_expires_in_days));
        assert!(!s.needs_reauth);
    }

    #[test]
    fn a_dead_refresh_token_demands_reauth() {
        let s = account_status(&account("organization", -1, -1));
        assert_eq!(s.access_token_status, "expired");
        assert!(s.needs_reauth);
        assert_eq!(s.account_type, "organization");
    }
}
