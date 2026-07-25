//! "Sign in with Twitch" via the OAuth **Device Code Flow**.
//!
//! Why this flow: it needs NO client secret, so a non-technical user never has to
//! visit the developer console or paste credentials. They click Sign in, we show
//! an 8-character code, they enter it at twitch.tv/activate, and we poll until
//! Twitch hands us a token. (A client ID is still required, but it's a public
//! identifier — it ships with the app.)
//!
//! The resulting USER token replaces the app token for reading clips/videos, and
//! tells us who signed in, so the channel can default to their own.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const DEVICE_URL: &str = "https://id.twitch.tv/oauth2/device";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";

/// Reading clips, videos, and "who am I" are all public — no scopes needed, so we
/// ask for none and the consent screen stays minimal.
const SCOPES: &str = "";

/// The app's public client ID. Baked at build time via OKRA_TWITCH_CLIENT_ID so a
/// shipped build can offer one-click sign-in; a user (or dev) can still override
/// it in Settings.
pub fn default_client_id() -> String {
    option_env!("OKRA_TWITCH_CLIENT_ID").unwrap_or("").to_string()
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Deserialize)]
#[allow(dead_code)] // expires_in is informational; we refresh reactively on 401
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: i64,
}

/// Kick off a device authorization; the caller shows `user_code` + `verification_uri`.
pub async fn start_device(client_id: &str) -> Result<DeviceStart> {
    anyhow::ensure!(!client_id.trim().is_empty(), "no Twitch client ID configured");
    let res = reqwest::Client::new()
        .post(DEVICE_URL)
        .form(&[("client_id", client_id), ("scopes", SCOPES)])
        .send()
        .await?;
    let status = res.status();
    let body = res.text().await?;
    anyhow::ensure!(status.is_success(), "Twitch rejected the sign-in request ({status}): {body}");
    serde_json::from_str(&body).context("unexpected response from Twitch")
}

/// One poll of the token endpoint.
/// `Ok(None)` = the user hasn't finished authorizing yet (keep polling).
pub async fn poll_device(client_id: &str, device_code: &str) -> Result<Option<TokenResponse>> {
    let res = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("scopes", SCOPES),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await?;

    let status = res.status();
    let body = res.text().await?;
    if status.is_success() {
        return Ok(Some(serde_json::from_str(&body)?));
    }
    // Still waiting on the user — the documented, expected "error".
    if body.contains("authorization_pending") {
        return Ok(None);
    }
    if body.contains("expired") {
        anyhow::bail!("that code expired — start again");
    }
    anyhow::bail!("sign-in failed: {body}")
}

/// Exchange a refresh token for a fresh access token. Twitch refresh tokens are
/// single-use, so the new one must be stored.
pub async fn refresh(client_id: &str, refresh_token: &str) -> Result<TokenResponse> {
    let res = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?;
    let status = res.status();
    let body = res.text().await?;
    anyhow::ensure!(status.is_success(), "couldn't refresh the Twitch session: {body}");
    Ok(serde_json::from_str(&body)?)
}
