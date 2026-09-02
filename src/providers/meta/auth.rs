//! Facebook Login (OAuth 2.0 authorization code), followed by Page and
//! Instagram discovery.
//!
//! One authorization covers both platforms: the Page tokens it yields publish
//! to Facebook Pages, and each Page carries the ID of the Instagram
//! Professional account linked to it.

use super::api;
use super::store::{self, Account, InstagramAccount, Page};
use crate::core::config::now_ms;
use crate::core::oauth::{self, Callback};
use serde_json::Value;

/// Everything rustedin needs to publish on both platforms.
///
/// `pages_manage_posts` and `instagram_content_publish` are Advanced Access
/// permissions: they work for users with a role on the app while it is in
/// Development mode, and require App Review before the app can go Live.
///
/// `publish_video` is deliberately absent: Meta rejects it as an invalid scope
/// and covers Page video publishing with `pages_manage_posts`.
pub const DEFAULT_SCOPES: &[&str] = &[
    "pages_show_list",
    "pages_read_engagement",
    "pages_manage_posts",
    "instagram_basic",
    "instagram_content_publish",
];

/// Fallback lifetime for a long-lived user token when Meta omits `expires_in`.
const SIXTY_DAYS_SECONDS: u64 = 60 * 24 * 60 * 60;

/// Stand-in lifetime for tokens Meta reports as `expires_at: 0` — system user
/// tokens. A far-future date keeps every expiry comparison in `store` working,
/// where a sentinel value would need special-casing everywhere.
const NEVER_EXPIRES_SECONDS: u64 = 10 * 365 * 24 * 60 * 60;

pub struct AuthArgs<'a> {
    pub alias: &'a str,
    pub port: u16,
    pub redirect_uri: Option<&'a str>,
    pub scopes: Option<&'a str>,
    pub config_id: Option<&'a str>,
    pub no_browser: bool,
}

pub async fn run_auth(args: AuthArgs<'_>) -> Result<(), String> {
    let AuthArgs {
        alias,
        port,
        redirect_uri,
        scopes,
        config_id,
        no_browser,
    } = args;

    let (app_id, app_secret) = store::get_app_credentials()?;

    let redirect_uri = redirect_uri
        .map(str::to_string)
        .unwrap_or_else(|| format!("http://localhost:{port}/callback"));
    let config_id = config_id.map(str::to_string).or_else(store::config_id);
    // Login for Business delegates assets through a configuration; the classic
    // login asks for scopes. Meta explicitly advises against sending `scope`
    // alongside a `config_id`, so only an explicit --scopes overrides it.
    let scopes = match (&config_id, scopes) {
        (Some(_), None) => None,
        (_, explicit) => Some(
            explicit
                .map(str::to_string)
                .unwrap_or_else(|| DEFAULT_SCOPES.join(",")),
        ),
    };

    let state = oauth::random_state();

    let mut params: Vec<(&str, &str)> = vec![
        ("client_id", app_id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("state", state.as_str()),
        ("response_type", "code"),
    ];
    if let Some(id) = config_id.as_deref() {
        params.push(("config_id", id));
        // Required by system-user configurations, and a no-op for user-token
        // ones: it just makes our response_type=code win over the default.
        params.push(("override_default_response_type", "true"));
    }
    if let Some(s) = scopes.as_deref() {
        params.push(("scope", s));
    }

    let auth_url = reqwest::Url::parse_with_params(
        &format!("https://www.facebook.com/{}/dialog/oauth", api::version()),
        &params,
    )
    .map_err(|e| format!("Failed to build the authorization URL: {e}"))?
    .to_string();

    eprintln!("\nAuthenticating \"{alias}\" with Meta...");
    eprintln!("  Redirect URI : {redirect_uri}");
    match (config_id.as_deref(), scopes.as_deref()) {
        (Some(id), None) => {
            eprintln!("  Config ID    : {id} (assets delegated by the configuration)")
        }
        (Some(id), Some(s)) => {
            eprintln!("  Config ID    : {id}");
            eprintln!("  Scopes       : {s} (explicit --scopes)");
        }
        (None, Some(s)) => eprintln!("  Scopes       : {s}"),
        (None, None) => unreachable!("scopes are only dropped when a config ID is set"),
    }
    eprintln!(
        "\n> The redirect URI above must be listed verbatim under\n\
         >   App Dashboard → Facebook Login → Settings → Valid OAuth Redirect URIs.\n\
         > Meta only accepts http://localhost while the app is in Development mode.\n"
    );
    eprintln!("If your browser doesn't open, visit this URL manually:\n");
    eprintln!("{auth_url}\n");

    if !no_browser {
        let _ = open::that(&auth_url);
    }

    let code = oauth::wait_for_code(Callback {
        port,
        state: &state,
        platform: "Meta",
        alias,
    })
    .await?;

    eprintln!("\nExchanging the code for a long-lived token...");

    let short_lived = exchange_code(&app_id, &app_secret, &redirect_uri, &code).await?;
    let (user_token, expires_in) = match exchange_long_lived(&short_lived).await {
        Ok(pair) => pair,
        Err(e) => {
            // System user tokens have no long-lived grant: they never expire.
            eprintln!("! Long-lived exchange declined ({e}).");
            eprintln!("! Keeping the token as issued — asking Meta when it expires.");
            let lifetime = token_lifetime(&short_lived).await;
            (short_lived, lifetime)
        }
    };

    store_account(alias, user_token, expires_in).await
}

/// Adopt an access token minted outside the login flow — typically a Business
/// system user token, which is how Meta expects you to automate assets you own
/// yourself rather than on behalf of a client business.
pub async fn run_import_token(alias: &str, token: &str) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("The token is empty. Pass --token, or pipe it on stdin.".to_string());
    }

    eprintln!("\nAdopting a token for \"{alias}\"...");
    let lifetime = token_lifetime(token).await;
    store_account(alias, token.to_string(), lifetime).await
}

/// Resolve who a token belongs to, what it can reach, and persist it.
async fn store_account(alias: &str, user_token: String, expires_in: u64) -> Result<(), String> {
    let me = api::get(
        &user_token,
        "/me",
        &[("fields".to_string(), "id,name".to_string())],
    )
    .await?;
    let user_id = me["id"]
        .as_str()
        .ok_or("Missing 'id' in the /me response")?
        .to_string();
    let name = me["name"].as_str().unwrap_or("unknown").to_string();

    let granted = granted_scopes(&user_token).await;
    let pages = fetch_pages(&user_token).await?;

    let now = now_ms();
    let default_page = (pages.len() == 1).then(|| pages[0].id.clone());

    store::upsert_account(Account {
        alias: alias.to_string(),
        user_id: user_id.clone(),
        name: name.clone(),
        access_token: user_token,
        expires_at: now + expires_in * 1000,
        scopes: granted.clone(),
        pages: pages.clone(),
        default_page,
    })?;

    eprintln!(
        "\nMeta account \"{alias}\" saved to {}",
        crate::core::config::path().display()
    );
    eprintln!("  Authorized by : {name} ({user_id})");
    if expires_in >= NEVER_EXPIRES_SECONDS {
        eprintln!("  User token    : does not expire");
    } else {
        eprintln!(
            "  User token    : {} days (Page tokens below do not expire)",
            expires_in / 86_400
        );
    }
    eprintln!("  Scopes        : {}", granted.join(", "));

    if pages.is_empty() {
        eprintln!(
            "\n! No Facebook Page returned. The token must carry `pages_show_list`, and its\n\
             ! owner must have a role on at least one Page. For a system user token, the\n\
             ! Pages have to be assigned to that system user in the portfolio settings."
        );
    } else {
        eprintln!("  Pages         : {}", pages.len());
        for p in &pages {
            let ig = match &p.instagram {
                Some(ig) => format!(
                    " → Instagram {} ({})",
                    ig.username.clone().unwrap_or_else(|| "?".to_string()),
                    ig.id
                ),
                None => " (no linked Instagram account)".to_string(),
            };
            eprintln!("    - {} ({}){ig}", p.name, p.id);
        }
    }

    warn_about_missing_scopes(&granted);
    eprintln!("\nDone.\n");
    Ok(())
}

/// Granted scopes, from `/me/permissions` — falling back to `/debug_token`,
/// which system user tokens answer where `/me/permissions` does not.
async fn granted_scopes(token: &str) -> Vec<String> {
    if let Ok(scopes) = fetch_permissions(token).await {
        if !scopes.is_empty() {
            return scopes;
        }
    }
    debug_token(token)
        .await
        .ok()
        .and_then(|info| {
            info["scopes"].as_array().map(|items| {
                items
                    .iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default()
}

fn warn_about_missing_scopes(granted: &[String]) {
    let missing: Vec<&str> = DEFAULT_SCOPES
        .iter()
        .copied()
        .filter(|s| !granted.iter().any(|g| g == s))
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "\n! Not granted: {}\n\
             ! Commands relying on these will fail with a Graph API permission error.",
            missing.join(", ")
        );
    }
}

// ---------------------------------------------------------------------------
// Token exchanges
// ---------------------------------------------------------------------------

async fn exchange_code(
    app_id: &str,
    app_secret: &str,
    redirect_uri: &str,
    code: &str,
) -> Result<String, String> {
    let data = api::oauth_get(
        "/oauth/access_token",
        &[
            ("client_id".to_string(), app_id.to_string()),
            ("client_secret".to_string(), app_secret.to_string()),
            ("redirect_uri".to_string(), redirect_uri.to_string()),
            ("code".to_string(), code.to_string()),
        ],
    )
    .await?;

    data["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("Missing access_token in the token exchange response: {data}"))
}

/// Exchange any valid user token for a fresh long-lived one (~60 days).
///
/// Also used to renew a long-lived token before it lapses — Facebook has no
/// refresh-token grant.
pub async fn exchange_long_lived(token: &str) -> Result<(String, u64), String> {
    let (app_id, app_secret) = store::get_app_credentials()?;

    let data = api::oauth_get(
        "/oauth/access_token",
        &[
            ("grant_type".to_string(), "fb_exchange_token".to_string()),
            ("client_id".to_string(), app_id),
            ("client_secret".to_string(), app_secret),
            ("fb_exchange_token".to_string(), token.to_string()),
        ],
    )
    .await?;

    let access_token = data["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| {
            format!("Missing access_token in the long-lived exchange response: {data}")
        })?;
    let expires_in = data["expires_in"].as_u64().unwrap_or(SIXTY_DAYS_SECONDS);

    Ok((access_token, expires_in))
}

/// Seconds until a token expires, straight from `/debug_token`.
///
/// `expires_at: 0` means "never" — system user tokens report that. We store a
/// far-future date rather than a sentinel, so every expiry comparison in
/// `store` keeps working unchanged.
async fn token_lifetime(token: &str) -> u64 {
    let info = match debug_token(token).await {
        Ok(info) => info,
        Err(e) => {
            eprintln!("! Could not read the token's expiry ({e}); assuming 60 days.");
            return SIXTY_DAYS_SECONDS;
        }
    };
    // A rejected token answers with an empty object, whose absent `expires_at`
    // would otherwise read as "never expires".
    if info["is_valid"].as_bool() != Some(true) {
        return SIXTY_DAYS_SECONDS;
    }
    let expires_at = info["expires_at"].as_u64().unwrap_or(0);
    if expires_at == 0 {
        return NEVER_EXPIRES_SECONDS;
    }
    let now = now_ms() / 1000;
    expires_at.saturating_sub(now)
}

/// Ask Meta what a token really is: its expiry, its scopes and its validity.
pub async fn debug_token(token: &str) -> Result<Value, String> {
    let (app_id, app_secret) = store::get_app_credentials()?;
    api::oauth_get(
        "/debug_token",
        &[
            ("input_token".to_string(), token.to_string()),
            ("access_token".to_string(), format!("{app_id}|{app_secret}")),
        ],
    )
    .await
    .map(|v| v["data"].clone())
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

async fn fetch_permissions(token: &str) -> Result<Vec<String>, String> {
    let data = api::get(token, "/me/permissions", &[]).await?;
    Ok(data["data"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|p| p["status"].as_str() == Some("granted"))
                .filter_map(|p| p["permission"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// Every Page the user administers, with its non-expiring Page token and the
/// Instagram Professional account linked to it.
pub async fn fetch_pages(user_token: &str) -> Result<Vec<Page>, String> {
    let mut pages = Vec::new();
    let mut response = api::get(
        user_token,
        "/me/accounts",
        &[
            (
                "fields".to_string(),
                "id,name,category,access_token,tasks,instagram_business_account{id,username}"
                    .to_string(),
            ),
            ("limit".to_string(), "100".to_string()),
        ],
    )
    .await?;

    // Follow `paging.next` — a user can administer more Pages than one page of
    // results. Bounded so a malformed cursor can never loop forever.
    for _ in 0..20 {
        let Some(items) = response["data"].as_array() else {
            break;
        };
        for item in items {
            pages.push(parse_page(item));
        }
        // Copy the cursor out before reassigning `response`, which would
        // otherwise still be borrowed by it.
        let next = response["paging"]["next"].as_str().map(str::to_string);
        match next {
            Some(url) => response = api::get_absolute(&url).await?,
            None => break,
        }
    }

    Ok(pages)
}

fn parse_page(item: &Value) -> Page {
    Page {
        id: item["id"].as_str().unwrap_or_default().to_string(),
        name: item["name"].as_str().unwrap_or("unnamed").to_string(),
        category: item["category"].as_str().map(str::to_string),
        access_token: item["access_token"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        tasks: item["tasks"]
            .as_array()
            .map(|t| {
                t.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        instagram: item["instagram_business_account"]["id"]
            .as_str()
            .map(|id| InstagramAccount {
                id: id.to_string(),
                username: item["instagram_business_account"]["username"]
                    .as_str()
                    .map(|u| format!("@{u}")),
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_parsing_extracts_the_linked_instagram_account() {
        let raw = serde_json::json!({
            "id": "123",
            "name": "Acme",
            "category": "Software",
            "access_token": "page-tok",
            "tasks": ["CREATE_CONTENT", "MANAGE"],
            "instagram_business_account": { "id": "ig-9", "username": "acme" }
        });
        let page = parse_page(&raw);
        assert_eq!(page.id, "123");
        assert!(page.can_create_content());
        let ig = page.instagram.unwrap();
        assert_eq!(ig.id, "ig-9");
        assert_eq!(ig.username.as_deref(), Some("@acme"));
    }

    #[test]
    fn page_without_instagram_parses_cleanly() {
        let raw = serde_json::json!({ "id": "1", "name": "Solo", "access_token": "t" });
        let page = parse_page(&raw);
        assert!(page.instagram.is_none());
        // No `tasks` at all must not block publishing.
        assert!(page.can_create_content());
    }
}
