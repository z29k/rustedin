use crate::token_store::{self, AccountTokens};
use rand::Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const REDIRECT_URI: &str = "http://localhost:8765/callback";
const PERSONAL_SCOPES: &str = "w_member_social openid profile email";
// The Community Management API must be the ONLY product on the org app (LinkedIn
// legal/security restriction), so openid/profile/email aren't available for it.
const ORGANIZATION_SCOPES: &str = "r_organization_social w_organization_social";

fn random_state() -> String {
    let mut rng = rand::thread_rng();
    (0..16).map(|_| format!("{:x}", rng.gen::<u8>())).collect()
}

pub async fn run_auth(alias: &str, org_id: Option<&str>) -> Result<(), String> {
    let account_type = if org_id.is_some() {
        "organization"
    } else {
        "person"
    };
    let (client_id, client_secret) = token_store::get_app_credentials(account_type)?;
    let scopes = if org_id.is_some() {
        ORGANIZATION_SCOPES
    } else {
        PERSONAL_SCOPES
    };

    let state = random_state();

    let auth_url = format!(
        "https://www.linkedin.com/oauth/v2/authorization?response_type=code&client_id={}&redirect_uri={}&state={}&scope={}",
        urlencoded(&client_id),
        urlencoded(REDIRECT_URI),
        &state,
        urlencoded(scopes),
    );

    let label = match org_id {
        Some(id) => format!("company page (org {id})"),
        None => "personal account".to_string(),
    };
    eprintln!("\nAuthenticating \"{alias}\" as {label}...");
    eprintln!("\nIf your browser doesn't open, visit this URL manually:\n");
    eprintln!("{auth_url}\n");

    let _ = open::that(&auth_url);

    // Wait for OAuth callback
    let code = wait_for_callback(&state, alias).await?;

    eprintln!("\nExchanging code for tokens...");

    // Exchange code for tokens
    let client = reqwest::Client::new();
    let params = [
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", &client_id),
        ("client_secret", &client_secret),
    ];

    let res = client
        .post("https://www.linkedin.com/oauth/v2/accessToken")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token exchange request failed: {e}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed ({status}): {body}"));
    }

    let tokens: serde_json::Value = res.json().await.map_err(|e| format!("JSON error: {e}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let access_token = tokens["access_token"]
        .as_str()
        .ok_or("Missing access_token")?
        .to_string();

    // Resolve the account URN and (for personal accounts) the authorizing member.
    // The org app only carries the Community Management API, so openid/profile aren't
    // granted and /v2/userinfo can't be called — the org URN comes from org_id instead.
    let (account_urn, person_urn, name) = match org_id {
        Some(id) => (
            format!("urn:li:organization:{id}"),
            None,
            "organization admin".to_string(),
        ),
        None => {
            let profile_res = client
                .get("https://api.linkedin.com/v2/userinfo")
                .header("Authorization", format!("Bearer {access_token}"))
                .send()
                .await
                .map_err(|e| format!("Profile fetch failed: {e}"))?;

            let profile: serde_json::Value = profile_res
                .json()
                .await
                .map_err(|e| format!("Profile JSON error: {e}"))?;

            let sub = profile["sub"].as_str().ok_or("Missing 'sub' in profile")?;
            let name = profile["name"].as_str().unwrap_or("unknown").to_string();
            let person_urn = format!("urn:li:person:{sub}");
            (person_urn.clone(), Some(person_urn), name)
        }
    };
    let account_type = if org_id.is_some() {
        "organization"
    } else {
        "person"
    };

    let expires_in = tokens["expires_in"].as_u64().unwrap_or(5184000);
    let refresh_token = tokens["refresh_token"].as_str().unwrap_or("").to_string();
    let refresh_expires_in = tokens["refresh_token_expires_in"]
        .as_u64()
        .unwrap_or(365 * 86400);

    token_store::upsert_account(AccountTokens {
        alias: alias.to_string(),
        account_type: account_type.to_string(),
        urn: Some(account_urn.clone()),
        person_urn: person_urn.clone(),
        access_token,
        refresh_token,
        expires_at: now + expires_in * 1000,
        refresh_expires_at: now + refresh_expires_in * 1000,
    })?;

    let access_days = expires_in / 86400;
    let refresh_days = refresh_expires_in / 86400;

    eprintln!(
        "\nAccount \"{alias}\" saved to {}",
        token_store::accounts_path().display()
    );
    eprintln!("  Type          : {account_type}");
    eprintln!("  URN           : {account_urn}");
    match &person_urn {
        Some(urn) => eprintln!("  Authorized by : {name} ({urn})"),
        None => eprintln!("  Authorized by : {name}"),
    }
    eprintln!("  Access token  : {access_days} days (auto-refreshed)");
    eprintln!("  Refresh token : {refresh_days} days (rustedin auth once/year)");
    eprintln!("\nDone.\n");

    Ok(())
}

async fn wait_for_callback(expected_state: &str, alias: &str) -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:8765")
        .await
        .map_err(|e| format!("Failed to bind port 8765: {e}"))?;

    eprintln!("Waiting for LinkedIn callback on http://localhost:8765 ...");

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(300), async {
        loop {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("Accept failed: {e}"))?;

            let mut buf = vec![0u8; 4096];
            let n = stream
                .read(&mut buf)
                .await
                .map_err(|e| format!("Read failed: {e}"))?;
            let request = String::from_utf8_lossy(&buf[..n]);

            // Parse GET /callback?code=...&state=... HTTP/1.1
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("");

            if !path.starts_with("/callback") {
                continue;
            }

            let query = path.split('?').nth(1).unwrap_or("");
            let params: std::collections::HashMap<&str, &str> = query
                .split('&')
                .filter_map(|p| {
                    let mut parts = p.splitn(2, '=');
                    Some((parts.next()?, parts.next()?))
                })
                .collect();

            let returned_state = params.get("state").copied().unwrap_or("");
            let code = params.get("code").copied().unwrap_or("");
            let error = params.get("error").copied();

            if error.is_some() || returned_state != expected_state || code.is_empty() {
                let msg = error.unwrap_or("invalid state or missing code");
                let response = format!(
                    concat!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\n\r\n",
                        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Authentification LinkedIn</title></head>",
                        "<body style=\"font-family:-apple-system,system-ui,sans-serif;display:flex;justify-content:center;align-items:center;min-height:100vh;margin:0;background:#fafafa\">",
                        "<div style=\"text-align:center;padding:2rem;background:#fff;border-radius:12px;box-shadow:0 2px 12px rgba(0,0,0,.08);max-width:420px\">",
                        "<div style=\"font-size:3rem;margin-bottom:1rem\">\u{274C}</div>",
                        "<h1 style=\"color:#dc3545;font-size:1.4rem;margin:0 0 .5rem\">Authentification \u{00E9}chou\u{00E9}e</h1>",
                        "<p style=\"color:#666;margin:0;font-size:.95rem\">{msg}</p>",
                        "</div></body></html>"
                    ),
                    msg = msg
                );
                let _ = stream.write_all(response.as_bytes()).await;
                return Err(format!("Auth failed: {msg}"));
            }

            let response = format!(
                concat!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\r\n",
                    "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Authentification LinkedIn</title></head>",
                    "<body style=\"font-family:-apple-system,system-ui,sans-serif;display:flex;justify-content:center;align-items:center;min-height:100vh;margin:0;background:#fafafa\">",
                    "<div style=\"text-align:center;padding:2rem;background:#fff;border-radius:12px;box-shadow:0 2px 12px rgba(0,0,0,.08);max-width:420px\">",
                    "<div style=\"font-size:3rem;margin-bottom:1rem\">\u{2705}</div>",
                    "<h1 style=\"color:#28a745;font-size:1.4rem;margin:0 0 .5rem\">Authentification r\u{00E9}ussie !</h1>",
                    "<p style=\"color:#333;margin:0 0 1rem;font-size:.95rem\">Compte <strong>\"{alias}\"</strong> connect\u{00E9}</p>",
                    "<p style=\"color:#999;margin:0;font-size:.85rem\">Vous pouvez fermer cet onglet.</p>",
                    "</div></body></html>"
                ),
                alias = alias
            );
            let _ = stream.write_all(response.as_bytes()).await;

            return Ok(code.to_string());
        }
    });

    timeout
        .await
        .map_err(|_| "Timeout waiting for OAuth callback (5 min)".to_string())?
}

fn urlencoded(s: &str) -> String {
    s.replace(' ', "%20")
        .replace(':', "%3A")
        .replace('/', "%2F")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
}
