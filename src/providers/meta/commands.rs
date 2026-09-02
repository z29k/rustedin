//! One `cmd_*` function per Meta subcommand.

use super::api;
use super::auth;
use super::facebook::{self, FeedPost, Publish, VideoPost};
use super::instagram::{self, PostOptions, Surface};
use super::store::{self, Account, InstagramAccount, Page};
use crate::core::config;
use crate::core::media::{MediaKind, MediaSource};
use crate::core::output::emit;
use serde_json::{json, Value};

/// Resolve `--account` / `--page` into the objects every publishing command
/// needs, warning when the Page lacks the content-creation capability.
pub fn target(account_alias: &str, page: Option<&str>) -> Result<(Account, Page), String> {
    let account = store::get_account(account_alias)?;
    let page = store::resolve_page(&account, page)?;
    if !page.can_create_content() {
        crate::note!(
            "Warning: the token for Page \"{}\" does not carry the CREATE_CONTENT task. \
             Publishing will likely fail.",
            page.name
        );
    }
    Ok((account, page))
}

pub fn target_instagram(
    account_alias: &str,
    page: Option<&str>,
) -> Result<(Account, Page, InstagramAccount), String> {
    let (account, page) = target(account_alias, page)?;
    let ig = store::resolve_instagram(&page)?;
    Ok((account, page, ig))
}

pub fn parse_media_kind(explicit: Option<&str>, source: &MediaSource) -> Result<MediaKind, String> {
    match explicit {
        Some(v) => match v.to_lowercase().as_str() {
            "image" | "photo" => Ok(MediaKind::Image),
            "video" | "reel" | "reels" => Ok(MediaKind::Video),
            other => Err(format!(
                "Invalid --media-type \"{other}\". Use \"image\" or \"video\"."
            )),
        },
        None => source.require_kind(),
    }
}

pub fn schedule_or_none(schedule: Option<&str>) -> Result<Option<String>, String> {
    match schedule {
        Some(s) => {
            let now_seconds = config::now_ms() / 1000;
            facebook::normalize_schedule(s, now_seconds).map(Some)
        }
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Configuration & accounts
// ---------------------------------------------------------------------------

pub fn cmd_setup(app_id: &str, app_secret: &str, config_id: Option<&str>) -> Result<(), String> {
    if app_id.trim().is_empty() || app_secret.trim().is_empty() {
        return Err("--app-id and --app-secret are both required.".to_string());
    }
    let config_id = config_id.map(str::trim).filter(|s| !s.is_empty());
    store::set_app_credentials(app_id.trim(), app_secret.trim(), config_id)?;
    eprintln!("Meta app credentials saved to {}", config::path().display());
    emit(&json!({
        "success": true,
        "platform": "meta",
        "app_id": app_id.trim(),
        "config_id": config_id,
        "config": config::path().display().to_string(),
    }));
    Ok(())
}

/// Adopt a token minted outside the login flow. Reads stdin when `--token` is
/// omitted, so the secret never has to land in the shell history.
pub async fn cmd_token(alias: &str, token: Option<String>) -> Result<(), String> {
    let token = match token {
        Some(t) => t,
        None => {
            eprintln!("Reading the token from stdin...");
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                .map_err(|e| format!("Failed to read the token from stdin: {e}"))?;
            buf
        }
    };
    auth::run_import_token(alias, &token).await
}

/// Every Meta account as a JSON array, for the platform-scoped `accounts`
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
        eprintln!("No Meta account configured. Run: rustedin meta auth --account=<alias>");
    }
    emit(&Value::Array(accounts));
    Ok(())
}

/// Status for every Meta account, keyed by alias. `check` additionally asks
/// Meta to validate each token.
pub async fn status_map(check: bool) -> Result<serde_json::Map<String, Value>, String> {
    let mut out = serde_json::Map::new();
    for alias in store::list_aliases()? {
        let account = store::get_account(&alias)?;
        let mut entry =
            serde_json::to_value(store::account_status(&account)).unwrap_or_else(|_| json!({}));

        if check {
            crate::note!("Checking the token for \"{alias}\" against Meta...");
            match auth::debug_token(&account.access_token).await {
                Ok(data) => entry["debug_token"] = data,
                Err(e) => entry["debug_token"] = json!({ "error": e }),
            }
        }
        out.insert(alias, entry);
    }
    Ok(out)
}

pub async fn cmd_status(check: bool) -> Result<(), String> {
    let map = status_map(check).await?;
    if map.is_empty() {
        eprintln!("No Meta account configured. Run: rustedin meta auth --account=<alias>");
    }
    emit(&Value::Object(map));
    Ok(())
}

pub async fn cmd_pages(alias: &str, refresh: bool) -> Result<(), String> {
    let mut account = store::get_account(alias)?;

    if refresh {
        crate::note!("Re-fetching Pages for \"{alias}\"...");
        let user_token = store::get_valid_user_token(alias).await?;
        let pages = auth::fetch_pages(&user_token).await?;
        // Drop a stored default that no longer exists.
        if let Some(def) = &account.default_page {
            if !pages.iter().any(|p| &p.id == def) {
                account.default_page = None;
            }
        }
        account.pages = pages;
        store::upsert_account(account.clone())?;
        crate::note!("Stored {} Page(s).", account.pages.len());
    }

    let pages: Vec<Value> = account
        .pages
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "category": p.category,
                "tasks": p.tasks,
                "is_default": account.default_page.as_deref() == Some(p.id.as_str()),
                "instagram": p.instagram.as_ref().map(|ig| json!({
                    "id": ig.id,
                    "username": ig.username,
                })),
            })
        })
        .collect();

    emit(&json!({
        "account": alias,
        "total": pages.len(),
        "pages": pages,
    }));
    Ok(())
}

pub fn cmd_use(alias: &str, page: Option<&str>) -> Result<(), String> {
    let account = store::get_account(alias)?;
    match page {
        Some(p) => {
            let resolved = store::resolve_page(&account, Some(p))?;
            store::set_default_page(alias, Some(resolved.id.clone()))?;
            eprintln!(
                "Default Page for \"{alias}\" set to {} ({}).",
                resolved.name, resolved.id
            );
            emit(&json!({
                "success": true,
                "account": alias,
                "default_page": { "id": resolved.id, "name": resolved.name },
            }));
        }
        None => {
            store::set_default_page(alias, None)?;
            eprintln!("Default Page cleared for \"{alias}\".");
            emit(&json!({ "success": true, "account": alias, "default_page": null }));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Facebook
// ---------------------------------------------------------------------------

pub struct FbPostArgs<'a> {
    pub account: &'a str,
    pub page: Option<&'a str>,
    pub message: Option<&'a str>,
    pub link: Option<&'a str>,
    pub schedule: Option<&'a str>,
    pub draft: bool,
}

pub async fn cmd_fb_post(args: FbPostArgs<'_>) -> Result<(), String> {
    let (_, page) = target(args.account, args.page)?;
    let publish = Publish {
        schedule: schedule_or_none(args.schedule)?,
        draft: args.draft,
    };

    let result = facebook::create_feed_post(
        &page,
        FeedPost {
            message: args.message,
            link: args.link,
            publish: publish.clone(),
        },
    )
    .await?;

    let post_id = result["id"].as_str().unwrap_or_default().to_string();
    emit(&json!({
        "success": true,
        "platform": "facebook",
        "account": args.account,
        "page": { "id": page.id, "name": page.name },
        "post_id": post_id,
        "permalink": facebook::permalink(&post_id),
        "scheduled_publish_time": publish.schedule,
        "published": publish.schedule.is_none() && !publish.draft,
    }));
    Ok(())
}

pub struct FbPhotoArgs<'a> {
    pub account: &'a str,
    pub page: Option<&'a str>,
    pub images: &'a [String],
    pub message: Option<&'a str>,
    pub schedule: Option<&'a str>,
    pub draft: bool,
}

pub async fn cmd_fb_photo(args: FbPhotoArgs<'_>) -> Result<(), String> {
    if args.images.is_empty() {
        return Err("At least one --image is required.".to_string());
    }
    let (_, page) = target(args.account, args.page)?;
    let publish = Publish {
        schedule: schedule_or_none(args.schedule)?,
        draft: args.draft,
    };

    let sources: Vec<MediaSource> = args
        .images
        .iter()
        .map(|s| MediaSource::parse(s))
        .collect::<Result<_, _>>()?;

    // One photo: a native photo post. Several: unpublished uploads stitched
    // together with `attached_media` on the feed edge.
    if sources.len() == 1 {
        let result = facebook::upload_photo(&page, &sources[0], args.message, &publish).await?;
        let photo_id = result["id"].as_str().unwrap_or_default().to_string();
        let post_id = result["post_id"].as_str().unwrap_or_default().to_string();

        emit(&json!({
            "success": true,
            "platform": "facebook",
            "account": args.account,
            "page": { "id": page.id, "name": page.name },
            "photo_id": photo_id,
            "post_id": post_id,
            "permalink": (!post_id.is_empty()).then(|| facebook::permalink(&post_id)),
            "scheduled_publish_time": publish.schedule,
        }));
        return Ok(());
    }

    let mut photo_ids = Vec::with_capacity(sources.len());
    for (i, source) in sources.iter().enumerate() {
        crate::note!("Uploading photo {}/{}...", i + 1, sources.len());
        let uploaded = facebook::upload_photo(
            &page,
            source,
            None,
            &Publish {
                schedule: None,
                draft: true,
            },
        )
        .await?;
        photo_ids.push(
            uploaded["id"]
                .as_str()
                .ok_or("A photo upload returned no id")?
                .to_string(),
        );
    }

    let result =
        facebook::create_multi_photo_post(&page, &photo_ids, args.message, &publish).await?;
    let post_id = result["id"].as_str().unwrap_or_default().to_string();

    emit(&json!({
        "success": true,
        "platform": "facebook",
        "account": args.account,
        "page": { "id": page.id, "name": page.name },
        "post_id": post_id,
        "permalink": facebook::permalink(&post_id),
        "photo_ids": photo_ids,
        "scheduled_publish_time": publish.schedule,
    }));
    Ok(())
}

pub struct FbVideoArgs<'a> {
    pub account: &'a str,
    pub page: Option<&'a str>,
    pub video: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub schedule: Option<&'a str>,
    pub draft: bool,
}

pub async fn cmd_fb_video(args: FbVideoArgs<'_>) -> Result<(), String> {
    let (_, page) = target(args.account, args.page)?;
    let source = MediaSource::parse(args.video)?;
    let publish = Publish {
        schedule: schedule_or_none(args.schedule)?,
        draft: args.draft,
    };

    let result = facebook::upload_video(
        &page,
        &source,
        VideoPost {
            title: args.title,
            description: args.description,
            publish: publish.clone(),
        },
    )
    .await?;

    let video_id = result["id"].as_str().unwrap_or_default().to_string();
    emit(&json!({
        "success": true,
        "platform": "facebook",
        "account": args.account,
        "page": { "id": page.id, "name": page.name },
        "video_id": video_id,
        "permalink": format!("https://www.facebook.com/{video_id}"),
        "scheduled_publish_time": publish.schedule,
    }));
    Ok(())
}

// ---------------------------------------------------------------------------
// Instagram
// ---------------------------------------------------------------------------

pub struct IgPostArgs<'a> {
    pub account: &'a str,
    pub page: Option<&'a str>,
    pub media: &'a [String],
    pub media_type: Option<&'a str>,
    pub surface: Surface,
    pub caption: Option<&'a str>,
    pub alt_text: Option<&'a str>,
    pub location_id: Option<&'a str>,
    pub collaborators: Option<&'a str>,
    pub cover_url: Option<&'a str>,
    pub thumb_offset: Option<u64>,
    pub share_to_feed: Option<bool>,
    pub ai_generated: bool,
    pub cleanup_relay: bool,
}

pub async fn cmd_ig_post(args: IgPostArgs<'_>) -> Result<(), String> {
    let count = args.media.len();
    match (args.surface, count) {
        (_, 0) => return Err("At least one --media is required.".to_string()),
        (Surface::Stories, n) if n > 1 => {
            return Err("A Story takes exactly one --media.".to_string())
        }
        _ => {}
    }

    if count > 1 {
        return cmd_ig_carousel(args).await;
    }

    let (_, page, ig) = target_instagram(args.account, args.page)?;

    let source = MediaSource::parse(&args.media[0])?;
    let kind = parse_media_kind(args.media_type, &source)?;

    let opts = PostOptions {
        caption: args.caption,
        alt_text: args.alt_text,
        location_id: args.location_id,
        collaborators: args.collaborators,
        cover_url: args.cover_url,
        thumb_offset: args.thumb_offset,
        share_to_feed: args.share_to_feed,
        is_ai_generated: args.ai_generated,
    };

    let published =
        instagram::publish_single(&page, &ig, &source, kind, args.surface, &opts).await?;

    cleanup_relays(&page, &published.relay_photo_ids, args.cleanup_relay).await;

    emit(&json!({
        "success": true,
        "platform": "instagram",
        "surface": args.surface.name(),
        "account": args.account,
        "page": { "id": page.id, "name": page.name },
        "instagram": { "id": ig.id, "username": ig.username },
        "media_id": published.media_id,
        "creation_id": published.creation_id,
        "permalink": published.permalink,
        "relay_photo_ids": published.relay_photo_ids,
    }));
    Ok(())
}

async fn cmd_ig_carousel(args: IgPostArgs<'_>) -> Result<(), String> {
    let count = args.media.len();
    if !(instagram::MIN_CAROUSEL_ITEMS..=instagram::MAX_CAROUSEL_ITEMS).contains(&count) {
        return Err(format!(
            "A carousel takes between {} and {} items — {count} given.",
            instagram::MIN_CAROUSEL_ITEMS,
            instagram::MAX_CAROUSEL_ITEMS
        ));
    }

    let (_, page, ig) = target_instagram(args.account, args.page)?;
    instagram::check_caption(args.caption)?;

    let mut child_ids = Vec::with_capacity(count);
    let mut relay_ids = Vec::new();

    for (i, raw) in args.media.iter().enumerate() {
        crate::note!("Preparing carousel item {}/{count}...", i + 1);

        let source = MediaSource::parse(raw)?;
        let kind = parse_media_kind(args.media_type, &source)?;
        let ingested = instagram::ingest(&page, &source, kind).await?;
        relay_ids.extend(ingested.relay_photo_ids.clone());

        let form =
            instagram::container_params(&ingested, Surface::Feed, true, &PostOptions::default());
        let child_id = instagram::create_container(&page, &ig, &form).await?;

        if let Some(bytes) = ingested.resumable_bytes {
            instagram::resumable_upload(&page, &child_id, bytes).await?;
        }
        instagram::wait_until_ready(&page, &child_id, kind).await?;
        child_ids.push(child_id);
    }

    let mut form = vec![
        ("media_type".to_string(), "CAROUSEL".to_string()),
        ("children".to_string(), child_ids.join(",")),
    ];
    if let Some(c) = args.caption {
        form.push(("caption".to_string(), c.to_string()));
    }
    if let Some(l) = args.location_id {
        form.push(("location_id".to_string(), l.to_string()));
    }
    if args.ai_generated {
        form.push(("is_ai_generated".to_string(), "true".to_string()));
    }

    let container_id = instagram::create_container(&page, &ig, &form).await?;
    crate::note!("Carousel container {container_id} created.");
    instagram::wait_until_ready(&page, &container_id, MediaKind::Image).await?;

    let published = instagram::publish(&page, &ig, &container_id).await?;
    let media_id = published["id"].as_str().unwrap_or_default().to_string();
    let permalink = instagram::permalink(&page, &media_id).await;

    cleanup_relays(&page, &relay_ids, args.cleanup_relay).await;

    emit(&json!({
        "success": true,
        "platform": "instagram",
        "surface": "carousel",
        "account": args.account,
        "page": { "id": page.id, "name": page.name },
        "instagram": { "id": ig.id, "username": ig.username },
        "media_id": media_id,
        "creation_id": container_id,
        "children": child_ids,
        "permalink": permalink,
        "relay_photo_ids": relay_ids,
    }));
    Ok(())
}

/// Publish a container that was already created — the recovery path when
/// `wait_until_ready` timed out but Instagram finished processing afterwards.
pub async fn cmd_ig_publish(
    alias: &str,
    page: Option<&str>,
    creation_id: &str,
) -> Result<(), String> {
    let (_, page, ig) = target_instagram(alias, page)?;
    let published = instagram::publish(&page, &ig, creation_id).await?;
    let media_id = published["id"].as_str().unwrap_or_default().to_string();
    let permalink = instagram::permalink(&page, &media_id).await;

    emit(&json!({
        "success": true,
        "platform": "instagram",
        "account": alias,
        "instagram": { "id": ig.id, "username": ig.username },
        "creation_id": creation_id,
        "media_id": media_id,
        "permalink": permalink,
    }));
    Ok(())
}

pub async fn cmd_ig_limit(alias: &str, page: Option<&str>) -> Result<(), String> {
    let (_, page, ig) = target_instagram(alias, page)?;
    let data = instagram::publishing_limit(&page, &ig).await?;
    // The quota is wrapped in a single-element `data` array; unwrap it when
    // present, otherwise surface whatever Meta returned.
    let quota = data["data"].as_array().and_then(|a| a.first()).cloned();

    emit(&json!({
        "account": alias,
        "instagram": { "id": ig.id, "username": ig.username },
        "limit": quota.unwrap_or(data),
    }));
    Ok(())
}

pub async fn cleanup_relays(page: &Page, relay_ids: &[String], enabled: bool) {
    if relay_ids.is_empty() {
        return;
    }
    if enabled {
        for id in relay_ids {
            facebook::delete_photo(page, id).await;
        }
    } else {
        crate::note!(
            "{} unpublished relay photo(s) left on Page \"{}\": {}. \
             Pass --cleanup-relay to delete them automatically.",
            relay_ids.len(),
            page.name,
            relay_ids.join(", ")
        );
    }
}

// ---------------------------------------------------------------------------
// Debug
// ---------------------------------------------------------------------------

/// Raw Graph API read — the escape hatch for anything rustedin doesn't wrap.
pub async fn cmd_get(
    alias: &str,
    page: Option<&str>,
    path: &str,
    query: &[String],
) -> Result<(), String> {
    let params: Vec<(String, String)> = query
        .iter()
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => Ok((k.to_string(), v.to_string())),
            None => Err(format!("Invalid --query \"{kv}\". Expected key=value.")),
        })
        .collect::<Result<_, _>>()?;

    // A Page token when a Page is in play, the user token otherwise.
    let token = match page {
        Some(_) => target(alias, page)?.1.access_token,
        None => {
            let account = store::get_account(alias)?;
            match store::resolve_page(&account, None) {
                Ok(p) => p.access_token,
                Err(_) => store::get_valid_user_token(alias).await?,
            }
        }
    };

    let data = api::get(&token, path, &params).await?;
    emit(&data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_kind_override_accepts_aliases() {
        let s = MediaSource::parse("https://x/asset/1").unwrap();
        assert_eq!(
            parse_media_kind(Some("photo"), &s).unwrap(),
            MediaKind::Image
        );
        assert_eq!(
            parse_media_kind(Some("REEL"), &s).unwrap(),
            MediaKind::Video
        );
        assert!(parse_media_kind(Some("gif"), &s)
            .unwrap_err()
            .contains("image"));
    }

    #[test]
    fn media_kind_falls_back_to_the_extension() {
        let s = MediaSource::parse("https://x/a.mp4").unwrap();
        assert_eq!(parse_media_kind(None, &s).unwrap(), MediaKind::Video);
    }
}
