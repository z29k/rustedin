//! Commands that span every platform.

use crate::core::config::{self, ConfigFile};
use crate::core::output::emit;
use serde_json::{json, Map, Value};

/// Every configured account, grouped by platform.
pub fn cmd_accounts() -> Result<(), String> {
    let mut out = Map::new();
    let mut total = 0usize;

    #[cfg(feature = "linkedin")]
    {
        let accounts = crate::providers::linkedin::commands::account_list()?;
        total += accounts.len();
        out.insert("linkedin".to_string(), Value::Array(accounts));
    }
    #[cfg(feature = "meta")]
    {
        let accounts = crate::providers::meta::commands::account_list()?;
        total += accounts.len();
        out.insert("meta".to_string(), Value::Array(accounts));
    }

    if total == 0 {
        eprintln!(
            "No account configured yet.\n  \
             LinkedIn : rustedin linkedin setup ... && rustedin linkedin auth --account=<alias>\n  \
             Meta     : rustedin meta setup ... && rustedin meta auth --account=<alias>"
        );
    }
    emit(&Value::Object(out));
    Ok(())
}

/// Token status for every account, grouped by platform.
pub async fn cmd_status(check: bool) -> Result<(), String> {
    let mut out = Map::new();

    #[cfg(feature = "linkedin")]
    out.insert(
        "linkedin".to_string(),
        Value::Object(crate::providers::linkedin::commands::status_map()?),
    );
    #[cfg(feature = "meta")]
    out.insert(
        "meta".to_string(),
        Value::Object(crate::providers::meta::commands::status_map(check).await?),
    );
    #[cfg(not(feature = "meta"))]
    let _ = check;

    emit(&Value::Object(out));
    Ok(())
}

/// Fold another config file into this one.
///
/// Written for the 1.x → 2.0 move, where LinkedIn and Meta credentials lived in
/// two separate files (`rustedin.json` and `rustameta.json`): point `--from` at
/// the other one and its app credentials and accounts land in their namespaced
/// section. Nothing already configured is overwritten unless `--force` says so,
/// and the source file is never modified.
pub fn cmd_migrate(from: &str, force: bool) -> Result<(), String> {
    let raw = std::fs::read_to_string(from)
        .map_err(|e| format!("Failed to read {from}: {e}\nPass the path with --from."))?;
    let value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("{from} is not valid JSON: {e}"))?;
    let incoming: ConfigFile = serde_json::from_value(config::upgrade_legacy(value))
        .map_err(|e| format!("{from} is not a rustedin or rustameta config: {e}"))?;

    let mut current = config::load()?;
    let mut report = Report::default();

    #[cfg(feature = "linkedin")]
    merge_linkedin(&mut current, &incoming, force, &mut report);
    #[cfg(feature = "meta")]
    merge_meta(&mut current, &incoming, force, &mut report);

    // Sections rustedin does not know about travel too, so a file written by a
    // newer build is not silently truncated.
    for (key, value) in &incoming.extra {
        if !current.extra.contains_key(key) {
            current.extra.insert(key.clone(), value.clone());
            report.imported_apps.push(key.clone());
        }
    }

    config::save(&current)?;

    eprintln!(
        "Merged {from} into {} — {} account(s) imported, {} skipped.",
        config::path().display(),
        report.imported.len(),
        report.skipped.len()
    );
    if !report.skipped.is_empty() {
        eprintln!("Pass --force to overwrite: {}", report.skipped.join(", "));
    }

    emit(&json!({
        "success": true,
        "from": from,
        "into": config::path().display().to_string(),
        "imported_accounts": report.imported,
        "imported_app_credentials": report.imported_apps,
        "skipped_accounts": report.skipped,
    }));
    Ok(())
}

#[derive(Default)]
struct Report {
    imported: Vec<String>,
    imported_apps: Vec<String>,
    skipped: Vec<String>,
}

#[cfg(feature = "linkedin")]
fn merge_linkedin(
    current: &mut ConfigFile,
    incoming: &ConfigFile,
    force: bool,
    report: &mut Report,
) {
    for (label, src, dst) in [
        (
            "linkedin.personal",
            &incoming.linkedin.app.personal,
            &mut current.linkedin.app.personal,
        ),
        (
            "linkedin.organization",
            &incoming.linkedin.app.organization,
            &mut current.linkedin.app.organization,
        ),
    ] {
        if !src.client_id.is_empty() && (dst.client_id.is_empty() || force) {
            *dst = src.clone();
            report.imported_apps.push(label.to_string());
        }
    }

    for (alias, account) in &incoming.linkedin.accounts {
        let key = format!("linkedin:{alias}");
        if current.linkedin.accounts.contains_key(alias) && !force {
            report.skipped.push(key);
            continue;
        }
        current
            .linkedin
            .accounts
            .insert(alias.clone(), account.clone());
        report.imported.push(key);
    }
}

#[cfg(feature = "meta")]
fn merge_meta(current: &mut ConfigFile, incoming: &ConfigFile, force: bool, report: &mut Report) {
    if !incoming.meta.app.app_id.is_empty() && (current.meta.app.app_id.is_empty() || force) {
        current.meta.app = incoming.meta.app.clone();
        report.imported_apps.push("meta".to_string());
    } else if current.meta.app.config_id.is_none() {
        current.meta.app.config_id = incoming.meta.app.config_id.clone();
    }

    for (alias, account) in &incoming.meta.accounts {
        let key = format!("meta:{alias}");
        if current.meta.accounts.contains_key(alias) && !force {
            report.skipped.push(key);
            continue;
        }
        current.meta.accounts.insert(alias.clone(), account.clone());
        report.imported.push(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{AppCredentials, LinkedInAccount};

    fn linkedin_account(alias: &str, token: &str) -> LinkedInAccount {
        LinkedInAccount {
            alias: alias.to_string(),
            account_type: "person".to_string(),
            urn: Some("urn:li:person:x".to_string()),
            person_urn: None,
            access_token: token.to_string(),
            refresh_token: String::new(),
            expires_at: 1,
            refresh_expires_at: 2,
        }
    }

    #[cfg(feature = "linkedin")]
    #[test]
    fn merging_adds_new_accounts_and_leaves_existing_ones_alone() {
        let mut current = ConfigFile::default();
        current
            .linkedin
            .accounts
            .insert("mine".to_string(), linkedin_account("mine", "keep"));

        let mut incoming = ConfigFile::default();
        incoming
            .linkedin
            .accounts
            .insert("mine".to_string(), linkedin_account("mine", "overwrite"));
        incoming
            .linkedin
            .accounts
            .insert("other".to_string(), linkedin_account("other", "new"));

        let mut report = Report::default();
        merge_linkedin(&mut current, &incoming, false, &mut report);

        assert_eq!(current.linkedin.accounts["mine"].access_token, "keep");
        assert_eq!(current.linkedin.accounts["other"].access_token, "new");
        assert_eq!(report.imported, vec!["linkedin:other"]);
        assert_eq!(report.skipped, vec!["linkedin:mine"]);
    }

    #[cfg(feature = "linkedin")]
    #[test]
    fn force_overwrites_an_existing_account() {
        let mut current = ConfigFile::default();
        current
            .linkedin
            .accounts
            .insert("mine".to_string(), linkedin_account("mine", "keep"));
        let mut incoming = ConfigFile::default();
        incoming
            .linkedin
            .accounts
            .insert("mine".to_string(), linkedin_account("mine", "fresh"));

        let mut report = Report::default();
        merge_linkedin(&mut current, &incoming, true, &mut report);
        assert_eq!(current.linkedin.accounts["mine"].access_token, "fresh");
        assert!(report.skipped.is_empty());
    }

    #[cfg(feature = "linkedin")]
    #[test]
    fn app_credentials_only_fill_an_empty_slot() {
        let mut current = ConfigFile::default();
        current.linkedin.app.personal = AppCredentials {
            client_id: "mine".to_string(),
            client_secret: "s".to_string(),
        };
        let mut incoming = ConfigFile::default();
        incoming.linkedin.app.personal = AppCredentials {
            client_id: "theirs".to_string(),
            client_secret: "s".to_string(),
        };
        incoming.linkedin.app.organization = AppCredentials {
            client_id: "org".to_string(),
            client_secret: "s".to_string(),
        };

        let mut report = Report::default();
        merge_linkedin(&mut current, &incoming, false, &mut report);
        assert_eq!(current.linkedin.app.personal.client_id, "mine");
        assert_eq!(current.linkedin.app.organization.client_id, "org");
        assert_eq!(report.imported_apps, vec!["linkedin.organization"]);
    }

    #[cfg(feature = "meta")]
    #[test]
    fn a_rustameta_file_lands_in_the_meta_section() {
        let raw = serde_json::json!({
            "metaApp": { "app_id": "1", "app_secret": "2", "config_id": "3" },
            "accounts": { "z29k": {
                "alias": "z29k", "user_id": "9", "access_token": "t", "expires_at": 5 } }
        });
        let incoming: ConfigFile = serde_json::from_value(config::upgrade_legacy(raw)).unwrap();

        let mut current = ConfigFile::default();
        let mut report = Report::default();
        merge_meta(&mut current, &incoming, false, &mut report);

        assert_eq!(current.meta.app.app_id, "1");
        assert_eq!(current.meta.app.config_id.as_deref(), Some("3"));
        assert_eq!(current.meta.accounts["z29k"].access_token, "t");
        assert_eq!(report.imported, vec!["meta:z29k"]);
    }
}
