//! One HTTP client for every platform.
//!
//! Providers differ in how they authenticate and how they render errors, but
//! not in what a resilient request looks like: a pooled connection, a timeout,
//! and a bounded retry on transient failures. That part lives here, and
//! [`send`] hands the raw outcome back so each provider can turn it into its
//! own error message — a Graph API envelope and a LinkedIn one read nothing
//! alike, and flattening both into a generic string would lose the actionable
//! half.

use reqwest::header::HeaderMap;
use reqwest::{RequestBuilder, StatusCode};
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// One shared, pooled client. Unlike a per-call `Client::new()`, it keeps
/// connections alive across the many round-trips a publish can need (Instagram:
/// container → poll → publish) and enforces timeouts everywhere.
pub fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            // Generous: video uploads stream large bodies through this client.
            .timeout(Duration::from_secs(600))
            .user_agent(concat!("rustedin/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("failed to build HTTP client")
    })
}

/// A completed, successful response.
///
/// The payload is kept as **bytes**, not as a `String`: this client also
/// downloads images, and decoding those as UTF-8 would silently corrupt them.
/// The headers are kept because some APIs return the created object's id there
/// (LinkedIn's `x-restli-id`) rather than in the body.
pub struct Response {
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl Response {
    /// The body as text, for the overwhelming majority of calls that answer
    /// with JSON.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    pub fn json(&self, context: &str) -> Result<Value, String> {
        parse_json(&self.text(), context)
    }

    /// First matching header value, as a string.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

/// A request that did not succeed.
///
/// Providers match on this to build their own message; [`Display`] is the
/// fallback rendering for anything that does not bother.
#[derive(Debug)]
pub enum HttpError {
    /// The request never produced a response (DNS, TLS, timeout, refused).
    Transport { context: String, source: String },
    /// The server answered with a non-2xx status.
    Status {
        context: String,
        status: StatusCode,
        body: String,
    },
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Transport { context, source } => {
                write!(f, "{context}: request failed: {source}")
            }
            HttpError::Status {
                context,
                status,
                body,
            } => write!(f, "{context}: HTTP {status} — {body}"),
        }
    }
}

impl From<HttpError> for String {
    fn from(e: HttpError) -> String {
        e.to_string()
    }
}

/// How hard to try before giving up.
///
/// The distinction that matters is **read vs. write**. Repeating a read costs
/// nothing; repeating a publish can post twice. A `502` or a timeout says
/// nothing about whether the server already applied the request, so writes only
/// ever repeat on failures that provably did *not* reach the platform: a
/// refused connection, or an explicit "throttled, I did not process this".
pub struct Retry {
    /// Total attempts, including the first.
    pub max_attempts: u32,
    /// Verdict on a failed response. Providers widen it — Meta, for one,
    /// reports throttling as an HTTP 400 with a code in the body.
    pub retryable: fn(StatusCode, &str) -> bool,
    /// Also repeat a request that timed out. A connect failure is always safe
    /// to repeat (nothing was sent); a timeout is not, because the server may
    /// have applied the request anyway.
    pub retry_timeouts: bool,
}

/// Rate limited, or the server broke before answering.
///
/// Exported so a provider widening the rule can fall back to it.
pub fn read_retryable(status: StatusCode, _: &str) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

/// Rate limited only: the platform is telling us it did *not* process the call.
pub fn write_retryable(status: StatusCode, _: &str) -> bool {
    status.as_u16() == 429
}

/// One attempt plus three retries, backing off 2s → 6s → 18s.
const DEFAULT_ATTEMPTS: u32 = 4;

impl Retry {
    /// For reads and other repeatable calls.
    pub fn reads() -> Self {
        Retry {
            max_attempts: DEFAULT_ATTEMPTS,
            retryable: read_retryable,
            retry_timeouts: true,
        }
    }

    /// For calls that create something: same backoff, but only on failures that
    /// cannot have published anything.
    pub fn writes() -> Self {
        Retry {
            max_attempts: DEFAULT_ATTEMPTS,
            retryable: write_retryable,
            retry_timeouts: false,
        }
    }

    /// [`Self::reads`] with a provider's own rule replacing the built-in one.
    pub fn reads_with(retryable: fn(StatusCode, &str) -> bool) -> Self {
        Retry {
            retryable,
            ..Self::reads()
        }
    }

    /// [`Self::writes`] with a provider's own rule replacing the built-in one.
    pub fn writes_with(retryable: fn(StatusCode, &str) -> bool) -> Self {
        Retry {
            retryable,
            ..Self::writes()
        }
    }
}

/// Send a request exactly once, consuming the builder.
///
/// For bodies that cannot be rebuilt between attempts — a multipart stream, a
/// `Vec<u8>` moved into the request — and for calls whose repetition would
/// publish twice.
pub async fn send_once(req: RequestBuilder, context: &str) -> Result<Response, HttpError> {
    let res = req.send().await.map_err(|e| HttpError::Transport {
        context: context.to_string(),
        source: e.to_string(),
    })?;

    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.bytes().await.map(|b| b.to_vec()).unwrap_or_default();

    if status.is_success() {
        return Ok(Response { headers, bytes });
    }
    Err(HttpError::Status {
        context: context.to_string(),
        status,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// Send a request, retrying transient failures with exponential backoff.
///
/// `build` is called once per attempt because [`RequestBuilder::send`] consumes
/// the builder.
pub async fn send<F>(build: F, context: &str, retry: &Retry) -> Result<Response, HttpError>
where
    F: Fn() -> RequestBuilder,
{
    let mut delay = 2u64;

    let attempts = retry.max_attempts.max(1);
    for attempt in 1..=attempts {
        let last = attempt == attempts;

        match build().send().await {
            Ok(res) => {
                let status = res.status();
                let headers = res.headers().clone();
                let bytes = res.bytes().await.map(|b| b.to_vec()).unwrap_or_default();

                if status.is_success() {
                    return Ok(Response { headers, bytes });
                }
                // An error body is always text, so rendering it lossily is safe.
                let body = String::from_utf8_lossy(&bytes).into_owned();
                if !last && (retry.retryable)(status, &body) {
                    crate::note!(
                        "{context} → HTTP {status}; retrying in {delay}s ({attempt}/{})",
                        attempts - 1
                    );
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    delay *= 3;
                    continue;
                }
                return Err(HttpError::Status {
                    context: context.to_string(),
                    status,
                    body,
                });
            }
            Err(e) => {
                // A refused connection never reached the platform; a timeout
                // may have, so only repeat it when the caller allows it.
                if !last && (e.is_connect() || (retry.retry_timeouts && e.is_timeout())) {
                    crate::note!(
                        "{context} → {e}; retrying in {delay}s ({attempt}/{})",
                        attempts - 1
                    );
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    delay *= 3;
                    continue;
                }
                return Err(HttpError::Transport {
                    context: context.to_string(),
                    source: e.to_string(),
                });
            }
        }
    }

    unreachable!("the last attempt always returns")
}

/// Parse a response body, treating an empty one as `null` — some endpoints
/// answer a successful `DELETE` with nothing at all.
pub fn parse_json(body: &str, context: &str) -> Result<Value, String> {
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(body).map_err(|e| format!("{context}: invalid JSON response: {e}\n{body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph_style(status: StatusCode, body: &str) -> bool {
        write_retryable(status, body) || body.contains("\"code\":4")
    }

    #[test]
    fn reads_repeat_on_rate_limits_and_server_errors() {
        let r = Retry::reads();
        assert!((r.retryable)(StatusCode::TOO_MANY_REQUESTS, ""));
        assert!((r.retryable)(StatusCode::BAD_GATEWAY, ""));
        assert!(!(r.retryable)(StatusCode::BAD_REQUEST, ""));
        assert!(!(r.retryable)(StatusCode::UNAUTHORIZED, ""));
        assert!(r.retry_timeouts);
    }

    /// The rule that keeps a publish from happening twice: a 5xx or a timeout
    /// says nothing about whether the platform already accepted the post.
    #[test]
    fn writes_never_repeat_on_a_server_error_or_a_timeout() {
        let r = Retry::writes();
        assert!((r.retryable)(StatusCode::TOO_MANY_REQUESTS, ""));
        assert!(!(r.retryable)(StatusCode::BAD_GATEWAY, ""));
        assert!(!(r.retryable)(StatusCode::GATEWAY_TIMEOUT, ""));
        assert!(!r.retry_timeouts);
    }

    #[test]
    fn a_provider_can_widen_the_rule_without_losing_the_write_guarantee() {
        let r = Retry::writes_with(graph_style);
        assert!((r.retryable)(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"code":4}}"#
        ));
        assert!(!(r.retryable)(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"code":100}}"#
        ));
        assert!(!(r.retryable)(StatusCode::BAD_GATEWAY, ""));
        assert!(!r.retry_timeouts);
    }

    #[test]
    fn an_empty_body_parses_as_null() {
        assert_eq!(parse_json("  ", "DELETE /x").unwrap(), Value::Null);
    }

    #[test]
    fn a_malformed_body_names_the_call() {
        let err = parse_json("<html>", "GET /me").unwrap_err();
        assert!(err.contains("GET /me"));
        assert!(err.contains("<html>"));
    }
}
