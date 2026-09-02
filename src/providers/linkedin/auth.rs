//! LinkedIn OAuth 2.0 (authorization code) with a local loopback callback.

use super::store::{self, Account};
use crate::core::config::now_ms;
use crate::core::http::{self, Retry};
use crate::core::oauth::{self, Callback};
use serde_json::Value;

const TOKEN_URL: &str = "https://www.linkedin.com/oauth/v2/accessToken";
const AUTHORIZE_URL: &str = "https://www.linkedin.com/oauth/v2/authorization";

const PERSONAL_SCOPES: &str = "w_member_social openid profile email";
// The Community Management API must be the ONLY product on the org app (a
// LinkedIn legal/security restriction), so openid/profile/email aren't
// available for it.
const ORGANIZATION_SCOPES: &str = "r_organization_social w_organization_social";

/// Fallback lifetimes when LinkedIn omits them: 60 days / 1 year.
const DEFAULT_EXPIRES_IN: u64 = 5_184_000;
const DEFAULT_REFRESH_EXPIRES_IN: u64 = 365 * 86_400;

pub async fn run_auth(alias: &str, org_id: Option<&str>, port: u16) -> Result<(), String> {
    let account_type = if org_id.is_some() {
        "organization"
    } else {
        "person"
    };
    let (client_id, client_secret) = store::get_app_credentials(account_type)?;
    let scopes = if org_id.is_some() {
        ORGANIZATION_SCOPES
    } else {
        PERSONAL_SCOPES
    };

    let redirect_uri = format!("http://localhost:{port}/callback");
    let state = oauth::random_state();

    let auth_url = format!(
        "{AUTHORIZE_URL}?response_type=code&client_id={}&redirect_uri={}&state={state}&scope={}",
        oauth::urlencode(&client_id),
        oauth::urlencode(&redirect_uri),
        oauth::urlencode(scopes),
    );

    let label = match org_id {
        Some(id) => format!("company page (org {id})"),
        None => "personal account".to_string(),
    };
    eprintln!("\nAuthenticating \"{alias}\" as {label}...");
    eprintln!("  Redirect URI : {redirect_uri}");
    eprintln!("  Scopes       : {scopes}");
    eprintln!(
        "\n> The redirect URI above must be listed verbatim under\n\
         >   LinkedIn Developers → your app → Auth → Authorized redirect URLs.\n"
    );
    eprintln!("If your browser doesn't open, visit this URL manually:\n");
    eprintln!("{auth_url}\n");

    let _ = open::that(&auth_url);

    let code = oauth::wait_for_code(Callback {
        port,
        state: &state,
        platform: "LinkedIn",
        alias,
    })
    .await?;

    eprintln!("\nExchanging code for tokens...");

    let tokens = token_request(
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect_uri),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
        ],
        "Token exchange",
    )
    .await?;

    let now = now_ms();
    let access_token = tokens["access_token"]
        .as_str()
        .ok_or("Missing access_token")?
        .to_string();

    // Resolve the account URN and, for personal accounts, the authorizing
    // member. The org app only carries the Community Management API, so
    // openid/profile aren't granted and /v2/userinfo can't be called — the org
    // URN comes from --org-id instead.
    let (account_urn, person_urn, name) = match org_id {
        Some(id) => (
            format!("urn:li:organization:{id}"),
            None,
            "organization admin".to_string(),
        ),
        None => {
            let profile = userinfo(&access_token).await?;
            let sub = profile["sub"].as_str().ok_or("Missing 'sub' in profile")?;
            let name = profile["name"].as_str().unwrap_or("unknown").to_string();
            let person_urn = format!("urn:li:person:{sub}");
            (person_urn.clone(), Some(person_urn), name)
        }
    };

    let expires_in = tokens["expires_in"].as_u64().unwrap_or(DEFAULT_EXPIRES_IN);
    let refresh_token = tokens["refresh_token"].as_str().unwrap_or("").to_string();
    let refresh_expires_in = tokens["refresh_token_expires_in"]
        .as_u64()
        .unwrap_or(DEFAULT_REFRESH_EXPIRES_IN);

    store::upsert_account(Account {
        alias: alias.to_string(),
        account_type: account_type.to_string(),
        urn: Some(account_urn.clone()),
        person_urn: person_urn.clone(),
        access_token,
        refresh_token,
        expires_at: now + expires_in * 1000,
        refresh_expires_at: now + refresh_expires_in * 1000,
    })?;

    eprintln!(
        "\nLinkedIn account \"{alias}\" saved to {}",
        crate::core::config::path().display()
    );
    eprintln!("  Type          : {account_type}");
    eprintln!("  URN           : {account_urn}");
    match &person_urn {
        Some(urn) => eprintln!("  Authorized by : {name} ({urn})"),
        None => eprintln!("  Authorized by : {name}"),
    }
    eprintln!(
        "  Access token  : {} days (auto-refreshed)",
        expires_in / 86_400
    );
    eprintln!(
        "  Refresh token : {} days (re-run auth once a year)",
        refresh_expires_in / 86_400
    );
    eprintln!("\nDone.\n");

    Ok(())
}

/// Trade a refresh token for a fresh access token. Called by
/// [`super::store::get_valid_token`] when expiry is near.
pub async fn exchange_refresh_token(
    refresh_token: &str,
    client_id: &str,
    client_secret: &str,
    alias: &str,
) -> Result<Value, String> {
    if refresh_token.is_empty() {
        return Err(format!(
            "No refresh token stored for \"{alias}\". \
             Re-authenticate with: rustedin linkedin auth --account={alias}"
        ));
    }
    token_request(
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ],
        &format!("Token refresh for \"{alias}\""),
    )
    .await
    .map_err(|e| format!("{e}\nRe-authenticate with: rustedin linkedin auth --account={alias}"))
}

async fn token_request(params: &[(&str, &str)], what: &str) -> Result<Value, String> {
    let res = http::send(
        || {
            http::client()
                .post(TOKEN_URL)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .form(params)
        },
        what,
        // An authorization code is single-use: never repeat an exchange that
        // may already have consumed it.
        &Retry::writes(),
    )
    .await
    .map_err(|e| format!("{what} failed: {e}"))?;

    res.json(what)
}

/// OpenID `userinfo`, used to resolve a personal account's member URN.
async fn userinfo(access_token: &str) -> Result<Value, String> {
    let context = "GET /v2/userinfo";
    http::send(
        || {
            http::client()
                .get("https://api.linkedin.com/v2/userinfo")
                .header("Authorization", format!("Bearer {access_token}"))
        },
        context,
        &Retry::reads(),
    )
    .await
    .map_err(|e| format!("Profile fetch failed: {e}"))?
    .json(context)
}
