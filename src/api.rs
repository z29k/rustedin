use crate::token_store;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

const LINKEDIN_VERSION: &str = "202603";
const API_BASE: &str = "https://api.linkedin.com";

async fn api_headers(alias: &str) -> Result<HeaderMap, String> {
    let token = token_store::get_valid_token(alias).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
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

pub async fn linkedin_post(alias: &str, path: &str, body: &Value) -> Result<String, String> {
    let headers = api_headers(alias).await?;
    let client = reqwest::Client::new();

    eprintln!("[rustedin] POST {API_BASE}{path}");
    eprintln!(
        "[rustedin] Body:\n{}",
        serde_json::to_string_pretty(body).unwrap_or_default()
    );

    let res = client
        .post(format!("{API_BASE}{path}"))
        .headers(headers)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!(
            "LinkedIn API error {status} for \"{alias}\": {text}"
        ));
    }

    let post_id = res
        .headers()
        .get("x-restli-id")
        .or_else(|| res.headers().get("X-RestLi-Id"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    Ok(post_id)
}

pub async fn linkedin_get(
    alias: &str,
    path: &str,
    query: &[(&str, &str)],
) -> Result<serde_json::Value, String> {
    let headers = api_headers(alias).await?;
    let client = reqwest::Client::new();

    let res = client
        .get(format!("{API_BASE}{path}"))
        .headers(headers)
        .query(query)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!(
            "LinkedIn API error {status} for \"{alias}\": {text}"
        ));
    }

    res.json()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))
}

pub fn encode_urn(urn: &str) -> String {
    urn.replace(':', "%3A")
}

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

pub async fn initialize_image_upload(
    alias: &str,
    owner_urn: &str,
) -> Result<(String, String), String> {
    let headers = api_headers(alias).await?;
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "initializeUploadRequest": {
            "owner": owner_urn
        }
    });

    let res = client
        .post(format!("{API_BASE}/rest/images?action=initializeUpload"))
        .headers(headers)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Image upload init failed: {e}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("Image upload init error {status}: {text}"));
    }

    let data: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("Image upload init JSON error: {e}"))?;

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

pub async fn upload_image_binary(
    alias: &str,
    upload_url: &str,
    image_bytes: Vec<u8>,
) -> Result<(), String> {
    let token = token_store::get_valid_token(alias).await?;
    let client = reqwest::Client::new();

    let res = client
        .put(upload_url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/octet-stream")
        .body(image_bytes)
        .send()
        .await
        .map_err(|e| format!("Image binary upload failed: {e}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("Image binary upload error {status}: {text}"));
    }

    Ok(())
}

pub fn resolve_urn(alias: &str) -> Result<String, String> {
    let account = token_store::get_account(alias)
        .ok_or_else(|| format!("Account \"{alias}\" not configured."))?;
    account.urn.ok_or_else(|| {
        format!("No URN stored for \"{alias}\". Run: rustedin auth --account={alias}")
    })
}
