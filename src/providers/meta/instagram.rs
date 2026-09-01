//! Publishing to an Instagram Professional account.
//!
//! Instagram never takes the media inline. Every publish is a handshake:
//!
//! 1. `POST /{ig-user-id}/media` creates a *container* pointing at a public
//!    media URL (or, for local video, a resumable upload slot);
//! 2. the container is polled until `status_code=FINISHED`;
//! 3. `POST /{ig-user-id}/media_publish` turns it into a real post.
//!
//! Local **images** have no upload endpoint at all, so rustedin relays them:
//! the file is pushed to the linked Facebook Page as an unpublished photo, and
//! the resulting CDN URL is what the container ingests.

use super::api;
use super::facebook;
use super::store::{InstagramAccount, Page};
use crate::core::media::{human_size, MediaKind, MediaSource};
use serde_json::Value;
use std::time::Duration;

pub const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_VIDEO_BYTES: u64 = 1024 * 1024 * 1024;
pub const MIN_CAROUSEL_ITEMS: usize = 2;
pub const MAX_CAROUSEL_ITEMS: usize = 10;
pub const MAX_CAPTION_CHARS: usize = 2200;

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const IMAGE_TIMEOUT: Duration = Duration::from_secs(180);
const VIDEO_TIMEOUT: Duration = Duration::from_secs(900);

/// Where the media lands. Instagram calls this the media product type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// A regular feed post — an image, or a video published as a Reel.
    Feed,
    /// Explicitly a Reel.
    Reels,
    /// A 24-hour Story.
    Stories,
}

impl Surface {
    pub fn name(self) -> &'static str {
        match self {
            Surface::Feed => "feed",
            Surface::Reels => "reels",
            Surface::Stories => "stories",
        }
    }
}

/// Optional container fields. Everything here is Instagram-specific and
/// silently ignored by the surfaces that don't support it.
#[derive(Debug, Clone, Default)]
pub struct PostOptions<'a> {
    pub caption: Option<&'a str>,
    pub alt_text: Option<&'a str>,
    pub location_id: Option<&'a str>,
    /// Comma-separated Instagram usernames, sent as a JSON array.
    pub collaborators: Option<&'a str>,
    pub cover_url: Option<&'a str>,
    /// Milliseconds into the video to use as the thumbnail.
    pub thumb_offset: Option<u64>,
    /// Stories only: also push the Reel to the main feed.
    pub share_to_feed: Option<bool>,
    pub is_ai_generated: bool,
}

/// Media that has been made ingestible by Instagram, plus any Facebook photo
/// IDs created along the way so the caller can clean them up.
pub struct Ingested {
    pub url: Option<String>,
    pub kind: MediaKind,
    pub relay_photo_ids: Vec<String>,
    /// Local video: the bytes must be pushed to the container after creation.
    pub resumable_bytes: Option<Vec<u8>>,
}

/// Instagram only ingests JPEG for images. Warn rather than fail: Meta
/// sometimes transcodes, and a hard error would be worse than a heads-up.
pub fn warn_if_not_jpeg(source: &MediaSource) {
    if let Some(ext) = source.extension() {
        if ext != "jpg" && ext != "jpeg" {
            crate::note!(
                "Warning: Instagram officially supports JPEG only; \"{}\" is .{ext}. \
                 Publishing may fail.",
                source.as_str()
            );
        }
    }
}

/// Turn any [`MediaSource`] into something a container can reference.
pub async fn ingest(
    page: &Page,
    source: &MediaSource,
    kind: MediaKind,
) -> Result<Ingested, String> {
    match (source, kind) {
        (MediaSource::Url(url), _) => Ok(Ingested {
            url: Some(url.clone()),
            kind,
            relay_photo_ids: Vec::new(),
            resumable_bytes: None,
        }),

        // Local image → relayed through the Page as an unpublished photo.
        (MediaSource::File(_), MediaKind::Image) => {
            source.check_size(MAX_IMAGE_BYTES, "an Instagram image")?;
            warn_if_not_jpeg(source);
            crate::note!(
                "Instagram cannot receive local images directly; relaying \"{}\" through \
                 Page \"{}\"...",
                source.file_name(),
                page.name
            );

            let uploaded = facebook::upload_photo(
                page,
                source,
                None,
                &facebook::Publish {
                    schedule: None,
                    draft: true,
                },
            )
            .await?;
            let photo_id = uploaded["id"]
                .as_str()
                .ok_or("The relay upload returned no photo id")?
                .to_string();
            let url = facebook::photo_source_url(page, &photo_id).await?;
            crate::note!("Relay photo {photo_id} ready.");

            Ok(Ingested {
                url: Some(url),
                kind,
                relay_photo_ids: vec![photo_id],
                resumable_bytes: None,
            })
        }

        // Local video → Instagram's resumable upload protocol.
        (MediaSource::File(_), MediaKind::Video) => {
            source.check_size(MAX_VIDEO_BYTES, "an Instagram video")?;
            Ok(Ingested {
                url: None,
                kind,
                relay_photo_ids: Vec::new(),
                resumable_bytes: Some(source.read_bytes()?),
            })
        }
    }
}

/// Build the `POST /{ig-user-id}/media` form for one piece of media.
pub fn container_params(
    ingested: &Ingested,
    surface: Surface,
    carousel_item: bool,
    opts: &PostOptions<'_>,
) -> Vec<(String, String)> {
    let mut form: Vec<(String, String)> = Vec::new();

    match (&ingested.url, ingested.kind) {
        (Some(url), MediaKind::Image) => form.push(("image_url".to_string(), url.clone())),
        (Some(url), MediaKind::Video) => form.push(("video_url".to_string(), url.clone())),
        // Resumable upload: no URL, the bytes follow the container creation.
        (None, _) => form.push(("upload_type".to_string(), "resumable".to_string())),
    }

    let media_type = match (carousel_item, surface, ingested.kind) {
        // Carousel children declare IMAGE / VIDEO, never REELS.
        (true, _, MediaKind::Video) => Some("VIDEO"),
        (true, _, MediaKind::Image) => None,
        (false, Surface::Stories, _) => Some("STORIES"),
        (false, _, MediaKind::Video) => Some("REELS"),
        (false, _, MediaKind::Image) => None,
    };
    if let Some(mt) = media_type {
        form.push(("media_type".to_string(), mt.to_string()));
    }
    if carousel_item {
        form.push(("is_carousel_item".to_string(), "true".to_string()));
    }

    // A carousel child never carries the caption — the parent container does.
    if !carousel_item {
        if let Some(c) = opts.caption {
            form.push(("caption".to_string(), c.to_string()));
        }
    }
    if let Some(a) = opts.alt_text {
        if ingested.kind == MediaKind::Image {
            form.push(("alt_text".to_string(), a.to_string()));
        }
    }
    if let Some(l) = opts.location_id {
        form.push(("location_id".to_string(), l.to_string()));
    }
    if let Some(c) = opts.cover_url {
        form.push(("cover_url".to_string(), c.to_string()));
    }
    if let Some(t) = opts.thumb_offset {
        form.push(("thumb_offset".to_string(), t.to_string()));
    }
    if surface == Surface::Stories {
        if let Some(s) = opts.share_to_feed {
            form.push(("share_to_feed".to_string(), s.to_string()));
        }
    }
    if opts.is_ai_generated {
        form.push(("is_ai_generated".to_string(), "true".to_string()));
    }
    if let Some(raw) = opts.collaborators {
        let names: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().trim_start_matches('@'))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if !names.is_empty() {
            form.push((
                "collaborators".to_string(),
                serde_json::to_string(&names).unwrap_or_default(),
            ));
        }
    }

    form
}

// ---------------------------------------------------------------------------
// Graph calls
// ---------------------------------------------------------------------------

pub async fn create_container(
    page: &Page,
    ig: &InstagramAccount,
    form: &[(String, String)],
) -> Result<String, String> {
    let data = api::post(&page.access_token, &format!("/{}/media", ig.id), form).await?;
    data["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("Container creation returned no id: {data}"))
}

/// Push local video bytes into a container created with `upload_type=resumable`.
pub async fn resumable_upload(
    page: &Page,
    container_id: &str,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let size = bytes.len();
    crate::note!(
        "Uploading video to container {container_id} ({})...",
        human_size(Some(size as u64))
    );

    let url = format!(
        "{}/ig-api-upload/{}/{container_id}",
        api::RUPLOAD_HOST,
        api::version()
    );
    api::post_bytes(
        &url,
        &[
            ("Authorization", format!("OAuth {}", page.access_token)),
            ("offset", "0".to_string()),
            ("file_size", size.to_string()),
        ],
        bytes,
        "POST rupload (resumable video)",
    )
    .await?;

    Ok(())
}

/// Poll `status_code` until the container is publishable.
pub async fn wait_until_ready(
    page: &Page,
    container_id: &str,
    kind: MediaKind,
) -> Result<(), String> {
    let timeout = match kind {
        MediaKind::Image => IMAGE_TIMEOUT,
        MediaKind::Video => VIDEO_TIMEOUT,
    };
    let started = std::time::Instant::now();
    let mut announced = false;

    loop {
        let data = api::get(
            &page.access_token,
            &format!("/{container_id}"),
            &[("fields".to_string(), "status_code,status".to_string())],
        )
        .await?;

        match data["status_code"].as_str().unwrap_or("") {
            "FINISHED" | "PUBLISHED" => return Ok(()),
            "ERROR" => {
                let detail = data["status"].as_str().unwrap_or("no detail provided");
                return Err(format!(
                    "Instagram failed to process container {container_id}: {detail}"
                ));
            }
            "EXPIRED" => {
                return Err(format!(
                    "Container {container_id} expired before it could be published \
                     (containers live 24 h)."
                ));
            }
            _ => {
                if !announced {
                    crate::note!("Waiting for Instagram to process {container_id}...");
                    announced = true;
                }
            }
        }

        if started.elapsed() >= timeout {
            return Err(format!(
                "Timed out after {}s waiting for container {container_id} to be ready. \
                 It may still finish — retry \
                 `rustedin instagram publish --creation-id={container_id}`.",
                timeout.as_secs()
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// The outcome of a completed publish.
pub struct Published {
    pub media_id: String,
    pub creation_id: String,
    pub permalink: Option<String>,
    /// Unpublished Page photos created to relay local images, for cleanup.
    pub relay_photo_ids: Vec<String>,
}

/// The whole handshake for one piece of media: ingest → container → upload →
/// poll → publish.
///
/// Shared by `instagram post/reel/story` and by `broadcast`, so the fan-out
/// command cannot drift from the single-target one.
pub async fn publish_single(
    page: &Page,
    ig: &InstagramAccount,
    source: &MediaSource,
    kind: MediaKind,
    surface: Surface,
    opts: &PostOptions<'_>,
) -> Result<Published, String> {
    check_caption(opts.caption)?;
    if surface == Surface::Reels && kind == MediaKind::Image {
        return Err("A Reel requires a video — use `instagram post` for images.".to_string());
    }
    if kind == MediaKind::Image && source.is_url() {
        warn_if_not_jpeg(source);
    }

    let ingested = ingest(page, source, kind).await?;
    let relay_photo_ids = ingested.relay_photo_ids.clone();

    let form = container_params(&ingested, surface, false, opts);
    let creation_id = create_container(page, ig, &form).await?;
    crate::note!("Container {creation_id} created.");

    if let Some(bytes) = ingested.resumable_bytes {
        resumable_upload(page, &creation_id, bytes).await?;
    }
    wait_until_ready(page, &creation_id, kind).await?;

    let published = publish(page, ig, &creation_id).await?;
    let media_id = published["id"].as_str().unwrap_or_default().to_string();
    let permalink = permalink(page, &media_id).await;

    Ok(Published {
        media_id,
        creation_id,
        permalink,
        relay_photo_ids,
    })
}

pub async fn publish(
    page: &Page,
    ig: &InstagramAccount,
    creation_id: &str,
) -> Result<Value, String> {
    api::post(
        &page.access_token,
        &format!("/{}/media_publish", ig.id),
        &[("creation_id".to_string(), creation_id.to_string())],
    )
    .await
}

/// `GET /{ig-user-id}/content_publishing_limit` — posts used in the rolling
/// 24-hour window (Meta allows 100).
pub async fn publishing_limit(page: &Page, ig: &InstagramAccount) -> Result<Value, String> {
    api::get(
        &page.access_token,
        &format!("/{}/content_publishing_limit", ig.id),
        &[("fields".to_string(), "config,quota_usage".to_string())],
    )
    .await
}

/// Best-effort permalink lookup so the CLI can print a clickable URL.
pub async fn permalink(page: &Page, media_id: &str) -> Option<String> {
    api::get(
        &page.access_token,
        &format!("/{media_id}"),
        &[("fields".to_string(), "permalink".to_string())],
    )
    .await
    .ok()
    .and_then(|v| v["permalink"].as_str().map(str::to_string))
}

pub fn check_caption(caption: Option<&str>) -> Result<(), String> {
    if let Some(c) = caption {
        let len = c.chars().count();
        if len > MAX_CAPTION_CHARS {
            return Err(format!(
                "Caption is {len} characters — Instagram allows at most {MAX_CAPTION_CHARS}."
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ingested(url: Option<&str>, kind: MediaKind) -> Ingested {
        Ingested {
            url: url.map(str::to_string),
            kind,
            relay_photo_ids: Vec::new(),
            resumable_bytes: None,
        }
    }

    fn find<'a>(form: &'a [(String, String)], key: &str) -> Option<&'a str> {
        form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn image_feed_container_has_no_media_type() {
        let opts = PostOptions {
            caption: Some("hello"),
            alt_text: Some("a cat"),
            ..Default::default()
        };
        let form = container_params(
            &ingested(Some("https://x/a.jpg"), MediaKind::Image),
            Surface::Feed,
            false,
            &opts,
        );
        assert_eq!(find(&form, "image_url"), Some("https://x/a.jpg"));
        assert_eq!(find(&form, "media_type"), None);
        assert_eq!(find(&form, "caption"), Some("hello"));
        assert_eq!(find(&form, "alt_text"), Some("a cat"));
    }

    #[test]
    fn feed_video_is_published_as_a_reel() {
        let form = container_params(
            &ingested(Some("https://x/a.mp4"), MediaKind::Video),
            Surface::Feed,
            false,
            &PostOptions::default(),
        );
        assert_eq!(find(&form, "video_url"), Some("https://x/a.mp4"));
        assert_eq!(find(&form, "media_type"), Some("REELS"));
    }

    #[test]
    fn stories_override_the_media_type() {
        let opts = PostOptions {
            share_to_feed: Some(true),
            ..Default::default()
        };
        let form = container_params(
            &ingested(Some("https://x/a.mp4"), MediaKind::Video),
            Surface::Stories,
            false,
            &opts,
        );
        assert_eq!(find(&form, "media_type"), Some("STORIES"));
        assert_eq!(find(&form, "share_to_feed"), Some("true"));
    }

    #[test]
    fn carousel_children_declare_video_not_reels_and_drop_the_caption() {
        let opts = PostOptions {
            caption: Some("parent caption"),
            ..Default::default()
        };
        let video = container_params(
            &ingested(Some("https://x/a.mp4"), MediaKind::Video),
            Surface::Feed,
            true,
            &opts,
        );
        assert_eq!(find(&video, "media_type"), Some("VIDEO"));
        assert_eq!(find(&video, "is_carousel_item"), Some("true"));
        assert_eq!(find(&video, "caption"), None);

        let image = container_params(
            &ingested(Some("https://x/a.jpg"), MediaKind::Image),
            Surface::Feed,
            true,
            &opts,
        );
        assert_eq!(find(&image, "media_type"), None);
        assert_eq!(find(&image, "is_carousel_item"), Some("true"));
    }

    #[test]
    fn a_urlless_container_requests_a_resumable_upload() {
        let form = container_params(
            &ingested(None, MediaKind::Video),
            Surface::Feed,
            false,
            &PostOptions::default(),
        );
        assert_eq!(find(&form, "upload_type"), Some("resumable"));
        assert_eq!(find(&form, "video_url"), None);
        assert_eq!(find(&form, "media_type"), Some("REELS"));
    }

    #[test]
    fn collaborators_are_normalised_into_a_json_array() {
        let opts = PostOptions {
            collaborators: Some("@alice, bob ,"),
            ..Default::default()
        };
        let form = container_params(
            &ingested(Some("https://x/a.jpg"), MediaKind::Image),
            Surface::Feed,
            false,
            &opts,
        );
        assert_eq!(find(&form, "collaborators"), Some(r#"["alice","bob"]"#));
    }

    #[test]
    fn alt_text_is_dropped_for_video() {
        let opts = PostOptions {
            alt_text: Some("ignored"),
            ..Default::default()
        };
        let form = container_params(
            &ingested(Some("https://x/a.mp4"), MediaKind::Video),
            Surface::Feed,
            false,
            &opts,
        );
        assert_eq!(find(&form, "alt_text"), None);
    }

    #[test]
    fn caption_length_is_enforced() {
        assert!(check_caption(None).is_ok());
        assert!(check_caption(Some(&"a".repeat(MAX_CAPTION_CHARS))).is_ok());
        let err = check_caption(Some(&"a".repeat(MAX_CAPTION_CHARS + 1))).unwrap_err();
        assert!(err.contains("2200"));
    }

    #[test]
    fn caption_length_counts_characters_not_bytes() {
        // 2200 emoji are 2200 characters but far more bytes.
        assert!(check_caption(Some(&"🎉".repeat(MAX_CAPTION_CHARS))).is_ok());
    }

    #[test]
    fn surface_names_are_stable() {
        assert_eq!(Surface::Feed.name(), "feed");
        assert_eq!(Surface::Reels.name(), "reels");
        assert_eq!(Surface::Stories.name(), "stories");
    }
}
