//! One `cmd_*` function per LinkedIn subcommand.

use super::publish::{self, Article};
use super::store;
use super::{api, publish as pub_};
use crate::core::config;
use crate::core::media::MediaSource;
use crate::core::output::emit;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Configuration & accounts
// ---------------------------------------------------------------------------

pub fn cmd_setup(app_type: &str, client_id: &str, client_secret: &str) -> Result<(), String> {
    match app_type {
        "personal" | "organization" => {}
        _ => {
            return Err(format!(
                "Invalid app type \"{app_type}\". Use \"personal\" or \"organization\"."
            ))
        }
    }
    store::set_app_credentials(app_type, client_id, client_secret)?;
    eprintln!(
        "LinkedIn app \"{app_type}\" credentials saved to {}",
        config::path().display()
    );
    emit(&json!({
        "success": true,
        "platform": "linkedin",
        "app": app_type,
        "config": config::path().display().to_string(),
    }));
    Ok(())
}

/// Every LinkedIn account as a JSON array, for the platform-scoped `accounts`
/// command and for the aggregated one.
pub fn account_list() -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for alias in store::list_aliases()? {
        let account = store::get_account(&alias)?;
        out.push(serde_json::to_value(store::account_status(&account)).unwrap_or(Value::Null));
    }
    Ok(out)
}

pub fn cmd_accounts() -> Result<(), String> {
    let accounts = account_list()?;
    if accounts.is_empty() {
        eprintln!("No LinkedIn account configured. Run: rustedin linkedin auth --account=<alias>");
    }
    emit(&Value::Array(accounts));
    Ok(())
}

/// Status for every LinkedIn account, keyed by alias.
pub fn status_map() -> Result<serde_json::Map<String, Value>, String> {
    let mut out = serde_json::Map::new();
    for alias in store::list_aliases()? {
        let account = store::get_account(&alias)?;
        out.insert(
            alias,
            serde_json::to_value(store::account_status(&account)).unwrap_or(Value::Null),
        );
    }
    Ok(out)
}

pub fn cmd_status() -> Result<(), String> {
    let map = status_map()?;
    if map.is_empty() {
        eprintln!("No LinkedIn account configured. Run: rustedin linkedin auth --account=<alias>");
    }
    emit(&Value::Object(map));
    Ok(())
}

// ---------------------------------------------------------------------------
// Publishing
// ---------------------------------------------------------------------------

pub async fn cmd_post(account: &str, text: &str, visibility: &str) -> Result<(), String> {
    let post_id = pub_::text_post(account, text, visibility).await?;

    let preview: String = text.chars().take(100).collect();
    let preview = if text.chars().count() > 100 {
        format!("{preview}...")
    } else {
        preview
    };

    emit(&json!({
        "success": true,
        "platform": "linkedin",
        "post_id": post_id,
        "account": account,
        "urn": store::resolve_urn(account)?,
        "visibility": visibility,
        "text_preview": preview,
    }));
    Ok(())
}

pub async fn cmd_reshare(
    post_id: &str,
    accounts: &[String],
    commentary: Option<&str>,
    visibility: &str,
) -> Result<(), String> {
    let mut targets: Vec<String> = accounts.to_vec();

    // Resolve "*" to every personal account.
    if targets.iter().any(|a| a == "*") {
        targets = store::list_person_aliases()?;
        if targets.is_empty() {
            return Err(
                "No personal LinkedIn account configured. Add one with: rustedin linkedin auth"
                    .to_string(),
            );
        }
    }

    // Refresh tokens before fanning out — see `store::warm_tokens`.
    store::warm_tokens(&targets).await?;

    // Run the reshares concurrently.
    let handles: Vec<_> = targets
        .iter()
        .map(|alias| {
            let alias = alias.clone();
            let post_id = post_id.to_string();
            let commentary = commentary.map(str::to_string);
            let visibility = visibility.to_string();
            tokio::spawn(async move {
                pub_::reshare(&alias, &post_id, commentary.as_deref(), &visibility).await
            })
        })
        .collect();

    let mut results = Vec::new();
    for (i, handle) in handles.into_iter().enumerate() {
        let entry = match handle.await {
            Ok(Ok(reshare_id)) => json!({
                "account": targets[i], "success": true, "post_id": reshare_id,
            }),
            Ok(Err(e)) => json!({ "account": targets[i], "success": false, "error": e }),
            Err(e) => json!({
                "account": targets[i], "success": false, "error": e.to_string(),
            }),
        };
        results.push(entry);
    }

    let succeeded = results.iter().filter(|r| r["success"] == true).count();
    emit(&json!({
        "total": results.len(),
        "succeeded": succeeded,
        "failed": results.len() - succeeded,
        "results": results,
    }));
    Ok(())
}

/// Arguments for [`cmd_share`], grouped to keep the call site readable.
pub struct ShareArgs<'a> {
    pub account: &'a str,
    pub url: &'a str,
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub commentary: Option<&'a str>,
    pub visibility: &'a str,
    pub image: Option<&'a str>,
    pub mode: &'a str,
}

pub async fn cmd_share(args: ShareArgs<'_>) -> Result<(), String> {
    let ShareArgs {
        account,
        url,
        title,
        description,
        commentary,
        visibility,
        image,
        mode,
    } = args;

    if mode != "article" && mode != "image" {
        return Err("Mode must be \"article\" or \"image\".".to_string());
    }
    if mode == "image" && image.is_none() {
        return Err("--image is required in image mode.".to_string());
    }

    let source = match image {
        Some(s) => Some(MediaSource::parse(s)?),
        None => None,
    };

    let (post_id, image_urn) = if mode == "image" {
        // Image mode: full-size image, with the link inside the text.
        let text = match (commentary, description) {
            (Some(c), _) => format!("{c}\n\n{url}"),
            (None, Some(d)) => format!("{d}\n\n{url}"),
            (None, None) => url.to_string(),
        };
        let (post_id, image_urn) = pub_::image_post(
            account,
            &text,
            source.as_ref().expect("checked above"),
            title,
            visibility,
        )
        .await?;
        (post_id, Some(image_urn))
    } else {
        // Article mode: a link preview card with an optional thumbnail.
        // LinkedIn does not render article.description in the feed card, so a
        // description with no explicit commentary is promoted to commentary.
        let text = match (commentary, description) {
            (Some(c), _) => c,
            (None, Some(d)) => d,
            (None, None) => "",
        };
        if commentary.is_none() && description.is_some() {
            crate::note!(
                "Using --description as commentary (LinkedIn does not display it in the link \
                 preview)."
            );
        }
        publish::article_post(
            account,
            Article {
                url,
                title,
                description,
                commentary: text,
                visibility,
                image: source.as_ref(),
            },
        )
        .await?
    };

    let mut result = json!({
        "success": true,
        "platform": "linkedin",
        "post_id": post_id,
        "account": account,
        "urn": store::resolve_urn(account)?,
        "url": url,
        "mode": mode,
    });
    if let Some(urn) = image_urn {
        result["image_urn"] = json!(urn);
    }
    emit(&result);
    Ok(())
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

pub async fn cmd_get_post(account: &str, post_id: &str) -> Result<(), String> {
    let path = format!("/rest/posts/{}", api::encode_urn(post_id));
    emit(&api::get(account, &path, &[]).await?);
    Ok(())
}

pub async fn cmd_comments(
    account: &str,
    post_id: &str,
    count: u32,
    start: u32,
) -> Result<(), String> {
    if !post_id.starts_with("urn:li:") {
        return Err(format!(
            "Invalid post URN \"{post_id}\". Expected urn:li:share:xxx, urn:li:activity:xxx or \
             urn:li:ugcPost:xxx."
        ));
    }
    if count == 0 || count > 100 {
        return Err("Count must be between 1 and 100.".to_string());
    }

    let path = format!("/rest/socialActions/{}/comments", api::encode_urn(post_id));
    let count_str = count.to_string();
    let start_str = start.to_string();
    let data = api::get(
        account,
        &path,
        &[("count", count_str.as_str()), ("start", start_str.as_str())],
    )
    .await?;

    emit(&json!({
        "post_id": post_id,
        "account": account,
        "start": start,
        "count": count,
        "total": data["paging"]["total"].as_u64().unwrap_or(0),
        "comments": data["elements"].clone(),
    }));
    Ok(())
}

pub async fn cmd_profile(account: &str, urn: Option<&str>) -> Result<(), String> {
    match urn {
        Some(person_urn) => {
            if !person_urn.starts_with("urn:li:person:") {
                return Err(format!(
                    "Invalid person URN \"{person_urn}\". Expected urn:li:person:xxx."
                ));
            }
            let path = format!("/rest/people/(id:{})", api::encode_urn(person_urn));
            emit(&api::get(account, &path, &[]).await?);
        }
        None => {
            let stored = store::get_account(account)?;

            // Organization tokens don't carry the openid scope (the Community
            // Management API must be the only product on the app), so
            // /v2/userinfo is unavailable.
            if stored.account_type == "organization" {
                return Err(format!(
                    "Profile not available for organization account \"{account}\". Pass a person \
                     URN to look up a member: rustedin linkedin profile --account={account} \
                     --urn=urn:li:person:xxx"
                ));
            }

            let data = api::get(account, "/v2/userinfo", &[]).await?;
            let mut result = json!({
                "account": account,
                "type": stored.account_type,
                "urn": stored.urn.unwrap_or_default(),
            });
            for key in [
                "sub",
                "name",
                "given_name",
                "family_name",
                "email",
                "email_verified",
                "picture",
                "locale",
            ] {
                if let Some(val) = data.get(key) {
                    result[key] = val.clone();
                }
            }
            emit(&result);
        }
    }
    Ok(())
}
