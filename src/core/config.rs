//! The single on-disk configuration file, shared by every platform.
//!
//! One JSON document (`rustedin.json`, `0600` on Unix) holds the app
//! credentials and the authenticated accounts of every provider, each under its
//! own key:
//!
//! ```json
//! {
//!   "linkedin": { "app": { "personal": {}, "organization": {} }, "accounts": {} },
//!   "meta":     { "app": {}, "accounts": {} }
//! }
//! ```
//!
//! Namespacing matters: the same alias (`z29k`) routinely names a LinkedIn
//! company page *and* a Meta account, and the two must not collide.
//!
//! Files written by rustedin 1.x — where the LinkedIn app sat under
//! `linkedInApp` and its accounts at the top level — are upgraded on load, and
//! so are `rustameta.json` files, so `--config` keeps working when pointed at
//! either.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Default config file name, looked up next to the binary.
const FILE_NAME: &str = "rustedin.json";

/// Initialize the config path. Call once at startup.
/// With no override, defaults to `rustedin.json` next to the binary.
pub fn init_path(override_path: Option<&str>) {
    let path = match override_path {
        Some(p) => PathBuf::from(p),
        None => default_path(),
    };
    CONFIG_PATH.set(path).ok();
}

pub fn path() -> PathBuf {
    CONFIG_PATH.get().cloned().unwrap_or_else(default_path)
}

fn default_path() -> PathBuf {
    let exe = std::env::current_exe().expect("Failed to get executable path");
    exe.parent().unwrap().join(FILE_NAME)
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

/// The whole file.
///
/// Provider sections are always compiled, whatever the enabled features, and
/// unknown keys round-trip through `extra`. Between them, a build with a
/// platform disabled can never drop the other platform's credentials.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "LinkedInSection::is_empty")]
    pub linkedin: LinkedInSection,
    #[serde(default, skip_serializing_if = "MetaSection::is_empty")]
    pub meta: MetaSection,
    /// Anything rustedin does not know about, preserved verbatim on save.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// --- LinkedIn ---------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LinkedInSection {
    #[serde(default)]
    pub app: LinkedInApps,
    #[serde(default)]
    pub accounts: HashMap<String, LinkedInAccount>,
}

impl LinkedInSection {
    /// Whether the section carries nothing worth writing. Checked field by
    /// field rather than on the client ID alone: a half-filled app must still
    /// survive a save.
    fn is_empty(&self) -> bool {
        self.accounts.is_empty() && self.app.personal.is_empty() && self.app.organization.is_empty()
    }
}

/// LinkedIn requires two separate apps: the Community Management API must be
/// the only product on the app that posts for company pages, so personal
/// posting needs an app of its own.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LinkedInApps {
    #[serde(default)]
    pub personal: AppCredentials,
    #[serde(default)]
    pub organization: AppCredentials,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AppCredentials {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
}

impl AppCredentials {
    pub fn is_empty(&self) -> bool {
        self.client_id.is_empty() && self.client_secret.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedInAccount {
    pub alias: String,
    /// `person` or `organization`.
    #[serde(rename = "type")]
    pub account_type: String,
    pub urn: Option<String>,
    pub person_urn: Option<String>,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    /// Unix epoch, milliseconds.
    pub expires_at: u64,
    /// Unix epoch, milliseconds.
    #[serde(default)]
    pub refresh_expires_at: u64,
}

// --- Meta -------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MetaSection {
    #[serde(default)]
    pub app: MetaApp,
    #[serde(default)]
    pub accounts: HashMap<String, MetaAccount>,
}

impl MetaSection {
    fn is_empty(&self) -> bool {
        self.accounts.is_empty()
            && self.app.app_id.is_empty()
            && self.app.app_secret.is_empty()
            && self.app.config_id.is_none()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MetaApp {
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub app_secret: String,
    /// Facebook Login for Business configuration ID. Absent on apps using the
    /// classic Facebook Login, where `scope` carries the request instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaAccount {
    pub alias: String,
    pub user_id: String,
    #[serde(default)]
    pub name: String,
    /// Long-lived **user** token (~60 days).
    pub access_token: String,
    /// Unix epoch, milliseconds.
    pub expires_at: u64,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub pages: Vec<Page>,
    #[serde(default)]
    pub default_page: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
    /// Long-lived Page token — does not expire.
    pub access_token: String,
    #[serde(default)]
    pub tasks: Vec<String>,
    #[serde(default)]
    pub instagram: Option<InstagramAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstagramAccount {
    pub id: String,
    #[serde(default)]
    pub username: Option<String>,
}

// ---------------------------------------------------------------------------
// Load / save
// ---------------------------------------------------------------------------

/// Read the config file.
///
/// A missing file yields defaults, but a *malformed* one is an error rather
/// than a silent reset — overwriting a corrupt file would destroy the only
/// copy of the stored tokens.
pub fn load() -> Result<ConfigFile, String> {
    let path = path();
    if !path.exists() {
        return Ok(ConfigFile::default());
    }
    let data =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    if data.trim().is_empty() {
        return Ok(ConfigFile::default());
    }

    let raw: Value = serde_json::from_str(&data).map_err(|e| {
        format!(
            "{} is not valid JSON: {e}\nFix or move the file, then re-run `rustedin <platform> auth`.",
            path.display()
        )
    })?;

    serde_json::from_value(upgrade_legacy(raw)).map_err(|e| {
        format!(
            "{} is not a valid rustedin config: {e}\n\
             Fix or move the file, then re-run `rustedin <platform> auth`.",
            path.display()
        )
    })
}

pub fn save(data: &ConfigFile) -> Result<(), String> {
    let path = path();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
        }
    }
    let json = serde_json::to_string_pretty(data).map_err(|e| format!("JSON error: {e}"))?;
    write_private(&path, &json)
}

/// Rewrite a 1.x (or `rustameta.json`) layout into the namespaced one.
///
/// * `linkedInApp` + top-level `accounts` → `linkedin.app` / `linkedin.accounts`
/// * `metaApp` + top-level `accounts`     → `meta.app` / `meta.accounts`
///
/// Already-namespaced documents pass through untouched, so this is safe to run
/// on every load.
pub fn upgrade_legacy(raw: Value) -> Value {
    let Value::Object(mut obj) = raw else {
        return raw;
    };
    let legacy_linkedin = obj.contains_key("linkedInApp");
    let legacy_meta = obj.contains_key("metaApp");
    if !legacy_linkedin && !legacy_meta {
        return Value::Object(obj);
    }

    // The single top-level `accounts` map belongs to whichever app is declared.
    let accounts = obj.remove("accounts");
    let linkedin_app = obj.remove("linkedInApp");
    let meta_app = obj.remove("metaApp");

    if legacy_linkedin {
        obj.insert("linkedin".to_string(), section(linkedin_app, accounts));
        if legacy_meta {
            obj.insert("meta".to_string(), section(meta_app, None));
        }
    } else {
        obj.insert("meta".to_string(), section(meta_app, accounts));
    }

    Value::Object(obj)
}

fn section(app: Option<Value>, accounts: Option<Value>) -> Value {
    let mut s = Map::new();
    if let Some(app) = app {
        s.insert("app".to_string(), app);
    }
    s.insert(
        "accounts".to_string(),
        accounts.unwrap_or_else(|| Value::Object(Map::new())),
    );
    Value::Object(s)
}

/// Write `contents` to `path`, restricting the file to owner-only access
/// (`0600`) on Unix. It holds OAuth tokens, Page tokens and client secrets, so
/// it must never be world- or group-readable.
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

// ---------------------------------------------------------------------------
// Time helpers
// ---------------------------------------------------------------------------

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Rough `YYYY-MM-DD` rendering of an epoch-milliseconds timestamp.
///
/// Deliberately dependency-free: this only ever reaches human-facing messages,
/// so civil-calendar edge cases are not worth a date crate.
pub fn format_epoch_ms(ms: u64) -> String {
    let mut year = 1970i64;
    let mut days = (ms / 86_400_000) as i64;
    loop {
        let len = if is_leap(year) { 366 } else { 365 };
        if days < len {
            break;
        }
        days -= len;
        year += 1;
    }
    let months = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0usize;
    while month < 12 && days >= months[month] {
        days -= months[month];
        month += 1;
    }
    format!("{year:04}-{:02}-{:02}", month + 1, days + 1)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_1x_config_moves_under_the_linkedin_key() {
        let legacy = serde_json::json!({
            "linkedInApp": {
                "personal": { "client_id": "pid", "client_secret": "psec" },
                "organization": { "client_id": "oid", "client_secret": "osec" }
            },
            "accounts": {
                "quentin": {
                    "alias": "quentin", "type": "person",
                    "urn": "urn:li:person:x", "person_urn": "urn:li:person:x",
                    "access_token": "tok", "refresh_token": "",
                    "expires_at": 1, "refresh_expires_at": 2
                }
            }
        });
        let cfg: ConfigFile = serde_json::from_value(upgrade_legacy(legacy)).unwrap();
        assert_eq!(cfg.linkedin.app.personal.client_id, "pid");
        assert_eq!(cfg.linkedin.app.organization.client_secret, "osec");
        assert_eq!(cfg.linkedin.accounts["quentin"].account_type, "person");
        assert!(cfg.meta.accounts.is_empty());
    }

    #[test]
    fn a_rustameta_config_moves_under_the_meta_key() {
        let legacy = serde_json::json!({
            "metaApp": { "app_id": "42", "app_secret": "sec", "config_id": "cfg" },
            "accounts": {
                "z29k": {
                    "alias": "z29k", "user_id": "1", "name": "Z",
                    "access_token": "tok", "expires_at": 9,
                    "pages": [{ "id": "p1", "name": "P", "access_token": "pt" }]
                }
            }
        });
        let cfg: ConfigFile = serde_json::from_value(upgrade_legacy(legacy)).unwrap();
        assert_eq!(cfg.meta.app.app_id, "42");
        assert_eq!(cfg.meta.app.config_id.as_deref(), Some("cfg"));
        assert_eq!(cfg.meta.accounts["z29k"].pages[0].id, "p1");
        assert!(cfg.linkedin.accounts.is_empty());
    }

    #[test]
    fn a_namespaced_config_is_left_alone() {
        let modern = serde_json::json!({
            "linkedin": { "app": { "personal": { "client_id": "a", "client_secret": "b" } },
                          "accounts": {} },
            "meta": { "app": { "app_id": "1", "app_secret": "2" }, "accounts": {} }
        });
        assert_eq!(upgrade_legacy(modern.clone()), modern);
    }

    #[test]
    fn the_same_alias_can_name_both_a_linkedin_and_a_meta_account() {
        let doc = serde_json::json!({
            "linkedin": { "accounts": { "z29k": {
                "alias": "z29k", "type": "organization",
                "urn": "urn:li:organization:1", "person_urn": null,
                "access_token": "li", "refresh_token": "r",
                "expires_at": 1, "refresh_expires_at": 2 } } },
            "meta": { "accounts": { "z29k": {
                "alias": "z29k", "user_id": "9", "access_token": "fb", "expires_at": 3 } } }
        });
        let cfg: ConfigFile = serde_json::from_value(doc).unwrap();
        assert_eq!(cfg.linkedin.accounts["z29k"].access_token, "li");
        assert_eq!(cfg.meta.accounts["z29k"].access_token, "fb");
    }

    #[test]
    fn unknown_keys_survive_a_round_trip() {
        let doc = serde_json::json!({
            "linkedin": { "accounts": {} },
            "bluesky": { "accounts": { "me": { "handle": "me.bsky.social" } } }
        });
        let cfg: ConfigFile = serde_json::from_value(doc).unwrap();
        let back = serde_json::to_value(&cfg).unwrap();
        assert_eq!(
            back["bluesky"]["accounts"]["me"]["handle"],
            "me.bsky.social"
        );
    }

    #[test]
    fn empty_sections_are_not_written_out() {
        let back = serde_json::to_value(ConfigFile::default()).unwrap();
        assert_eq!(back, serde_json::json!({}));
    }

    #[test]
    fn a_half_filled_app_still_survives_a_save() {
        let mut cfg = ConfigFile::default();
        cfg.linkedin.app.personal.client_secret = "orphan".to_string();
        let back = serde_json::to_value(&cfg).unwrap();
        assert_eq!(
            back["linkedin"]["app"]["personal"]["client_secret"],
            "orphan"
        );

        let mut cfg = ConfigFile::default();
        cfg.meta.app.config_id = Some("cfg".to_string());
        let back = serde_json::to_value(&cfg).unwrap();
        assert_eq!(back["meta"]["app"]["config_id"], "cfg");
    }

    #[test]
    fn epoch_formatting_hits_known_dates() {
        assert_eq!(format_epoch_ms(0), "1970-01-01");
        // 2026-08-03T00:00:00Z
        assert_eq!(format_epoch_ms(1_785_715_200_000), "2026-08-03");
    }
}
