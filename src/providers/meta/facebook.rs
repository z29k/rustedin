//! Publishing to a Facebook Page.
//!
//! Everything here uses the **Page** access token, not the user token. Page
//! tokens derived from a long-lived user token do not expire, so these calls
//! keep working long after the user token would have lapsed.

use super::api;
use super::store::Page;
use crate::core::media::{human_size, MediaSource};
use serde_json::Value;

/// Generous ceilings that only catch obvious mistakes; Meta enforces the real
/// limits server-side.
pub const MAX_PHOTO_BYTES: u64 = 25 * 1024 * 1024;
/// Beyond this a video needs the resumable protocol, which rustedin does not
/// implement — pass a `--video` URL instead and let Meta fetch it.
pub const MAX_VIDEO_BYTES: u64 = 1024 * 1024 * 1024;

const MIN_SCHEDULE_LEAD_SECONDS: u64 = 10 * 60;
const MAX_SCHEDULE_LEAD_SECONDS: u64 = 75 * 24 * 60 * 60;

/// Common publishing controls shared by every Page endpoint.
#[derive(Debug, Clone, Default)]
pub struct Publish {
    /// `scheduled_publish_time`, already normalized.
    pub schedule: Option<String>,
    /// Create the object without publishing it (drafts, or carousel children).
    pub draft: bool,
}

impl Publish {
    /// `published` / `scheduled_publish_time` as Graph form fields.
    fn params(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        match &self.schedule {
            Some(when) => {
                out.push(("published".to_string(), "false".to_string()));
                out.push(("scheduled_publish_time".to_string(), when.clone()));
            }
            None if self.draft => out.push(("published".to_string(), "false".to_string())),
            None => out.push(("published".to_string(), "true".to_string())),
        }
        out
    }
}

/// Validate and normalize a `--schedule` value.
///
/// A bare integer is treated as a Unix timestamp in seconds and checked against
/// Meta's 10-minute / 75-day window. Anything else (ISO 8601, `strtotime`
/// expressions) is passed through for Meta to parse.
pub fn normalize_schedule(input: &str, now_seconds: u64) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("--schedule cannot be empty".to_string());
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Ok(trimmed.to_string());
    }

    let ts: u64 = trimmed
        .parse()
        .map_err(|_| format!("Invalid Unix timestamp: {trimmed}"))?;

    if ts < now_seconds.saturating_add(MIN_SCHEDULE_LEAD_SECONDS) {
        return Err(format!(
            "Scheduled time {ts} is too close: Facebook requires at least 10 minutes of lead time."
        ));
    }
    if ts > now_seconds.saturating_add(MAX_SCHEDULE_LEAD_SECONDS) {
        return Err(format!(
            "Scheduled time {ts} is too far out: Facebook allows at most 75 days ahead."
        ));
    }
    Ok(trimmed.to_string())
}

// ---------------------------------------------------------------------------
// Text / link posts
// ---------------------------------------------------------------------------

pub struct FeedPost<'a> {
    pub message: Option<&'a str>,
    pub link: Option<&'a str>,
    pub publish: Publish,
}

/// `POST /{page-id}/feed` — a text post, a link post, or both.
pub async fn create_feed_post(page: &Page, post: FeedPost<'_>) -> Result<Value, String> {
    if post.message.is_none() && post.link.is_none() {
        return Err("A Facebook post needs at least --message or --link.".to_string());
    }

    let mut form = post.publish.params();
    if let Some(m) = post.message {
        form.push(("message".to_string(), m.to_string()));
    }
    if let Some(l) = post.link {
        form.push(("link".to_string(), l.to_string()));
    }

    api::post(&page.access_token, &format!("/{}/feed", page.id), &form).await
}

// ---------------------------------------------------------------------------
// Photos
// ---------------------------------------------------------------------------

/// `POST /{page-id}/photos` — upload one photo, published or not.
///
/// A [`MediaSource::Url`] is handed to Meta as `url` and fetched server-side; a
/// [`MediaSource::File`] is uploaded as multipart `source`.
pub async fn upload_photo(
    page: &Page,
    source: &MediaSource,
    caption: Option<&str>,
    publish: &Publish,
) -> Result<Value, String> {
    source.check_size(MAX_PHOTO_BYTES, "a Facebook photo")?;

    let mut form = publish.params();
    if let Some(c) = caption {
        form.push(("caption".to_string(), c.to_string()));
    }

    let path = format!("/{}/photos", page.id);

    match source {
        MediaSource::Url(url) => {
            form.push(("url".to_string(), url.clone()));
            api::post(&page.access_token, &path, &form).await
        }
        MediaSource::File(_) => {
            crate::note!(
                "Uploading {} ({})...",
                source.file_name(),
                human_size(source.size())
            );
            api::post_multipart(
                api::GRAPH_HOST,
                &page.access_token,
                &path,
                &form,
                "source",
                &source.file_name(),
                source.read_bytes()?,
            )
            .await
        }
    }
}

/// `POST /{page-id}/feed` with `attached_media` — a single post carrying
/// several already-uploaded (unpublished) photos.
pub async fn create_multi_photo_post(
    page: &Page,
    photo_ids: &[String],
    message: Option<&str>,
    publish: &Publish,
) -> Result<Value, String> {
    let mut form = publish.params();
    if let Some(m) = message {
        form.push(("message".to_string(), m.to_string()));
    }
    for (i, id) in photo_ids.iter().enumerate() {
        form.push((
            format!("attached_media[{i}]"),
            serde_json::json!({ "media_fbid": id }).to_string(),
        ));
    }

    api::post(&page.access_token, &format!("/{}/feed", page.id), &form).await
}

/// Largest CDN URL Meta serves for an uploaded photo.
///
/// This is what makes local files usable on Instagram: an unpublished Page
/// photo yields a public URL the Instagram container can ingest.
pub async fn photo_source_url(page: &Page, photo_id: &str) -> Result<String, String> {
    let data = api::get(
        &page.access_token,
        &format!("/{photo_id}"),
        &[("fields".to_string(), "images".to_string())],
    )
    .await?;

    let images = data["images"]
        .as_array()
        .ok_or_else(|| format!("Photo {photo_id} returned no image renditions"))?;

    images
        .iter()
        .max_by_key(|img| img["width"].as_u64().unwrap_or(0) * img["height"].as_u64().unwrap_or(0))
        .and_then(|img| img["source"].as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("Photo {photo_id} has no usable source URL"))
}

/// Best-effort deletion of a relay photo. Failures are logged, never fatal.
pub async fn delete_photo(page: &Page, photo_id: &str) {
    match api::delete(&page.access_token, &format!("/{photo_id}")).await {
        Ok(_) => crate::note!("Deleted relay photo {photo_id}."),
        Err(e) => crate::note!("Warning: could not delete relay photo {photo_id}: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Videos
// ---------------------------------------------------------------------------

pub struct VideoPost<'a> {
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub publish: Publish,
}

/// `POST /{page-id}/videos` on the video host.
pub async fn upload_video(
    page: &Page,
    source: &MediaSource,
    post: VideoPost<'_>,
) -> Result<Value, String> {
    source.check_size(MAX_VIDEO_BYTES, "a single-request Facebook video upload")?;

    let mut form = post.publish.params();
    if let Some(t) = post.title {
        form.push(("title".to_string(), t.to_string()));
    }
    if let Some(d) = post.description {
        form.push(("description".to_string(), d.to_string()));
    }

    let path = format!("/{}/videos", page.id);

    match source {
        MediaSource::Url(url) => {
            form.push(("file_url".to_string(), url.clone()));
            api::post_to(api::VIDEO_HOST, &page.access_token, &path, &form).await
        }
        MediaSource::File(_) => {
            crate::note!(
                "Uploading {} ({})... this can take a while.",
                source.file_name(),
                human_size(source.size())
            );
            api::post_multipart(
                api::VIDEO_HOST,
                &page.access_token,
                &path,
                &form,
                "source",
                &source.file_name(),
                source.read_bytes()?,
            )
            .await
        }
    }
}

/// `{page-id}_{post-id}` → a permalink users can open.
pub fn permalink(post_id: &str) -> String {
    format!("https://www.facebook.com/{post_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_785_715_200; // 2026-08-03T00:00:00Z, in seconds

    #[test]
    fn publish_defaults_to_published() {
        let p = Publish::default().params();
        assert_eq!(p, vec![("published".to_string(), "true".to_string())]);
    }

    #[test]
    fn draft_marks_the_object_unpublished() {
        let p = Publish {
            schedule: None,
            draft: true,
        }
        .params();
        assert_eq!(p, vec![("published".to_string(), "false".to_string())]);
    }

    #[test]
    fn scheduling_implies_unpublished() {
        let p = Publish {
            schedule: Some("1785800000".to_string()),
            draft: false,
        }
        .params();
        assert!(p.contains(&("published".to_string(), "false".to_string())));
        assert!(p.contains(&(
            "scheduled_publish_time".to_string(),
            "1785800000".to_string()
        )));
    }

    #[test]
    fn schedule_accepts_a_timestamp_inside_the_window() {
        let ts = (NOW + 3600).to_string();
        assert_eq!(normalize_schedule(&ts, NOW).unwrap(), ts);
    }

    #[test]
    fn schedule_rejects_the_ten_minute_floor() {
        let err = normalize_schedule(&(NOW + 60).to_string(), NOW).unwrap_err();
        assert!(err.contains("10 minutes"));
    }

    #[test]
    fn schedule_rejects_beyond_seventy_five_days() {
        let err = normalize_schedule(&(NOW + 80 * 86_400).to_string(), NOW).unwrap_err();
        assert!(err.contains("75 days"));
    }

    #[test]
    fn schedule_passes_iso_strings_through_to_meta() {
        assert_eq!(
            normalize_schedule("2026-09-01T10:00:00+0200", NOW).unwrap(),
            "2026-09-01T10:00:00+0200"
        );
    }

    #[test]
    fn schedule_rejects_empty_input() {
        assert!(normalize_schedule("   ", NOW).is_err());
    }
}
