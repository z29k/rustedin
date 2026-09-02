//! Thin Graph API layer over [`crate::core::http`].
//!
//! Every Facebook and Instagram call funnels through here so that versioning,
//! `appsecret_proof`, the Graph-specific retry rule and error formatting are
//! defined exactly once.

use crate::core::http::{self, HttpError, Retry};
use hmac::{Hmac, Mac};
use reqwest::StatusCode;
use serde_json::Value;
use sha2::Sha256;
use std::sync::OnceLock;

/// Graph API version used when `--api-version` is not passed.
/// See <https://developers.facebook.com/docs/graph-api/changelog/versions/>.
pub const DEFAULT_API_VERSION: &str = "v25.0";

pub const GRAPH_HOST: &str = "https://graph.facebook.com";
/// Uploading video bytes must go through the video host, not `graph.facebook.com`.
pub const VIDEO_HOST: &str = "https://graph-video.facebook.com";
/// Instagram resumable upload host (used for local video files).
pub const RUPLOAD_HOST: &str = "https://rupload.facebook.com";

static API_VERSION: OnceLock<String> = OnceLock::new();

/// Initialize the Graph API version. Call once at startup.
pub fn init_version(override_version: Option<&str>) {
    let v = override_version.unwrap_or(DEFAULT_API_VERSION);
    // Accept both "25.0" and "v25.0".
    let v = if v.starts_with('v') {
        v.to_string()
    } else {
        format!("v{v}")
    };
    API_VERSION.set(v).ok();
}

pub fn version() -> String {
    API_VERSION
        .get()
        .cloned()
        .unwrap_or_else(|| DEFAULT_API_VERSION.to_string())
}

/// HMAC-SHA256 of the access token, keyed with the app secret, hex encoded.
///
/// Meta rejects calls without it once "Require App Secret" is enabled on the
/// app, and always accepts it otherwise — so rustedin sends it unconditionally.
pub fn appsecret_proof(access_token: &str, app_secret: &str) -> String {
    let mut mac = <Hmac<Sha256>>::new_from_slice(app_secret.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(access_token.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// `access_token` (+ `appsecret_proof` when the app secret is known), ready to
/// be merged into a query string or a form body.
pub fn auth_params(access_token: &str) -> Vec<(String, String)> {
    let mut params = vec![("access_token".to_string(), access_token.to_string())];
    if let Some(secret) = super::store::app_secret() {
        params.push((
            "appsecret_proof".to_string(),
            appsecret_proof(access_token, &secret),
        ));
    }
    params
}

/// Build a versioned Graph URL: `graph_url(GRAPH_HOST, "/me/accounts")`.
pub fn graph_url(host: &str, path: &str) -> String {
    let path = path.strip_prefix('/').unwrap_or(path);
    format!("{host}/{}/{path}", version())
}

/// Meta reports throttling as an HTTP 400 with a code in the body, so both
/// policies have to look inside it. Being throttled means the call was *not*
/// processed, which is exactly why repeating it is safe even for a publish.
fn graph_throttled(body: &str) -> bool {
    error_code(body)
        .map(|c| RATE_LIMIT_CODES.contains(&c))
        .unwrap_or(false)
}

/// For reads: rate limits, server errors and timeouts.
fn retry_read() -> Retry {
    Retry::reads_with(|status, body| http::read_retryable(status, body) || graph_throttled(body))
}

/// For publishes: rate limits only. A 5xx or a timeout says nothing about
/// whether Meta already created the post.
fn retry_write() -> Retry {
    Retry::writes_with(|status, body| http::write_retryable(status, body) || graph_throttled(body))
}

/// `GET` a Graph edge and return the parsed body.
pub async fn get(
    access_token: &str,
    path: &str,
    query: &[(String, String)],
) -> Result<Value, String> {
    let url = graph_url(GRAPH_HOST, path);
    let mut params = auth_params(access_token);
    params.extend_from_slice(query);
    let context = format!("GET {path}");

    http::send(
        || http::client().get(&url).query(&params),
        &context,
        &retry_read(),
    )
    .await
    .map_err(render)?
    .json(&context)
}

/// `GET` an OAuth endpoint with **exactly** the given parameters.
///
/// The `/oauth/*` and `/debug_token` endpoints carry their own credentials
/// (`client_secret`, or an `app_id|app_secret` token), so the usual
/// `access_token` + `appsecret_proof` pair must not be appended.
pub async fn oauth_get(path: &str, params: &[(String, String)]) -> Result<Value, String> {
    let url = graph_url(GRAPH_HOST, path);
    let context = format!("GET {path}");

    http::send(
        || http::client().get(&url).query(params),
        &context,
        // `/oauth/access_token` burns a single-use authorization code, so this
        // one is treated as a write even though it is a GET.
        &retry_write(),
    )
    .await
    .map_err(render)?
    .json(&context)
}

/// `GET` an absolute URL (used to follow `paging.next` cursors, which already
/// carry their own token and cursor parameters).
pub async fn get_absolute(url: &str) -> Result<Value, String> {
    let context = "GET (paged)";
    http::send(|| http::client().get(url), context, &retry_read())
        .await
        .map_err(render)?
        .json(context)
}

/// `POST` a form-encoded body to a Graph edge.
pub async fn post(
    access_token: &str,
    path: &str,
    form: &[(String, String)],
) -> Result<Value, String> {
    post_to(GRAPH_HOST, access_token, path, form).await
}

/// Same as [`post`] but lets the caller pick the host (e.g. [`VIDEO_HOST`]).
pub async fn post_to(
    host: &str,
    access_token: &str,
    path: &str,
    form: &[(String, String)],
) -> Result<Value, String> {
    let url = graph_url(host, path);
    let mut params = auth_params(access_token);
    params.extend_from_slice(form);
    let context = format!("POST {path}");

    log_request("POST", &url, &params);

    http::send(
        || http::client().post(&url).form(&params),
        &context,
        &retry_write(),
    )
    .await
    .map_err(render)?
    .json(&context)
}

/// `DELETE` a Graph node. Errors are returned but callers may choose to ignore
/// them (e.g. best-effort cleanup of relay photos).
pub async fn delete(access_token: &str, path: &str) -> Result<Value, String> {
    let url = graph_url(GRAPH_HOST, path);
    let params = auth_params(access_token);
    let context = format!("DELETE {path}");

    http::send(
        || http::client().delete(&url).query(&params),
        &context,
        // Deleting twice is harmless: the second call just 404s.
        &retry_read(),
    )
    .await
    .map_err(render)?
    .json(&context)
}

/// `POST` a `multipart/form-data` body (binary upload of a local file).
///
/// Multipart bodies cannot be rebuilt cheaply between attempts, so this call is
/// **not** retried — a failed upload surfaces immediately.
pub async fn post_multipart(
    host: &str,
    access_token: &str,
    path: &str,
    fields: &[(String, String)],
    file_field: &str,
    file_name: &str,
    file_bytes: Vec<u8>,
) -> Result<Value, String> {
    let url = graph_url(host, path);
    let context = format!("POST {path} (multipart)");

    let mut form = reqwest::multipart::Form::new();
    for (k, v) in auth_params(access_token).iter().chain(fields.iter()) {
        form = form.text(k.clone(), v.clone());
    }
    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name.to_string())
        .mime_str("application/octet-stream")
        .map_err(|e| format!("{context}: invalid MIME type: {e}"))?;
    form = form.part(file_field.to_string(), part);

    http::send_once(http::client().post(&url).multipart(form), &context)
        .await
        .map_err(render)?
        .json(&context)
}

/// Raw `POST` of a byte body with custom headers — used by Instagram's
/// resumable upload protocol on `rupload.facebook.com`.
pub async fn post_bytes(
    url: &str,
    headers: &[(&str, String)],
    body: Vec<u8>,
    context: &str,
) -> Result<Value, String> {
    let mut req = http::client().post(url).body(body);
    for (k, v) in headers {
        req = req.header(*k, v);
    }

    http::send_once(req, context)
        .await
        .map_err(render)?
        .json(context)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Turn a transport or status failure into the Graph-flavoured message.
fn render(e: HttpError) -> String {
    match e {
        HttpError::Transport { context, source } => format!("{context}: request failed: {source}"),
        HttpError::Status {
            context,
            status,
            body,
        } => graph_error(&context, status, &body),
    }
}

/// Graph API error codes that mean "throttled, try again later".
const RATE_LIMIT_CODES: [i64; 5] = [4, 17, 32, 613, 80004];

fn error_code(body: &str) -> Option<i64> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["code"].as_i64())
}

/// Turn a Graph API error envelope into a single actionable line (plus a hint
/// when the code is one we recognise).
pub fn graph_error(context: &str, status: StatusCode, body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return format!("{context}: Graph API {status} — {body}");
    };
    let Some(err) = v.get("error") else {
        return format!("{context}: Graph API {status} — {body}");
    };

    let message = err["message"].as_str().unwrap_or("unknown error");
    let code = err["code"].as_i64();
    let subcode = err["error_subcode"].as_i64();
    let user_title = err["error_user_title"].as_str();
    let user_msg = err["error_user_msg"].as_str();
    let trace = err["fbtrace_id"].as_str();

    let mut out = format!("{context}: Graph API {status} — {message}");

    if let Some(c) = code {
        out.push_str(&format!(" [code {c}"));
        if let Some(sc) = subcode {
            out.push_str(&format!("/{sc}"));
        }
        out.push(']');
    }
    if let Some(t) = trace {
        out.push_str(&format!(" (fbtrace_id {t})"));
    }
    if let Some(t) = user_title {
        out.push_str(&format!("\n  {t}"));
    }
    if let Some(m) = user_msg {
        out.push_str(&format!("\n  {m}"));
    }
    if let Some(hint) = hint_for(code, subcode) {
        out.push_str(&format!("\n  Hint: {hint}"));
    }
    out
}

/// Human-readable next step for the error codes users actually hit.
fn hint_for(code: Option<i64>, subcode: Option<i64>) -> Option<&'static str> {
    match (code, subcode) {
        (Some(190), Some(460)) => Some(
            "the user changed their Facebook password — re-run \
             `rustedin meta auth --account=<alias>`.",
        ),
        (Some(190), _) => Some(
            "the access token is invalid or expired — re-run \
             `rustedin meta auth --account=<alias>`.",
        ),
        (Some(200) | Some(10) | Some(3), _) => Some(
            "the app is missing a permission for this action. Check the granted scopes with \
             `rustedin meta status --check`, and remember that pages_manage_posts / \
             instagram_content_publish require App Review before the app goes Live.",
        ),
        (Some(c), _) if RATE_LIMIT_CODES.contains(&c) => Some(
            "rate limited by Meta. rustedin already retried with backoff; wait a few minutes \
             before trying again.",
        ),
        (Some(100), _) => {
            Some("invalid parameter — check the media URL is publicly reachable and the IDs exist.")
        }
        (Some(368), _) => Some("the Page or account is temporarily blocked for policy violations."),
        (Some(9004), _) => Some(
            "Instagram could not fetch the media URL. It must be public, HTTPS, and served \
             without redirects or authentication.",
        ),
        (Some(9007), _) => Some(
            "the media container is not ready to publish yet, or it failed processing — check \
             the container status.",
        ),
        (Some(25), _) => Some(
            "this Instagram account is not a Professional (Business/Creator) account, or it is \
             not linked to the Facebook Page.",
        ),
        _ => None,
    }
}

fn log_request(method: &str, url: &str, params: &[(String, String)]) {
    // Never echo credentials: the token and its proof are stripped from the log.
    let redacted: Vec<String> = params
        .iter()
        .filter(|(k, _)| k != "access_token" && k != "appsecret_proof")
        .map(|(k, v)| {
            if v.chars().count() > 120 {
                let head: String = v.chars().take(120).collect();
                format!("{k}={head}…")
            } else {
                format!("{k}={v}")
            }
        })
        .collect();
    crate::note!("{method} {url}");
    if !redacted.is_empty() {
        crate::note!("  {}", redacted.join(" "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appsecret_proof_matches_known_hmac() {
        // Reference vector: HMAC-SHA256("key", "The quick brown fox jumps over the lazy dog")
        assert_eq!(
            appsecret_proof("The quick brown fox jumps over the lazy dog", "key"),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    #[test]
    fn graph_url_joins_version_once() {
        init_version(Some("v25.0"));
        assert_eq!(
            graph_url(GRAPH_HOST, "/me/accounts"),
            "https://graph.facebook.com/v25.0/me/accounts"
        );
        assert_eq!(
            graph_url(GRAPH_HOST, "me/accounts"),
            "https://graph.facebook.com/v25.0/me/accounts"
        );
    }

    #[test]
    fn throttling_in_a_400_body_is_retryable_on_both_policies() {
        let body = r#"{"error":{"message":"too many calls","code":4}}"#;
        assert!((retry_read().retryable)(StatusCode::BAD_REQUEST, body));
        assert!((retry_write().retryable)(StatusCode::BAD_REQUEST, body));
    }

    #[test]
    fn ordinary_400_is_not_retryable() {
        let body = r#"{"error":{"message":"bad param","code":100}}"#;
        assert!(!(retry_read().retryable)(StatusCode::BAD_REQUEST, body));
        assert!(!(retry_write().retryable)(StatusCode::BAD_REQUEST, body));
    }

    /// A publish that may already have gone through is never repeated.
    #[test]
    fn a_publish_is_not_repeated_on_a_server_error() {
        assert!((retry_read().retryable)(StatusCode::BAD_GATEWAY, ""));
        assert!(!(retry_write().retryable)(StatusCode::BAD_GATEWAY, ""));
        assert!(!retry_write().retry_timeouts);
    }

    #[test]
    fn graph_error_extracts_code_and_hint() {
        let body =
            r#"{"error":{"message":"Invalid OAuth access token.","code":190,"fbtrace_id":"Abc"}}"#;
        let msg = graph_error("POST /feed", StatusCode::BAD_REQUEST, body);
        assert!(msg.contains("Invalid OAuth access token."));
        assert!(msg.contains("[code 190]"));
        assert!(msg.contains("fbtrace_id Abc"));
        assert!(msg.contains("rustedin meta auth"));
    }

    #[test]
    fn graph_error_falls_back_to_raw_body() {
        let msg = graph_error("GET /me", StatusCode::BAD_GATEWAY, "<html>oops</html>");
        assert!(msg.contains("<html>oops</html>"));
    }

    #[test]
    fn a_transport_failure_keeps_its_context() {
        let msg = render(HttpError::Transport {
            context: "GET /me".to_string(),
            source: "dns error".to_string(),
        });
        assert!(msg.contains("GET /me"));
        assert!(msg.contains("dns error"));
    }
}
