//! The publishing primitives: text posts, image posts, article shares and
//! reshares.
//!
//! `commands` orchestrates and prints; `broadcast` reuses the very same
//! functions, so the fan-out command can never drift from the single-target
//! one.

use super::api;
use crate::core::media::{human_size, MediaSource};
use serde_json::Value;

/// LinkedIn's commentary ceiling, counted in characters.
pub const MAX_COMMENTARY_CHARS: usize = 3000;
/// LinkedIn's article title ceiling.
pub const MAX_TITLE_CHARS: usize = 400;
/// Largest image LinkedIn accepts on the images upload endpoint.
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Escape reserved characters for LinkedIn's "little Text Format".
///
/// See <https://learn.microsoft.com/en-us/linkedin/marketing/community-management/shares/little-text-format>.
/// Without escaping, characters like `(` and `)` are read as mention syntax and
/// silently truncate the post.
pub fn escape_commentary(text: &str) -> String {
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

/// Reject commentary LinkedIn would refuse, counting **characters** — a post
/// full of accents or emoji is well under the limit even when its byte length
/// is not.
pub fn check_commentary(text: &str) -> Result<(), String> {
    let len = text.chars().count();
    if len > MAX_COMMENTARY_CHARS {
        return Err(format!(
            "Commentary is {len} characters — LinkedIn allows at most {MAX_COMMENTARY_CHARS}."
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// A plain text post. Returns the created post URN.
pub async fn text_post(alias: &str, text: &str, visibility: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err("Text must not be empty.".to_string());
    }
    check_commentary(text)?;

    let urn = super::store::resolve_urn(alias)?;
    let body = api::build_base_post(&urn, &escape_commentary(text), visibility);
    api::post(alias, "/rest/posts", &body).await
}

// ---------------------------------------------------------------------------
// Reshare
// ---------------------------------------------------------------------------

/// Reshare an existing post, optionally with commentary above it.
pub async fn reshare(
    alias: &str,
    post_id: &str,
    commentary: Option<&str>,
    visibility: &str,
) -> Result<String, String> {
    let commentary = commentary.unwrap_or("");
    check_commentary(commentary)?;

    let urn = super::store::resolve_urn(alias)?;
    let mut body = api::build_base_post(&urn, &escape_commentary(commentary), visibility);
    body["reshareContext"] = serde_json::json!({ "parent": post_id });
    api::post(alias, "/rest/posts", &body).await
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/// Upload an image and return its URN.
///
/// LinkedIn only ever takes bytes, so a URL is downloaded first — unlike Meta,
/// which fetches remote media itself.
pub async fn upload_image(
    alias: &str,
    owner_urn: &str,
    source: &MediaSource,
) -> Result<String, String> {
    let bytes = source.fetch_bytes().await?;

    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(format!(
            "{} is {} — LinkedIn allows at most 10 MB.",
            source.as_str(),
            human_size(Some(bytes.len() as u64))
        ));
    }

    crate::note!(
        "Uploading image ({})...",
        human_size(Some(bytes.len() as u64))
    );
    let (upload_url, image_urn) = api::initialize_image_upload(alias, owner_urn).await?;
    api::upload_image_binary(alias, &upload_url, bytes).await?;
    crate::note!("Image uploaded: {image_urn}");

    Ok(image_urn)
}

/// A post whose body is a full-size image, with the text above it. Returns
/// `(post URN, image URN)`.
pub async fn image_post(
    alias: &str,
    text: &str,
    image: &MediaSource,
    title: &str,
    visibility: &str,
) -> Result<(String, String), String> {
    check_commentary(text)?;

    let urn = super::store::resolve_urn(alias)?;
    let image_urn = upload_image(alias, &urn, image).await?;

    let mut body = api::build_base_post(&urn, &escape_commentary(text), visibility);
    body["content"] = serde_json::json!({
        "media": { "id": image_urn, "title": title }
    });

    let post_id = api::post(alias, "/rest/posts", &body).await?;
    Ok((post_id, image_urn))
}

// ---------------------------------------------------------------------------
// Articles
// ---------------------------------------------------------------------------

pub struct Article<'a> {
    pub url: &'a str,
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub commentary: &'a str,
    pub visibility: &'a str,
    /// Optional thumbnail for the link preview card.
    pub image: Option<&'a MediaSource>,
}

/// A link-preview card. Returns `(post URN, thumbnail URN if one was uploaded)`.
pub async fn article_post(
    alias: &str,
    article: Article<'_>,
) -> Result<(String, Option<String>), String> {
    if article.title.is_empty() || article.title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!(
            "Title must be between 1 and {MAX_TITLE_CHARS} characters."
        ));
    }
    check_commentary(article.commentary)?;

    let urn = super::store::resolve_urn(alias)?;

    let image_urn = match article.image {
        Some(source) => Some(upload_image(alias, &urn, source).await?),
        None => None,
    };

    let mut body = api::build_base_post(
        &urn,
        &escape_commentary(article.commentary),
        article.visibility,
    );
    body["content"] = serde_json::json!({ "article": article_content(&article, &image_urn) });

    let post_id = api::post(alias, "/rest/posts", &body).await?;
    Ok((post_id, image_urn))
}

fn article_content(article: &Article<'_>, image_urn: &Option<String>) -> Value {
    let mut content = serde_json::json!({
        "source": article.url,
        "title": article.title,
    });
    if let Some(desc) = article.description {
        content["description"] = serde_json::json!(desc);
    }
    if let Some(urn) = image_urn {
        content["thumbnail"] = serde_json::json!(urn);
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_syntax_is_escaped() {
        assert_eq!(escape_commentary("a (b) c"), "a \\(b\\) c");
        assert_eq!(escape_commentary("#tag @me"), "\\#tag \\@me");
        assert_eq!(escape_commentary("plain text"), "plain text");
    }

    #[test]
    fn escaping_leaves_accents_and_emoji_alone() {
        assert_eq!(escape_commentary("déjà vu 🎉"), "déjà vu 🎉");
    }

    #[test]
    fn commentary_length_counts_characters_not_bytes() {
        // 3000 accented characters are far more than 3000 bytes.
        assert!(check_commentary(&"é".repeat(MAX_COMMENTARY_CHARS)).is_ok());
        let err = check_commentary(&"a".repeat(MAX_COMMENTARY_CHARS + 1)).unwrap_err();
        assert!(err.contains("3000"));
    }

    #[test]
    fn an_article_carries_its_optional_fields_only_when_set() {
        let source = MediaSource::Url("https://x/a.jpg".to_string());
        let bare = Article {
            url: "https://z29k.fr",
            title: "T",
            description: None,
            commentary: "",
            visibility: "PUBLIC",
            image: None,
        };
        let content = article_content(&bare, &None);
        assert_eq!(content["source"], "https://z29k.fr");
        assert!(content.get("description").is_none());
        assert!(content.get("thumbnail").is_none());

        let full = Article {
            description: Some("d"),
            image: Some(&source),
            ..bare
        };
        let content = article_content(&full, &Some("urn:li:image:1".to_string()));
        assert_eq!(content["description"], "d");
        assert_eq!(content["thumbnail"], "urn:li:image:1");
    }
}
