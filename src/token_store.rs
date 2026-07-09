use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Initialize the config path. Call once at startup.
/// If `override_path` is None, defaults to `rustedin.json` next to the binary.
pub fn init_path(override_path: Option<&str>) {
    let path = match override_path {
        Some(p) => PathBuf::from(p),
        None => {
            let exe = std::env::current_exe().expect("Failed to get executable path");
            exe.parent().unwrap().join("rustedin.json")
        }
    };
    CONFIG_PATH.set(path).ok();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountTokens {
    pub alias: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub urn: Option<String>,
    pub person_urn: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    pub refresh_expires_at: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AppCredentials {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LinkedInApps {
    #[serde(default)]
    pub personal: AppCredentials,
    #[serde(default)]
    pub organization: AppCredentials,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AccountsFile {
    #[serde(default, rename = "linkedInApp")]
    pub linkedin_app: LinkedInApps,
    #[serde(default)]
    pub accounts: HashMap<String, AccountTokens>,
}

/// Refresh if token expires within 7 days
const REFRESH_THRESHOLD_MS: u64 = 7 * 24 * 60 * 60 * 1000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

pub fn accounts_path() -> PathBuf {
    CONFIG_PATH.get().cloned().unwrap_or_else(|| {
        let exe = std::env::current_exe().expect("Failed to get executable path");
        exe.parent().unwrap().join("rustedin.json")
    })
}

pub fn load_accounts() -> AccountsFile {
    let path = accounts_path();
    if !path.exists() {
        return AccountsFile::default();
    }
    match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => AccountsFile::default(),
    }
}

pub fn save_accounts(data: &AccountsFile) -> Result<(), String> {
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(data).map_err(|e| format!("JSON error: {e}"))?;
    write_private(&path, &json)?;

    Ok(())
}

/// Write `contents` to `path`, restricting the file to owner-only access (0600)
/// on Unix. This file holds OAuth tokens and client secrets, so it must never be
/// world- or group-readable.
#[cfg(unix)]
fn write_private(path: &std::path::Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    // `.mode()` only applies when the file is created; enforce 0600 even if the
    // file already existed with looser permissions.
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("Failed to set permissions on {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

pub fn upsert_account(account: AccountTokens) -> Result<(), String> {
    let mut data = load_accounts();
    data.accounts.insert(account.alias.clone(), account);
    save_accounts(&data)
}

pub fn get_account(alias: &str) -> Option<AccountTokens> {
    load_accounts().accounts.get(alias).cloned()
}

pub fn list_aliases() -> Vec<String> {
    load_accounts().accounts.keys().cloned().collect()
}

pub fn get_app_credentials(account_type: &str) -> Result<(String, String), String> {
    let data = load_accounts();
    let creds = match account_type {
        "organization" => &data.linkedin_app.organization,
        _ => &data.linkedin_app.personal,
    };
    let label = if account_type == "organization" {
        "organization"
    } else {
        "personal"
    };
    if creds.client_id.is_empty() || creds.client_secret.is_empty() {
        return Err(format!(
            "App {label} not configured. Run: rustedin setup --app={label} --client-id=XXX --client-secret=YYY"
        ));
    }
    Ok((creds.client_id.clone(), creds.client_secret.clone()))
}

pub fn set_app_credentials(
    app_type: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<(), String> {
    let mut data = load_accounts();
    let creds = match app_type {
        "organization" => &mut data.linkedin_app.organization,
        _ => &mut data.linkedin_app.personal,
    };
    creds.client_id = client_id.to_string();
    creds.client_secret = client_secret.to_string();
    save_accounts(&data)
}

async fn refresh_token(account: &AccountTokens) -> Result<AccountTokens, String> {
    let (client_id, client_secret) = get_app_credentials(&account.account_type)?;

    let client = reqwest::Client::new();
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", &account.refresh_token),
        ("client_id", &client_id),
        ("client_secret", &client_secret),
    ];

    let res = client
        .post("https://www.linkedin.com/oauth/v2/accessToken")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Refresh request failed: {e}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        return Err(format!(
            "Token refresh failed for \"{}\" ({status}): {body}\nRe-authenticate with: rustedin auth --account={}",
            account.alias, account.alias
        ));
    }

    let data: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))?;
    let now = now_ms();

    let mut updated = account.clone();
    updated.access_token = data["access_token"]
        .as_str()
        .ok_or("Missing access_token in refresh response")?
        .to_string();
    if let Some(rt) = data["refresh_token"].as_str() {
        updated.refresh_token = rt.to_string();
    }
    updated.expires_at = now + data["expires_in"].as_u64().unwrap_or(5184000) * 1000;
    if let Some(rt_exp) = data["refresh_token_expires_in"].as_u64() {
        updated.refresh_expires_at = now + rt_exp * 1000;
    }

    upsert_account(updated.clone())?;
    Ok(updated)
}

pub async fn get_valid_token(alias: &str) -> Result<String, String> {
    let account = match get_account(alias) {
        Some(a) => a,
        None => {
            let available = list_aliases();
            return Err(if available.is_empty() {
                format!("Account \"{alias}\" not found. No accounts configured. Run: rustedin auth --account={alias}")
            } else {
                format!(
                    "Account \"{alias}\" not found. Available: {}",
                    available.join(", ")
                )
            });
        }
    };

    let now = now_ms();

    if now >= account.refresh_expires_at {
        return Err(format!(
            "Refresh token for \"{alias}\" has expired. Re-authenticate with: rustedin auth --account={alias}"
        ));
    }

    if now >= account.expires_at.saturating_sub(REFRESH_THRESHOLD_MS) {
        eprintln!("[rustedin] Refreshing token for \"{alias}\"...");
        let updated = refresh_token(&account).await?;
        eprintln!("[rustedin] Token refreshed for \"{alias}\".");
        return Ok(updated.access_token);
    }

    Ok(account.access_token)
}

#[derive(Serialize)]
pub struct TokenStatus {
    #[serde(rename = "type")]
    pub account_type: String,
    pub urn: Option<String>,
    pub access_token_status: String,
    pub access_token_expires_in_days: i64,
    pub refresh_token_expires_in_days: i64,
    pub needs_reauth: bool,
}

pub fn get_all_token_statuses() -> HashMap<String, TokenStatus> {
    let data = load_accounts();
    let now = now_ms();
    let mut result = HashMap::new();

    for (alias, acc) in &data.accounts {
        let access_days = (acc.expires_at as i64 - now as i64) / 86_400_000;
        let refresh_days = (acc.refresh_expires_at as i64 - now as i64) / 86_400_000;
        result.insert(
            alias.clone(),
            TokenStatus {
                account_type: acc.account_type.clone(),
                urn: acc.urn.clone().or(acc.person_urn.clone()),
                access_token_status: if (now as i64) < acc.expires_at as i64 {
                    "active".to_string()
                } else {
                    "expired".to_string()
                },
                access_token_expires_in_days: access_days,
                refresh_token_expires_in_days: refresh_days,
                needs_reauth: refresh_days <= 0,
            },
        );
    }

    result
}
