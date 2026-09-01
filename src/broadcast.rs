//! Cross-posting: one piece of content, several platforms, in parallel.
//!
//! `broadcast` is the reason the three platforms share a binary. It does not
//! reimplement publishing — it maps a common `--text` / `--image` / `--link`
//! onto whatever each platform actually accepts, and calls the very same
//! primitives the single-target commands use:
//!
//! | | text only | + image | + link |
//! |---|---|---|---|
//! | LinkedIn  | text post | image post | article card |
//! | Facebook  | feed post | photo post | link preview |
//! | Instagram | *rejected* | feed post | appended to the caption |
//!
//! Failures are per target: one refusal never cancels the others, every target
//! reports its own outcome, and the process exits non-zero if any of them
//! failed.

use crate::core::output::emit;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    LinkedIn,
    Facebook,
    Instagram,
}

impl Platform {
    fn name(self) -> &'static str {
        match self {
            Platform::LinkedIn => "linkedin",
            Platform::Facebook => "facebook",
            Platform::Instagram => "instagram",
        }
    }
}

/// One `platform:account[/page]` destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub platform: Platform,
    pub account: String,
    /// Meta only: which Page of the account to act on.
    pub page: Option<String>,
}

impl Target {
    fn label(&self) -> String {
        match &self.page {
            Some(p) => format!("{}:{}/{p}", self.platform.name(), self.account),
            None => format!("{}:{}", self.platform.name(), self.account),
        }
    }
}

/// Parse `platform:account[/page]`.
pub fn parse_target(raw: &str) -> Result<Target, String> {
    let raw = raw.trim();
    let (platform, rest) = raw.split_once(':').ok_or_else(|| {
        format!(
            "Invalid target \"{raw}\". Expected platform:account[/page], \
             e.g. linkedin:quentin or facebook:z29k/My Page."
        )
    })?;

    let platform = match platform.trim().to_lowercase().as_str() {
        "linkedin" | "li" => Platform::LinkedIn,
        "facebook" | "fb" => Platform::Facebook,
        "instagram" | "ig" => Platform::Instagram,
        other => {
            return Err(format!(
                "Unknown platform \"{other}\" in target \"{raw}\". \
                 Use linkedin, facebook or instagram."
            ))
        }
    };

    let (account, page) = match rest.split_once('/') {
        Some((a, p)) => (a.trim(), Some(p.trim().to_string())),
        None => (rest.trim(), None),
    };
    if account.is_empty() {
        return Err(format!("Target \"{raw}\" names no account."));
    }
    if platform == Platform::LinkedIn && page.is_some() {
        return Err(format!(
            "Target \"{raw}\" carries a Page, which only Facebook and Instagram have."
        ));
    }

    Ok(Target {
        platform,
        account: account.to_string(),
        page,
    })
}

/// The content to fan out, already owned so each target can be driven from its
/// own task.
#[derive(Debug, Clone)]
pub struct Content {
    pub text: String,
    /// Raw `--image` spec: a local path or a public URL.
    pub image: Option<String>,
    pub link: Option<String>,
    /// Article/image title. Derived from the text when absent.
    pub title: Option<String>,
    /// LinkedIn only.
    pub visibility: String,
    /// Facebook only.
    pub schedule: Option<String>,
    /// Instagram only.
    pub alt_text: Option<String>,
    pub cleanup_relay: bool,
}

impl Content {
    /// A title for the platforms that demand one: the first line of the text,
    /// else the link's host.
    fn title(&self) -> String {
        if let Some(t) = &self.title {
            return t.clone();
        }
        let first_line = self
            .text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("");
        if !first_line.is_empty() {
            return first_line.chars().take(400).collect();
        }
        self.link
            .as_deref()
            .and_then(host_of)
            .unwrap_or("Shared link")
            .to_string()
    }

    /// Text with the link appended, for the platforms that have nowhere else to
    /// put it.
    fn text_with_link(&self) -> String {
        match (&self.link, self.text.trim().is_empty()) {
            (Some(link), true) => link.clone(),
            (Some(link), false) => format!("{}\n\n{link}", self.text),
            (None, _) => self.text.clone(),
        }
    }
}

fn host_of(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    after_scheme
        .split(['/', '?', '#'])
        .next()
        .filter(|h| !h.is_empty())
}

pub struct BroadcastArgs<'a> {
    pub targets: &'a [String],
    pub content: Content,
    pub dry_run: bool,
}

pub async fn run(args: BroadcastArgs<'_>) -> Result<(), String> {
    if args.targets.is_empty() {
        return Err("At least one --to target is required.".to_string());
    }
    let targets: Vec<Target> = args
        .targets
        .iter()
        .map(|t| parse_target(t))
        .collect::<Result<_, _>>()?;

    let content = args.content;
    check_content(&targets, &content)?;

    if args.dry_run {
        emit(&json!({
            "dry_run": true,
            "total": targets.len(),
            "plan": targets.iter().map(|t| plan_for(t, &content)).collect::<Vec<_>>(),
        }));
        return Ok(());
    }

    // Refresh LinkedIn tokens before fanning out: two tasks renewing at once
    // would each rewrite the config file from a stale copy.
    #[cfg(feature = "linkedin")]
    {
        let aliases: Vec<String> = targets
            .iter()
            .filter(|t| t.platform == Platform::LinkedIn)
            .map(|t| t.account.clone())
            .collect();
        crate::providers::linkedin::store::warm_tokens(&aliases).await?;
    }

    // Labels are taken before the targets are moved into their tasks.
    let labels: Vec<String> = targets.iter().map(Target::label).collect();

    let handles: Vec<_> = targets
        .into_iter()
        .map(|target| {
            let content = content.clone();
            tokio::spawn(async move { publish(&target, &content).await })
        })
        .collect();

    let mut results = Vec::with_capacity(labels.len());
    for (i, handle) in handles.into_iter().enumerate() {
        let label = labels[i].clone();
        let entry = match handle.await {
            Ok(Ok(mut value)) => {
                value["target"] = json!(label);
                value["success"] = json!(true);
                value
            }
            Ok(Err(e)) => json!({ "target": label, "success": false, "error": e }),
            Err(e) => json!({ "target": label, "success": false, "error": e.to_string() }),
        };
        results.push(entry);
    }

    let succeeded = results.iter().filter(|r| r["success"] == true).count();
    let failed = results.len() - succeeded;

    emit(&json!({
        "total": results.len(),
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
    }));

    if failed > 0 {
        return Err(format!(
            "{failed} of {} targets failed — see the JSON report on stdout.",
            results.len()
        ));
    }
    Ok(())
}

/// Reject up front what no target could accept, and warn about the options a
/// given target will ignore.
fn check_content(targets: &[Target], content: &Content) -> Result<(), String> {
    if content.text.trim().is_empty() && content.image.is_none() && content.link.is_none() {
        return Err("A broadcast needs at least --text, --image or --link.".to_string());
    }
    if content.image.is_none() && targets.iter().any(|t| t.platform == Platform::Instagram) {
        return Err(
            "Instagram cannot publish without media — pass --image, or drop the instagram target."
                .to_string(),
        );
    }
    if content.schedule.is_some() && targets.iter().any(|t| t.platform != Platform::Facebook) {
        crate::note!(
            "Warning: --schedule only applies to Facebook; the other targets publish immediately."
        );
    }
    if content.link.is_some() && targets.iter().any(|t| t.platform == Platform::Instagram) {
        crate::note!(
            "Warning: Instagram does not linkify captions; the link is appended as plain text."
        );
    }
    Ok(())
}

/// What each target would do, for `--dry-run`.
fn plan_for(target: &Target, content: &Content) -> Value {
    let action = match (
        target.platform,
        content.image.is_some(),
        content.link.is_some(),
    ) {
        // A link wins on LinkedIn: the card carries the URL, and the image
        // becomes its thumbnail. Keep these arms in the same order as
        // `publish_linkedin`, or the plan will misreport what happens.
        (Platform::LinkedIn, _, true) => "article card",
        (Platform::LinkedIn, true, false) => "image post",
        (Platform::LinkedIn, false, false) => "text post",
        (Platform::Facebook, true, _) => "photo post",
        (Platform::Facebook, false, _) => "feed post",
        (Platform::Instagram, _, _) => "feed post",
    };
    json!({
        "target": target.label(),
        "platform": target.platform.name(),
        "account": target.account,
        "page": target.page,
        "action": action,
        "text": match target.platform {
            Platform::Instagram | Platform::Facebook if content.image.is_some() =>
                content.text_with_link(),
            _ => content.text.clone(),
        },
        "image": content.image,
        "link": content.link,
    })
}

async fn publish(target: &Target, content: &Content) -> Result<Value, String> {
    match target.platform {
        Platform::LinkedIn => publish_linkedin(target, content).await,
        Platform::Facebook => publish_facebook(target, content).await,
        Platform::Instagram => publish_instagram(target, content).await,
    }
}

// ---------------------------------------------------------------------------
// LinkedIn
// ---------------------------------------------------------------------------

#[cfg(feature = "linkedin")]
async fn publish_linkedin(target: &Target, content: &Content) -> Result<Value, String> {
    use crate::core::media::MediaSource;
    use crate::providers::linkedin::publish::{self, Article};

    let alias = target.account.as_str();
    let image = match &content.image {
        Some(spec) => Some(MediaSource::parse(spec)?),
        None => None,
    };

    match (&image, &content.link) {
        // A link always wins: an article card carries both the URL and the
        // image as its thumbnail.
        (_, Some(link)) => {
            let (post_id, image_urn) = publish::article_post(
                alias,
                Article {
                    url: link,
                    title: &content.title(),
                    description: None,
                    commentary: &content.text,
                    visibility: &content.visibility,
                    image: image.as_ref(),
                },
            )
            .await?;
            Ok(json!({
                "platform": "linkedin", "kind": "article",
                "post_id": post_id, "image_urn": image_urn,
            }))
        }
        (Some(source), None) => {
            let (post_id, image_urn) = publish::image_post(
                alias,
                &content.text,
                source,
                &content.title(),
                &content.visibility,
            )
            .await?;
            Ok(json!({
                "platform": "linkedin", "kind": "image",
                "post_id": post_id, "image_urn": image_urn,
            }))
        }
        (None, None) => {
            let post_id = publish::text_post(alias, &content.text, &content.visibility).await?;
            Ok(json!({ "platform": "linkedin", "kind": "text", "post_id": post_id }))
        }
    }
}

#[cfg(not(feature = "linkedin"))]
async fn publish_linkedin(_: &Target, _: &Content) -> Result<Value, String> {
    Err("This build has the `linkedin` feature disabled.".to_string())
}

// ---------------------------------------------------------------------------
// Facebook
// ---------------------------------------------------------------------------

#[cfg(feature = "meta")]
async fn publish_facebook(target: &Target, content: &Content) -> Result<Value, String> {
    use crate::core::media::MediaSource;
    use crate::providers::meta::commands::{schedule_or_none, target as resolve_target};
    use crate::providers::meta::facebook::{self, FeedPost, Publish};

    let (_, page) = resolve_target(&target.account, target.page.as_deref())?;
    let publish = Publish {
        schedule: schedule_or_none(content.schedule.as_deref())?,
        draft: false,
    };

    match &content.image {
        // A photo post has no `link` field, so the URL rides in the caption
        // where Facebook linkifies it anyway.
        Some(spec) => {
            let source = MediaSource::parse(spec)?;
            let caption = content.text_with_link();
            let result = facebook::upload_photo(
                &page,
                &source,
                (!caption.is_empty()).then_some(caption.as_str()),
                &publish,
            )
            .await?;
            let post_id = result["post_id"].as_str().unwrap_or_default().to_string();
            Ok(json!({
                "platform": "facebook",
                "kind": "photo",
                "page": { "id": page.id, "name": page.name },
                "photo_id": result["id"].as_str(),
                "post_id": post_id,
                "permalink": (!post_id.is_empty()).then(|| facebook::permalink(&post_id)),
                "scheduled_publish_time": publish.schedule,
            }))
        }
        None => {
            let text = content.text.clone();
            let result = facebook::create_feed_post(
                &page,
                FeedPost {
                    message: (!text.trim().is_empty()).then_some(text.as_str()),
                    link: content.link.as_deref(),
                    publish: publish.clone(),
                },
            )
            .await?;
            let post_id = result["id"].as_str().unwrap_or_default().to_string();
            Ok(json!({
                "platform": "facebook",
                "kind": "feed",
                "page": { "id": page.id, "name": page.name },
                "post_id": post_id,
                "permalink": facebook::permalink(&post_id),
                "scheduled_publish_time": publish.schedule,
            }))
        }
    }
}

#[cfg(not(feature = "meta"))]
async fn publish_facebook(_: &Target, _: &Content) -> Result<Value, String> {
    Err("This build has the `meta` feature disabled.".to_string())
}

// ---------------------------------------------------------------------------
// Instagram
// ---------------------------------------------------------------------------

#[cfg(feature = "meta")]
async fn publish_instagram(target: &Target, content: &Content) -> Result<Value, String> {
    use crate::core::media::MediaSource;
    use crate::providers::meta::commands::{cleanup_relays, target_instagram};
    use crate::providers::meta::instagram::{self, PostOptions, Surface};

    let spec = content
        .image
        .as_deref()
        .ok_or("Instagram cannot publish without media.")?;
    let source = MediaSource::parse(spec)?;
    let kind = source.require_kind()?;

    let (_, page, ig) = target_instagram(&target.account, target.page.as_deref())?;
    let caption = content.text_with_link();

    let published = instagram::publish_single(
        &page,
        &ig,
        &source,
        kind,
        Surface::Feed,
        &PostOptions {
            caption: (!caption.is_empty()).then_some(caption.as_str()),
            alt_text: content.alt_text.as_deref(),
            ..Default::default()
        },
    )
    .await?;

    cleanup_relays(&page, &published.relay_photo_ids, content.cleanup_relay).await;

    Ok(json!({
        "platform": "instagram",
        "kind": "feed",
        "page": { "id": page.id, "name": page.name },
        "instagram": { "id": ig.id, "username": ig.username },
        "media_id": published.media_id,
        "creation_id": published.creation_id,
        "permalink": published.permalink,
        "relay_photo_ids": published.relay_photo_ids,
    }))
}

#[cfg(not(feature = "meta"))]
async fn publish_instagram(_: &Target, _: &Content) -> Result<Value, String> {
    Err("This build has the `meta` feature disabled.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(text: &str, image: Option<&str>, link: Option<&str>) -> Content {
        Content {
            text: text.to_string(),
            image: image.map(str::to_string),
            link: link.map(str::to_string),
            title: None,
            visibility: "PUBLIC".to_string(),
            schedule: None,
            alt_text: None,
            cleanup_relay: false,
        }
    }

    #[test]
    fn targets_parse_with_and_without_a_page() {
        assert_eq!(
            parse_target("linkedin:quentin").unwrap(),
            Target {
                platform: Platform::LinkedIn,
                account: "quentin".to_string(),
                page: None
            }
        );
        let t = parse_target("facebook:z29k/My Page").unwrap();
        assert_eq!(t.platform, Platform::Facebook);
        assert_eq!(t.account, "z29k");
        assert_eq!(t.page.as_deref(), Some("My Page"));
    }

    #[test]
    fn short_platform_names_are_accepted() {
        assert_eq!(parse_target("li:a").unwrap().platform, Platform::LinkedIn);
        assert_eq!(parse_target("fb:a").unwrap().platform, Platform::Facebook);
        assert_eq!(parse_target("IG:a").unwrap().platform, Platform::Instagram);
    }

    #[test]
    fn malformed_targets_explain_the_syntax() {
        assert!(parse_target("quentin")
            .unwrap_err()
            .contains("platform:account"));
        assert!(parse_target("twitter:me").unwrap_err().contains("Unknown"));
        assert!(parse_target("linkedin:")
            .unwrap_err()
            .contains("no account"));
        assert!(parse_target("linkedin:me/page")
            .unwrap_err()
            .contains("only Facebook and Instagram"));
    }

    #[test]
    fn a_page_name_may_contain_a_slash() {
        let t = parse_target("fb:z29k/Bar / Grill").unwrap();
        assert_eq!(t.page.as_deref(), Some("Bar / Grill"));
    }

    #[test]
    fn the_title_falls_back_to_the_first_line_then_the_host() {
        assert_eq!(content("Hello\nworld", None, None).title(), "Hello");
        assert_eq!(
            content("  \n Second line", None, None).title(),
            "Second line"
        );
        assert_eq!(
            content("", None, Some("https://z29k.fr/blog/a?b=1")).title(),
            "z29k.fr"
        );
        assert_eq!(content("", None, None).title(), "Shared link");
    }

    #[test]
    fn an_explicit_title_wins() {
        let mut c = content("Hello", None, None);
        c.title = Some("Explicit".to_string());
        assert_eq!(c.title(), "Explicit");
    }

    #[test]
    fn the_link_is_appended_where_it_cannot_be_a_field() {
        let c = content("Look at this", None, Some("https://z29k.fr"));
        assert_eq!(c.text_with_link(), "Look at this\n\nhttps://z29k.fr");
        assert_eq!(
            content("", None, Some("https://z29k.fr")).text_with_link(),
            "https://z29k.fr"
        );
        assert_eq!(
            content("Just text", None, None).text_with_link(),
            "Just text"
        );
    }

    #[test]
    fn instagram_without_media_is_refused_before_anything_is_published() {
        let targets = vec![parse_target("instagram:z29k").unwrap()];
        let err = check_content(&targets, &content("hi", None, None)).unwrap_err();
        assert!(err.contains("Instagram cannot publish without media"));
    }

    #[test]
    fn an_empty_broadcast_is_refused() {
        let targets = vec![parse_target("linkedin:me").unwrap()];
        let err = check_content(&targets, &content("  ", None, None)).unwrap_err();
        assert!(err.contains("--text"));
    }

    #[test]
    fn the_plan_names_what_each_target_would_do() {
        let c = content("Hi", Some("a.jpg"), None);
        let li = plan_for(&parse_target("li:me").unwrap(), &c);
        assert_eq!(li["action"], "image post");
        let fb = plan_for(&parse_target("fb:me").unwrap(), &c);
        assert_eq!(fb["action"], "photo post");

        let c = content("Hi", None, Some("https://z29k.fr"));
        assert_eq!(
            plan_for(&parse_target("li:me").unwrap(), &c)["action"],
            "article card"
        );
        assert_eq!(
            plan_for(&parse_target("fb:me").unwrap(), &c)["action"],
            "feed post"
        );

        // Image *and* link: LinkedIn publishes a card carrying the image as its
        // thumbnail, Facebook a photo post with the URL in the caption.
        let c = content("Hi", Some("a.jpg"), Some("https://z29k.fr"));
        assert_eq!(
            plan_for(&parse_target("li:me").unwrap(), &c)["action"],
            "article card"
        );
        assert_eq!(
            plan_for(&parse_target("fb:me").unwrap(), &c)["action"],
            "photo post"
        );
    }

    #[test]
    fn host_extraction_ignores_scheme_path_and_query() {
        assert_eq!(host_of("https://z29k.fr/a/b?c=1"), Some("z29k.fr"));
        assert_eq!(host_of("z29k.fr"), Some("z29k.fr"));
        assert_eq!(host_of("https://"), None);
    }
}
