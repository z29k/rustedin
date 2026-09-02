//! The loopback half of an OAuth 2.0 authorization-code flow.
//!
//! Every provider opens a browser at its own authorization URL and then waits
//! for the redirect to come back to `http://localhost:<port>/callback`. Only
//! the URL differs, so the listener, the CSRF check, the percent-decoding and
//! the two HTML pages the user actually sees live here.

use rand::Rng;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Default loopback port. Providers expose it as `--port` so a busy machine
/// still has a way out.
pub const DEFAULT_PORT: u16 = 8765;

/// How long to wait for the user to finish authorizing in the browser.
const TIMEOUT: Duration = Duration::from_secs(300);

/// 32 hex characters of CSRF state, echoed back by the provider.
pub fn random_state() -> String {
    let mut rng = rand::rng();
    (0..16)
        .map(|_| format!("{:02x}", rng.random::<u8>()))
        .collect()
}

/// What the callback listener needs to know.
pub struct Callback<'a> {
    pub port: u16,
    /// The `state` value sent in the authorization request.
    pub state: &'a str,
    /// Shown to the user on the confirmation page, e.g. `"LinkedIn"`.
    pub platform: &'a str,
    /// Shown to the user on the confirmation page.
    pub alias: &'a str,
}

/// Serve one request on the loopback port and return the authorization code.
pub async fn wait_for_code(cb: Callback<'_>) -> Result<String, String> {
    let listener = TcpListener::bind(("127.0.0.1", cb.port))
        .await
        .map_err(|e| {
            format!(
                "Failed to bind port {}: {e}\nPick another one with: --port=<PORT>",
                cb.port
            )
        })?;

    eprintln!(
        "Waiting for the {} callback on http://localhost:{} ...",
        cb.platform, cb.port
    );

    let wait = tokio::time::timeout(TIMEOUT, async {
        loop {
            let (mut stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("Accept failed: {e}"))?;

            let mut buf = vec![0u8; 8192];
            let n = stream
                .read(&mut buf)
                .await
                .map_err(|e| format!("Read failed: {e}"))?;
            let request = String::from_utf8_lossy(&buf[..n]);

            // Parse: GET /callback?code=...&state=... HTTP/1.1
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("");

            if !path.starts_with("/callback") {
                // A favicon or preflight request must not abort the flow.
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").await;
                continue;
            }

            let params = parse_query(path.split('?').nth(1).unwrap_or(""));
            let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());

            let returned_state = get("state").unwrap_or("");
            let code = get("code").unwrap_or("");

            if let Some(err) = get("error") {
                let detail = get("error_description").unwrap_or(err);
                let _ = stream
                    .write_all(failure_page(cb.platform, detail).as_bytes())
                    .await;
                return Err(format!(
                    "Authorization refused by {}: {detail}",
                    cb.platform
                ));
            }
            if returned_state != cb.state {
                let msg = "state mismatch (possible CSRF) — restart the authentication";
                let _ = stream
                    .write_all(failure_page(cb.platform, msg).as_bytes())
                    .await;
                return Err(format!("Auth failed: {msg}"));
            }
            if code.is_empty() {
                let msg = "no authorization code in the callback";
                let _ = stream
                    .write_all(failure_page(cb.platform, msg).as_bytes())
                    .await;
                return Err(format!("Auth failed: {msg}"));
            }

            let _ = stream
                .write_all(success_page(cb.platform, cb.alias).as_bytes())
                .await;
            return Ok(code.to_string());
        }
    });

    wait.await
        .map_err(|_| "Timeout waiting for the OAuth callback (5 min)".to_string())?
}

/// Split a query string and percent-decode both names and values.
fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            let mut parts = p.splitn(2, '=');
            let k = parts.next()?;
            let v = parts.next().unwrap_or("");
            Some((percent_decode(k), percent_decode(v)))
        })
        .collect()
}

/// Minimal `application/x-www-form-urlencoded` decoder.
///
/// Authorization codes are long and opaque; decoding one wrongly produces a
/// confusing "invalid code" error from the provider rather than a local one.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            // Work on bytes, never on `&s[i..]`: slicing a str at a
            // non-char-boundary would panic on malformed input.
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                let hi = (bytes[i + 1] as char).to_digit(16).unwrap_or(0) as u8;
                let lo = (bytes[i + 2] as char).to_digit(16).unwrap_or(0) as u8;
                out.push(hi * 16 + lo);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode a value for use in a query string.
///
/// Deliberately conservative: everything outside the unreserved set is escaped,
/// which is always valid and spares the crate a URL-encoding dependency.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

const PAGE_HEAD: &str = concat!(
    "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Authentification {platform}</title></head>",
    "<body style=\"font-family:-apple-system,system-ui,sans-serif;display:flex;justify-content:center;align-items:center;min-height:100vh;margin:0;background:#fafafa\">",
    "<div style=\"text-align:center;padding:2rem;background:#fff;border-radius:12px;box-shadow:0 2px 12px rgba(0,0,0,.08);max-width:420px\">",
);

fn success_page(platform: &str, alias: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n{}\
         <div style=\"font-size:3rem;margin-bottom:1rem\">\u{2705}</div>\
         <h1 style=\"color:#28a745;font-size:1.4rem;margin:0 0 .5rem\">Authentification r\u{00E9}ussie !</h1>\
         <p style=\"color:#333;margin:0 0 1rem;font-size:.95rem\">Compte <strong>\"{alias}\"</strong> connect\u{00E9}</p>\
         <p style=\"color:#999;margin:0;font-size:.85rem\">Vous pouvez fermer cet onglet.</p>\
         </div></body></html>",
        PAGE_HEAD.replace("{platform}", platform)
    )
}

fn failure_page(platform: &str, msg: &str) -> String {
    format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n{}\
         <div style=\"font-size:3rem;margin-bottom:1rem\">\u{274C}</div>\
         <h1 style=\"color:#dc3545;font-size:1.4rem;margin:0 0 .5rem\">Authentification \u{00E9}chou\u{00E9}e</h1>\
         <p style=\"color:#666;margin:0;font-size:.95rem\">{msg}</p>\
         </div></body></html>",
        PAGE_HEAD.replace("{platform}", platform)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing_decodes_values() {
        let params = parse_query("code=AQ%2Bxyz%3D%3D&state=abc123");
        assert_eq!(params[0], ("code".to_string(), "AQ+xyz==".to_string()));
        assert_eq!(params[1], ("state".to_string(), "abc123".to_string()));
    }

    #[test]
    fn query_parsing_tolerates_empty_and_valueless_pairs() {
        let params = parse_query("a=&&b");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], ("a".to_string(), String::new()));
        assert_eq!(params[1], ("b".to_string(), String::new()));
    }

    #[test]
    fn percent_decode_leaves_malformed_escapes_alone() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("a+b%20c"), "a b c");
    }

    #[test]
    fn urlencode_escapes_everything_reserved() {
        assert_eq!(
            urlencode("w_member_social openid profile"),
            "w_member_social%20openid%20profile"
        );
        assert_eq!(
            urlencode("http://localhost:8765/callback"),
            "http%3A%2F%2Flocalhost%3A8765%2Fcallback"
        );
        assert_eq!(urlencode("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn urlencode_round_trips_through_the_decoder() {
        let raw = "sc:ope=a+b&c/d";
        assert_eq!(percent_decode(&urlencode(raw)), raw);
    }

    #[test]
    fn state_is_32_hex_chars() {
        let s = random_state();
        assert_eq!(s.len(), 32);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_confirmation_page_names_the_platform_and_the_alias() {
        let page = success_page("LinkedIn", "quentin");
        assert!(page.contains("200 OK"));
        assert!(page.contains("Authentification LinkedIn"));
        assert!(page.contains("\"quentin\""));
    }

    #[test]
    fn the_failure_page_carries_the_reason() {
        let page = failure_page("Meta", "state mismatch");
        assert!(page.contains("400 Bad Request"));
        assert!(page.contains("state mismatch"));
    }
}
