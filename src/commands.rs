use crate::api;
use crate::token_store;
use std::path::Path;

/// Escape reserved characters for LinkedIn's "little Text Format".
/// See: https://learn.microsoft.com/en-us/linkedin/marketing/community-management/shares/little-text-format
/// Without escaping, characters like ( ) are interpreted as mention syntax
/// and cause silent text truncation.
fn escape_commentary(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + text.len() / 4);
    for ch in text.chars() {
        match ch {
            '\\' | '|' | '{' | '}' | '@' | '[' | ']' | '(' | ')' | '<' | '>' | '#' | '*' | '_'
            | '~' => {
                result.push('\\');
                result.push(ch);
            }
            _ => result.push(ch),
        }
    }
    result
}

pub fn cmd_setup(app_type: &str, client_id: &str, client_secret: &str) -> Result<(), String> {
    match app_type {
        "personal" | "organization" => {}
        _ => {
            return Err(format!(
                "Invalid app type \"{app_type}\". Use \"personal\" or \"organization\"."
            ))
        }
    }
    token_store::set_app_credentials(app_type, client_id, client_secret)?;
    eprintln!(
        "App {app_type} credentials saved to {}",
        token_store::accounts_path().display()
    );
    Ok(())
}

pub fn cmd_accounts() -> Result<(), String> {
    let statuses = token_store::get_all_token_statuses();
    let aliases = token_store::list_aliases();

    if aliases.is_empty() {
        eprintln!("No accounts configured. Run: rustedin auth --account=<alias>");
        return Ok(());
    }

    let accounts: Vec<serde_json::Value> = aliases
        .iter()
        .map(|alias| {
            let status = statuses.get(alias);
            serde_json::json!({
                "alias": alias,
                "type": status.map(|s| s.account_type.as_str()).unwrap_or("unknown"),
                "urn": status.and_then(|s| s.urn.as_deref()).unwrap_or(""),
                "access_token_status": status.map(|s| s.access_token_status.as_str()).unwrap_or("unknown"),
                "access_token_expires_in_days": status.map(|s| s.access_token_expires_in_days).unwrap_or(0),
                "refresh_token_expires_in_days": status.map(|s| s.refresh_token_expires_in_days).unwrap_or(0),
            })
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&accounts).unwrap_or_default()
    );
    Ok(())
}

pub fn cmd_status() -> Result<(), String> {
    let statuses = token_store::get_all_token_statuses();

    if statuses.is_empty() {
        eprintln!("No accounts configured. Run: rustedin auth --account=<alias>");
        return Ok(());
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&statuses).unwrap_or_default()
    );
    Ok(())
}

pub async fn cmd_post(account: &str, text: &str, visibility: &str) -> Result<(), String> {
    if text.is_empty() || text.len() > 3000 {
        return Err("Text must be between 1 and 3000 characters".to_string());
    }

    let urn = api::resolve_urn(account)?;
    let escaped = escape_commentary(text);
    let body = api::build_base_post(&urn, &escaped, visibility);
    let post_id = api::linkedin_post(account, "/rest/posts", &body).await?;

    let preview = if text.len() > 100 {
        format!("{}...", &text[..100])
    } else {
        text.to_string()
    };

    let result = serde_json::json!({
        "success": true,
        "post_id": post_id,
        "account": account,
        "urn": urn,
        "visibility": visibility,
        "text_preview": preview,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );
    Ok(())
}

pub async fn cmd_reshare(
    post_id: &str,
    accounts: &[String],
    commentary: Option<&str>,
    visibility: &str,
) -> Result<(), String> {
    let mut targets: Vec<String> = accounts.to_vec();

    // Resolve "*" to all person accounts
    if targets.contains(&"*".to_string()) {
        targets = token_store::list_aliases()
            .into_iter()
            .filter(|a| {
                token_store::get_account(a)
                    .map(|acc| acc.account_type == "person")
                    .unwrap_or(false)
            })
            .collect();
        if targets.is_empty() {
            return Err(
                "No person accounts configured. Add accounts with: rustedin auth".to_string(),
            );
        }
    }

    let mut results = Vec::new();

    // Run reshares concurrently
    let handles: Vec<_> = targets
        .iter()
        .map(|alias| {
            let alias = alias.clone();
            let post_id = post_id.to_string();
            let commentary = commentary.map(|s| s.to_string());
            let visibility = visibility.to_string();
            tokio::spawn(async move {
                let urn = api::resolve_urn(&alias)?;
                let body = serde_json::json!({
                    "author": urn,
                    "commentary": escape_commentary(commentary.as_deref().unwrap_or("")),
                    "visibility": visibility,
                    "distribution": {
                        "feedDistribution": "MAIN_FEED",
                        "targetEntities": [],
                        "thirdPartyDistributionChannels": []
                    },
                    "reshareContext": { "parent": post_id },
                    "lifecycleState": "PUBLISHED",
                    "isReshareDisabledByAuthor": false
                });
                let reshare_id = api::linkedin_post(&alias, "/rest/posts", &body).await?;
                Ok::<_, String>(serde_json::json!({
                    "account": alias,
                    "success": true,
                    "post_id": reshare_id,
                }))
            })
        })
        .collect();

    for (i, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(Ok(val)) => results.push(val),
            Ok(Err(e)) => results.push(serde_json::json!({
                "account": targets[i],
                "success": false,
                "error": e,
            })),
            Err(e) => results.push(serde_json::json!({
                "account": targets[i],
                "success": false,
                "error": e.to_string(),
            })),
        }
    }

    let succeeded = results.iter().filter(|r| r["success"] == true).count();
    let failed = results.len() - succeeded;

    let summary = serde_json::json!({
        "total": results.len(),
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&summary).unwrap_or_default()
    );
    Ok(())
}

async fn resolve_image_bytes(source: &str) -> Result<Vec<u8>, String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let res = reqwest::get(source)
            .await
            .map_err(|e| format!("Failed to download image from {source}: {e}"))?;

        if !res.status().is_success() {
            let status = res.status();
            return Err(format!("Image download failed ({status}): {source}"));
        }

        let bytes = res
            .bytes()
            .await
            .map_err(|e| format!("Failed to read image bytes: {e}"))?;
        Ok(bytes.to_vec())
    } else {
        let path = Path::new(source);
        if !path.exists() {
            return Err(format!("Image file not found: {source}"));
        }
        if !path.is_file() {
            return Err(format!("Image path is not a file: {source}"));
        }
        std::fs::read(path).map_err(|e| format!("Failed to read image file {source}: {e}"))
    }
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
        return Err("Mode must be \"article\" or \"image\"".to_string());
    }
    if title.is_empty() || title.len() > 400 {
        return Err("Title must be between 1 and 400 characters".to_string());
    }
    if mode == "image" && image.is_none() {
        return Err("--image is required in image mode".to_string());
    }

    let urn = api::resolve_urn(account)?;

    let image_urn = match image {
        Some(source) => {
            let image_bytes = resolve_image_bytes(source).await?;

            if image_bytes.len() > 10 * 1024 * 1024 {
                return Err(format!(
                    "Image too large ({:.1} MB). LinkedIn allows max 10 MB.",
                    image_bytes.len() as f64 / (1024.0 * 1024.0)
                ));
            }

            eprintln!(
                "[rustedin] Uploading image ({:.1} KB)...",
                image_bytes.len() as f64 / 1024.0
            );
            let (upload_url, img_urn) = api::initialize_image_upload(account, &urn).await?;
            api::upload_image_binary(account, &upload_url, image_bytes).await?;
            eprintln!("[rustedin] Image uploaded: {img_urn}");
            Some(img_urn)
        }
        None => None,
    };

    let post_id = if mode == "image" {
        // Image mode: full image + link in commentary text
        let effective_commentary = match (commentary, description) {
            (Some(c), _) => format!("{c}\n\n{url}"),
            (None, Some(d)) => format!("{d}\n\n{url}"),
            (None, None) => url.to_string(),
        };

        eprintln!(
            "[rustedin] Commentary length: {} chars",
            effective_commentary.len()
        );

        if effective_commentary.len() > 3000 {
            return Err(format!(
                "Commentary too long ({} chars). LinkedIn allows max 3000.",
                effective_commentary.len()
            ));
        }

        let escaped = escape_commentary(&effective_commentary);
        let mut body = api::build_base_post(&urn, &escaped, visibility);
        body["content"] = serde_json::json!({
            "media": {
                "id": image_urn.as_ref().unwrap(),
                "title": title
            }
        });

        api::linkedin_post(account, "/rest/posts", &body).await?
    } else {
        // Article mode (default): link preview card with optional thumbnail
        // LinkedIn does not render article.description in the feed link preview.
        // Fallback: use description as commentary when no explicit commentary.
        let effective_commentary = match (commentary, description) {
            (Some(c), _) => c,
            (None, Some(d)) => d,
            (None, None) => "",
        };

        eprintln!(
            "[rustedin] Commentary length: {} chars",
            effective_commentary.len()
        );

        if effective_commentary.len() > 3000 {
            return Err(format!(
                "Commentary too long ({} chars). LinkedIn allows max 3000.",
                effective_commentary.len()
            ));
        }

        if commentary.is_none() && description.is_some() {
            eprintln!(
                "[rustedin] Note: using --description as commentary (LinkedIn does not display it in the link preview)."
            );
        }

        let escaped = escape_commentary(effective_commentary);
        let mut body = api::build_base_post(&urn, &escaped, visibility);

        let mut article = serde_json::json!({
            "source": url,
            "title": title,
        });
        if let Some(desc) = description {
            article["description"] = serde_json::json!(desc);
        }
        if let Some(ref img_urn) = image_urn {
            article["thumbnail"] = serde_json::json!(img_urn);
        }
        body["content"] = serde_json::json!({ "article": article });

        api::linkedin_post(account, "/rest/posts", &body).await?
    };

    let mut result = serde_json::json!({
        "success": true,
        "post_id": post_id,
        "account": account,
        "urn": urn,
        "url": url,
        "mode": mode,
    });
    if let Some(img_urn) = image_urn {
        result["image_urn"] = serde_json::json!(img_urn);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );
    Ok(())
}

pub async fn cmd_get_post(account: &str, post_id: &str) -> Result<(), String> {
    let encoded_urn = api::encode_urn(post_id);
    let path = format!("/rest/posts/{encoded_urn}");
    let data = api::linkedin_get(account, &path, &[]).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&data).unwrap_or_default()
    );
    Ok(())
}

pub async fn cmd_comments(
    account: &str,
    post_id: &str,
    count: u32,
    start: u32,
) -> Result<(), String> {
    if !post_id.starts_with("urn:li:") {
        return Err(format!("Invalid post URN \"{post_id}\". Expected format: urn:li:share:xxx, urn:li:activity:xxx, or urn:li:ugcPost:xxx"));
    }
    if count == 0 || count > 100 {
        return Err("Count must be between 1 and 100".to_string());
    }

    let encoded_urn = api::encode_urn(post_id);
    let path = format!("/rest/socialActions/{encoded_urn}/comments");
    let count_str = count.to_string();
    let start_str = start.to_string();
    let query = [("count", count_str.as_str()), ("start", start_str.as_str())];

    let data = api::linkedin_get(account, &path, &query).await?;

    let total = data["paging"]["total"].as_u64().unwrap_or(0);
    let comments = data["elements"].clone();

    let result = serde_json::json!({
        "post_id": post_id,
        "account": account,
        "start": start,
        "count": count,
        "total": total,
        "comments": comments,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );
    Ok(())
}

pub async fn cmd_profile(account: &str, urn: Option<&str>) -> Result<(), String> {
    match urn {
        Some(person_urn) => {
            // Look up another person's profile
            if !person_urn.starts_with("urn:li:person:") {
                return Err(format!(
                    "Invalid person URN \"{person_urn}\". Expected format: urn:li:person:xxx"
                ));
            }
            let encoded_urn = api::encode_urn(person_urn);
            let path = format!("/rest/people/(id:{encoded_urn})");
            let data = api::linkedin_get(account, &path, &[]).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&data).unwrap_or_default()
            );
        }
        None => {
            let stored = token_store::get_account(account);

            // Organization tokens don't carry the openid scope (Community Management
            // API must be the only product on the app), so /v2/userinfo is unavailable.
            if stored.as_ref().map(|a| a.account_type.as_str()) == Some("organization") {
                return Err(format!(
                    "Profile not available for organization account \"{account}\". \
                     Pass a person URN to look up a member: rustedin profile --account={account} --urn=urn:li:person:xxx"
                ));
            }

            // Own profile via OpenID userinfo
            let data = api::linkedin_get(account, "/v2/userinfo", &[]).await?;

            let mut result = serde_json::json!({
                "account": account,
                "type": stored.as_ref().map(|a| a.account_type.as_str()).unwrap_or("unknown"),
                "urn": stored.as_ref().and_then(|a| a.urn.as_deref()).unwrap_or(""),
            });

            // Merge userinfo fields
            for key in &[
                "sub",
                "name",
                "given_name",
                "family_name",
                "email",
                "email_verified",
                "picture",
                "locale",
            ] {
                if let Some(val) = data.get(*key) {
                    result[*key] = val.clone();
                }
            }

            println!(
                "{}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );
        }
    }
    Ok(())
}
