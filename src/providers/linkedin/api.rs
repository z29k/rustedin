//! Thin LinkedIn REST layer over [`crate::core::http`].
//!
//! LinkedIn versions its API through a header rather than the path, and answers
//! errors with a `serviceErrorCode` envelope that looks nothing like Meta's —
//! both live here so the rest of the provider never sees a raw response.

use super::store;
use crate::core::http::{self, HttpError, Retry};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::StatusCode;
use serde_json::Value;

/// `LinkedIn-Version` header. LinkedIn deprecates versions on a rolling
/// one-year window; bump this deliberately.
const LINKEDIN_VERSION: &str = "202603";
const API_BASE: &str = "https://api.linkedin.com";

async fn api_headers(alias: &str) -> Result<HeaderMap, String> {
    let token = store::get_valid_token(alias).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| format!("Stored token for \"{alias}\" is not a valid header value"))?,
    );
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    headers.insert(
        "X-Restli-Protocol-Version",
        HeaderValue::from_static("2.0.0"),
    );
    headers.insert(
        "LinkedIn-Version",
        HeaderValue::from_static(LINKEDIN_VERSION),
    );
    Ok(headers)
}

/// `POST` a JSON body and return the created object's URN.
///
/// LinkedIn returns it in the `x-restli-id` response header, not in the body.
pub async fn post(alias: &str, path: &str, body: &Value) -> Result<String, String> {
    let headers = api_headers(alias).await?;
    let url = format!("{API_BASE}{path}");
    let context = format!("POST {path}");

    crate::note!("POST {url}");
    crate::note!(
        "Body:\n{}",
        serde_json::to_string_pretty(body).unwrap_or_default()
    );

    let res = http::send(
        || {
            http::client()
                .post(&url)
                .headers(headers.clone())
                .json(body)
        },
        &context,
        // A publish is never repeated on a 5xx or a timeout: LinkedIn may have
        // created the post before the connection broke.
        &Retry::writes(),
    )
    .await
    .map_err(|e| render(alias, e))?;

    Ok(res
        .header("x-restli-id")
        .or_else(|| res.header("X-RestLi-Id"))
        .unwrap_or("unknown")
        .to_string())
}

pub async fn get(alias: &str, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let headers = api_headers(alias).await?;
    let url = format!("{API_BASE}{path}");
    let context = format!("GET {path}");

    http::send(
        || {
            http::client()
                .get(&url)
                .headers(headers.clone())
                .query(query)
        },
        &context,
        &Retry::reads(),
    )
    .await
    .map_err(|e| render(alias, e))?
    .json(&context)
}

/// LinkedIn wants URNs percent-encoded when they appear inside a path.
pub fn encode_urn(urn: &str) -> String {
    urn.replace(':', "%3A")
}

/// The fields every `/rest/posts` body carries, whatever the content type.
pub fn build_base_post(author_urn: &str, commentary: &str, visibility: &str) -> Value {
    serde_json::json!({
        "author": author_urn,
        "commentary": commentary,
        "visibility": visibility,
        "distribution": {
            "feedDistribution": "MAIN_FEED",
            "targetEntities": [],
            "thirdPartyDistributionChannels": []
        },
        "lifecycleState": "PUBLISHED",
        "isReshareDisabledByAuthor": false
    })
}

// ---------------------------------------------------------------------------
// Image upload
// ---------------------------------------------------------------------------

/// `POST /rest/images?action=initializeUpload` → (upload URL, image URN).
pub async fn initialize_image_upload(
    alias: &str,
    owner_urn: &str,
) -> Result<(String, String), String> {
    let headers = api_headers(alias).await?;
    let url = format!("{API_BASE}/rest/images?action=initializeUpload");
    let context = "POST /rest/images?action=initializeUpload";

    let body = serde_json::json!({
        "initializeUploadRequest": { "owner": owner_urn }
    });

    let data = http::send(
        || {
            http::client()
                .post(&url)
                .headers(headers.clone())
                .json(&body)
        },
        context,
        &Retry::writes(),
    )
    .await
    .map_err(|e| render(alias, e))?
    .json(context)?;

    let upload_url = data["value"]["uploadUrl"]
        .as_str()
        .ok_or("Missing uploadUrl in image init response")?
        .to_string();
    let image_urn = data["value"]["image"]
        .as_str()
        .ok_or("Missing image URN in image init response")?
        .to_string();

    Ok((upload_url, image_urn))
}

/// `PUT` the raw bytes to the upload URL returned by
/// [`initialize_image_upload`]. Not retried: the body is moved into the request.
pub async fn upload_image_binary(
    alias: &str,
    upload_url: &str,
    image_bytes: Vec<u8>,
) -> Result<(), String> {
    let token = store::get_valid_token(alias).await?;

    http::send_once(
        http::client()
            .put(upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/octet-stream")
            .body(image_bytes),
        "PUT (image upload)",
    )
    .await
    .map_err(|e| render(alias, e))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

fn render(alias: &str, e: HttpError) -> String {
    match e {
        HttpError::Transport { context, source } => format!("{context}: request failed: {source}"),
        HttpError::Status {
            context,
            status,
            body,
        } => linkedin_error(alias, &context, status, &body),
    }
}

/// Turn a LinkedIn error envelope into one actionable line.
pub fn linkedin_error(alias: &str, context: &str, status: StatusCode, body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| v["message"].as_str())
        .unwrap_or(body);
    let code = parsed.as_ref().and_then(|v| v["serviceErrorCode"].as_i64());

    let mut out = format!("{context}: LinkedIn API {status} for \"{alias}\" — {message}");
    if let Some(c) = code {
        out.push_str(&format!(" [serviceErrorCode {c}]"));
    }
    if let Some(hint) = hint_for(status, code) {
        out.push_str(&format!("\n  Hint: {hint}"));
    }
    out
}

fn hint_for(status: StatusCode, code: Option<i64>) -> Option<&'static str> {
    match (status.as_u16(), code) {
        (401, _) => Some(
            "the access token is invalid or revoked — re-run \
             `rustedin linkedin auth --account=<alias>`.",
        ),
        (403, _) => Some(
            "the token lacks the scope this call needs. A personal account needs \
             w_member_social; a company page needs w_organization_social, and the member who \
             authorized it must still be an admin of the page.",
        ),
        (426, _) => Some(
            "LinkedIn rejected the API version. Bump LINKEDIN_VERSION in \
             src/providers/linkedin/api.rs — versions are deprecated after about a year.",
        ),
        (429, _) => Some(
            "rate limited by LinkedIn. rustedin already retried with backoff; daily quotas \
             reset at midnight UTC.",
        ),
        (422, _) => Some(
            "LinkedIn refused the post body — most often a malformed URN, or commentary that \
             failed Little Text Format escaping.",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urns_are_percent_encoded_for_paths() {
        assert_eq!(encode_urn("urn:li:share:7123"), "urn%3Ali%3Ashare%3A7123");
        assert_eq!(encode_urn("no-colons-here"), "no-colons-here");
    }

    #[test]
    fn the_base_post_carries_the_main_feed_distribution() {
        let body = build_base_post("urn:li:person:x", "hello", "PUBLIC");
        assert_eq!(body["author"], "urn:li:person:x");
        assert_eq!(body["commentary"], "hello");
        assert_eq!(body["visibility"], "PUBLIC");
        assert_eq!(body["distribution"]["feedDistribution"], "MAIN_FEED");
        assert_eq!(body["lifecycleState"], "PUBLISHED");
    }

    #[test]
    fn an_error_envelope_is_reduced_to_one_line() {
        let body =
            r#"{"serviceErrorCode":100,"message":"Field Value validation failed","status":422}"#;
        let msg = linkedin_error(
            "quentin",
            "POST /rest/posts",
            StatusCode::UNPROCESSABLE_ENTITY,
            body,
        );
        assert!(msg.contains("Field Value validation failed"));
        assert!(msg.contains("[serviceErrorCode 100]"));
        assert!(msg.contains("quentin"));
        assert!(msg.contains("Little Text Format"));
    }

    #[test]
    fn an_unparseable_body_is_shown_raw() {
        let msg = linkedin_error("z29k", "GET /rest/posts", StatusCode::BAD_GATEWAY, "<html>");
        assert!(msg.contains("<html>"));
    }

    #[test]
    fn a_401_points_at_reauthentication() {
        let msg = linkedin_error("z29k", "POST /rest/posts", StatusCode::UNAUTHORIZED, "{}");
        assert!(msg.contains("rustedin linkedin auth"));
    }
}
