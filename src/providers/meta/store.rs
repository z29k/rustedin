//! Reading and writing the `meta` section of the config file.
//!
//! Token model — this is Meta-specific and worth internalising:
//!
//! * The **user access token** is long-lived (~60 days). It cannot be renewed
//!   with a refresh token; instead it is re-exchanged for a fresh one while it
//!   is still valid (`grant_type=fb_exchange_token`).
//! * The **Page access tokens** derived from a long-lived user token **do not
//!   expire**. Publishing therefore keeps working even if the user token
//!   lapses — but re-discovering Pages requires a valid user token.

use crate::core::config::{self, now_ms};
use serde::Serialize;

pub use crate::core::config::{InstagramAccount, MetaAccount as Account, Page};

/// Re-exchange the user token when it expires within 7 days.
const REFRESH_THRESHOLD_MS: u64 = 7 * 24 * 60 * 60 * 1000;

impl Page {
    /// Whether this Page grants the capability needed to publish.
    pub fn can_create_content(&self) -> bool {
        // Older tokens may not carry `tasks` at all; don't block on absence.
        self.tasks.is_empty() || self.tasks.iter().any(|t| t == "CREATE_CONTENT")
    }
}

// ---------------------------------------------------------------------------
// App credentials
// ---------------------------------------------------------------------------

pub fn get_app_credentials() -> Result<(String, String), String> {
    let data = config::load()?;
    if data.meta.app.app_id.is_empty() || data.meta.app.app_secret.is_empty() {
        return Err(
            "Meta app not configured. Run: rustedin meta setup --app-id=XXX --app-secret=YYY"
                .to_string(),
        );
    }
    Ok((data.meta.app.app_id, data.meta.app.app_secret))
}

/// App secret if one is stored, for `appsecret_proof`. Never errors: calls
/// still work (without the proof) on apps that don't require it.
pub fn app_secret() -> Option<String> {
    config::load()
        .ok()
        .map(|c| c.meta.app.app_secret)
        .filter(|s| !s.is_empty())
}

pub fn set_app_credentials(
    app_id: &str,
    app_secret: &str,
    config_id: Option<&str>,
) -> Result<(), String> {
    let mut data = config::load()?;
    data.meta.app.app_id = app_id.to_string();
    data.meta.app.app_secret = app_secret.to_string();
    if let Some(id) = config_id {
        data.meta.app.config_id = Some(id.to_string());
    }
    config::save(&data)
}

/// Stored Login for Business configuration ID, if `setup` was given one.
pub fn config_id() -> Option<String> {
    config::load()
        .ok()
        .and_then(|c| c.meta.app.config_id)
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

pub fn upsert_account(account: Account) -> Result<(), String> {
    let mut data = config::load()?;
    data.meta.accounts.insert(account.alias.clone(), account);
    config::save(&data)
}

pub fn get_account(alias: &str) -> Result<Account, String> {
    let data = config::load()?;
    match data.meta.accounts.get(alias) {
        Some(a) => Ok(a.clone()),
        None => {
            let mut available: Vec<&String> = data.meta.accounts.keys().collect();
            available.sort();
            Err(if available.is_empty() {
                format!(
                    "Meta account \"{alias}\" not found. No Meta account configured. \
                     Run: rustedin meta auth --account={alias}"
                )
            } else {
                format!(
                    "Meta account \"{alias}\" not found. Available: {}",
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
    let mut aliases: Vec<String> = config::load()?.meta.accounts.keys().cloned().collect();
    aliases.sort();
    Ok(aliases)
}

pub fn set_default_page(alias: &str, page_id: Option<String>) -> Result<(), String> {
    let mut data = config::load()?;
    let account = data
        .meta
        .accounts
        .get_mut(alias)
        .ok_or_else(|| format!("Meta account \"{alias}\" not found."))?;
    account.default_page = page_id;
    config::save(&data)
}

/// Pick the Page a command should act on.
///
/// `wanted` accepts a Page ID, an exact name, or a unique case-insensitive
/// substring of a name. With no `wanted`, falls back to the stored default and
/// then to the single Page when there is only one.
pub fn resolve_page(account: &Account, wanted: Option<&str>) -> Result<Page, String> {
    if account.pages.is_empty() {
        return Err(format!(
            "No Facebook Page stored for \"{}\". The authenticating user must have a role on at \
             least one Page. Re-run: rustedin meta auth --account={}",
            account.alias, account.alias
        ));
    }

    let listing = || {
        account
            .pages
            .iter()
            .map(|p| format!("{} ({})", p.name, p.id))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let Some(wanted) = wanted else {
        if let Some(default_id) = &account.default_page {
            if let Some(p) = account.pages.iter().find(|p| &p.id == default_id) {
                return Ok(p.clone());
            }
        }
        if account.pages.len() == 1 {
            return Ok(account.pages[0].clone());
        }
        return Err(format!(
            "Account \"{}\" has {} Pages — pass --page=<id|name>. Available: {}",
            account.alias,
            account.pages.len(),
            listing()
        ));
    };

    let needle = wanted.to_lowercase();

    let exact: Vec<&Page> = account
        .pages
        .iter()
        .filter(|p| p.id == wanted || p.name.to_lowercase() == needle)
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0].clone());
    }
    if exact.len() > 1 {
        return Err(format!(
            "\"{wanted}\" matches {} Pages by name — use the Page ID instead. Available: {}",
            exact.len(),
            listing()
        ));
    }

    let partial: Vec<&Page> = account
        .pages
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&needle))
        .collect();
    match partial.len() {
        1 => Ok(partial[0].clone()),
        0 => Err(format!(
            "No Page matching \"{wanted}\" on account \"{}\". Available: {}",
            account.alias,
            listing()
        )),
        n => Err(format!(
            "\"{wanted}\" matches {n} Pages — be more specific or use the Page ID. Available: {}",
            listing()
        )),
    }
}

/// The Instagram Professional account linked to a Page.
pub fn resolve_instagram(page: &Page) -> Result<InstagramAccount, String> {
    page.instagram.clone().ok_or_else(|| {
        format!(
            "Page \"{}\" ({}) has no linked Instagram Professional account.\n\
             Link one in Meta Business Suite (Settings → Instagram accounts), then re-run: \
             rustedin meta pages --account=<alias> --refresh",
            page.name, page.id
        )
    })
}

// ---------------------------------------------------------------------------
// User token lifecycle
// ---------------------------------------------------------------------------

/// Return a usable long-lived user token, re-exchanging it when it is close to
/// expiry.
///
/// A failed re-exchange is a warning, not an error: the current token is still
/// valid until `expires_at`.
pub async fn get_valid_user_token(alias: &str) -> Result<String, String> {
    let account = get_account(alias)?;
    let now = now_ms();

    if now >= account.expires_at {
        return Err(format!(
            "The user access token for \"{alias}\" expired on {}. \
             Re-authenticate with: rustedin meta auth --account={alias}",
            config::format_epoch_ms(account.expires_at)
        ));
    }

    if now < account.expires_at.saturating_sub(REFRESH_THRESHOLD_MS) {
        return Ok(account.access_token);
    }

    crate::note!("User token for \"{alias}\" expires soon; re-exchanging...");
    match super::auth::exchange_long_lived(&account.access_token).await {
        Ok((token, expires_in)) => {
            let mut updated = account.clone();
            updated.access_token = token.clone();
            updated.expires_at = now + expires_in * 1000;
            upsert_account(updated)?;
            crate::note!(
                "User token for \"{alias}\" renewed for {} days.",
                expires_in / 86_400
            );
            Ok(token)
        }
        Err(e) => {
            crate::note!("Warning: could not renew the user token for \"{alias}\": {e}");
            crate::note!(
                "Continuing with the current token (valid until {}).",
                config::format_epoch_ms(account.expires_at)
            );
            Ok(account.access_token)
        }
    }
}

// ---------------------------------------------------------------------------
// Status reporting
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct AccountStatus {
    pub platform: &'static str,
    pub alias: String,
    pub user_id: String,
    pub name: String,
    pub user_token_status: String,
    pub user_token_expires_in_days: i64,
    pub user_token_expires_on: String,
    pub needs_reauth: bool,
    pub pages: usize,
    pub instagram_accounts: usize,
    pub scopes: Vec<String>,
}

pub fn account_status(account: &Account) -> AccountStatus {
    let now = now_ms();
    let days = (account.expires_at as i64 - now as i64) / 86_400_000;
    AccountStatus {
        platform: "meta",
        alias: account.alias.clone(),
        user_id: account.user_id.clone(),
        name: account.name.clone(),
        user_token_status: if (now as i64) < account.expires_at as i64 {
            "active".to_string()
        } else {
            "expired".to_string()
        },
        user_token_expires_in_days: days,
        user_token_expires_on: config::format_epoch_ms(account.expires_at),
        needs_reauth: days <= 0,
        pages: account.pages.len(),
        instagram_accounts: account
            .pages
            .iter()
            .filter(|p| p.instagram.is_some())
            .count(),
        scopes: account.scopes.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: &str, name: &str, ig: bool) -> Page {
        Page {
            id: id.to_string(),
            name: name.to_string(),
            category: None,
            access_token: "tok".to_string(),
            tasks: vec!["CREATE_CONTENT".to_string()],
            instagram: ig.then(|| InstagramAccount {
                id: format!("ig-{id}"),
                username: Some(format!("@{name}")),
            }),
        }
    }

    fn account(pages: Vec<Page>) -> Account {
        Account {
            alias: "acme".to_string(),
            user_id: "1".to_string(),
            name: "Test".to_string(),
            access_token: "user-tok".to_string(),
            expires_at: now_ms() + 60 * 86_400_000,
            scopes: vec![],
            pages,
            default_page: None,
        }
    }

    #[test]
    fn single_page_needs_no_selector() {
        let acc = account(vec![page("1", "Acme", false)]);
        assert_eq!(resolve_page(&acc, None).unwrap().id, "1");
    }

    #[test]
    fn multiple_pages_require_a_selector() {
        let acc = account(vec![page("1", "Acme", false), page("2", "Globex", false)]);
        let err = resolve_page(&acc, None).unwrap_err();
        assert!(err.contains("--page"));
        assert!(err.contains("Globex"));
    }

    #[test]
    fn default_page_is_used_when_set() {
        let mut acc = account(vec![page("1", "Acme", false), page("2", "Globex", false)]);
        acc.default_page = Some("2".to_string());
        assert_eq!(resolve_page(&acc, None).unwrap().id, "2");
    }

    #[test]
    fn page_resolves_by_id_name_and_substring() {
        let acc = account(vec![
            page("1", "Acme Corp", false),
            page("2", "Globex", false),
        ]);
        assert_eq!(resolve_page(&acc, Some("2")).unwrap().id, "2");
        assert_eq!(resolve_page(&acc, Some("acme corp")).unwrap().id, "1");
        assert_eq!(resolve_page(&acc, Some("glob")).unwrap().id, "2");
    }

    #[test]
    fn ambiguous_substring_is_rejected() {
        let acc = account(vec![
            page("1", "Acme FR", false),
            page("2", "Acme US", false),
        ]);
        let err = resolve_page(&acc, Some("acme")).unwrap_err();
        assert!(err.contains("matches 2 Pages"));
    }

    #[test]
    fn unknown_page_lists_the_available_ones() {
        let acc = account(vec![page("1", "Acme", false)]);
        let err = resolve_page(&acc, Some("nope")).unwrap_err();
        assert!(err.contains("Acme (1)"));
    }

    #[test]
    fn instagram_requires_a_linked_account() {
        assert!(resolve_instagram(&page("1", "Acme", true)).is_ok());
        let err = resolve_instagram(&page("1", "Acme", false)).unwrap_err();
        assert!(err.contains("no linked Instagram"));
    }

    #[test]
    fn a_page_without_tasks_is_not_blocked() {
        let mut p = page("1", "Acme", false);
        assert!(p.can_create_content());
        p.tasks = vec!["ANALYZE".to_string()];
        assert!(!p.can_create_content());
        p.tasks.clear();
        assert!(p.can_create_content());
    }
}
