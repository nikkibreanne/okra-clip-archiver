//! Minimal Twitch Helix client (app token) — just the reads the archiver needs.

use anyhow::{Context, Result};
use serde::Deserialize;

pub struct Twitch {
    client: reqwest::Client,
    client_id: String,
    token: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ClipDto {
    pub id: String,
    pub title: String,
    pub url: String,
    pub duration: f64,
    pub created_at: String,
    pub video_id: String,
    #[serde(default)]
    pub vod_offset: Option<i64>,
    #[serde(default)]
    pub thumbnail_url: String,
    #[serde(default)]
    pub creator_name: String,
}

impl Twitch {
    /// Build from the saved settings: prefer the signed-in USER token (device
    /// flow, no secret needed); fall back to a client-credentials app token when
    /// only an id+secret are configured. Refreshes the user token once if Twitch
    /// says it's stale, persisting the rotated refresh token.
    pub async fn from_settings(s: &crate::settings::Settings) -> Result<Self> {
        s.twitch_ready()?;
        if !s.twitch_access_token.trim().is_empty() {
            let me = Self {
                client: reqwest::Client::new(),
                client_id: s.twitch_client_id.clone(),
                token: s.twitch_access_token.clone(),
            };
            // Cheap liveness probe; on 401 refresh and retry once.
            if me.validate().await.is_ok() {
                return Ok(me);
            }
            if !s.twitch_refresh_token.trim().is_empty() {
                let t = crate::auth::refresh(&s.twitch_client_id, &s.twitch_refresh_token).await?;
                crate::settings::update(&serde_json::json!({
                    "twitch_access_token": t.access_token,
                    "twitch_refresh_token": t.refresh_token,
                }))?;
                return Ok(Self {
                    client: reqwest::Client::new(),
                    client_id: s.twitch_client_id.clone(),
                    token: crate::settings::get().twitch_access_token,
                });
            }
            anyhow::bail!("your Twitch sign-in expired — sign in again on the Settings page");
        }
        Self::app_token(&s.twitch_client_id, &s.twitch_client_secret).await
    }

    /// Wrap an already-obtained user token (used right after sign-in).
    pub fn with_token(client_id: &str, token: &str) -> Self {
        Self { client: reqwest::Client::new(), client_id: client_id.to_string(), token: token.to_string() }
    }

    /// Confirm the token is still accepted.
    async fn validate(&self) -> Result<()> {
        let res = self
            .client
            .get("https://id.twitch.tv/oauth2/validate")
            .header("Authorization", format!("OAuth {}", self.token))
            .send()
            .await?;
        anyhow::ensure!(res.status().is_success(), "token rejected");
        Ok(())
    }

    /// The login of whoever this (user) token belongs to.
    pub async fn current_user(&self) -> Result<(String, String)> {
        #[derive(Deserialize)] struct U { id: String, login: String }
        #[derive(Deserialize)] struct R { data: Vec<U> }
        let r: R = self.get("https://api.twitch.tv/helix/users", &[]).await?;
        r.data.into_iter().next().map(|u| (u.id, u.login)).context("no user for this token")
    }

    /// Client-credentials app token (Get Clips / Get Videos / Get Users are public).
    pub async fn app_token(client_id: &str, client_secret: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct Tok { access_token: String }
        let client = reqwest::Client::new();
        let tok: Tok = client
            .post("https://id.twitch.tv/oauth2/token")
            .form(&[
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("grant_type", "client_credentials"),
            ])
            .send().await?.error_for_status()?
            .json().await?;
        Ok(Self { client, client_id: client_id.to_string(), token: tok.access_token })
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, url: &str, query: &[(&str, &str)]) -> Result<T> {
        Ok(self.client.get(url)
            .header("Client-Id", &self.client_id)
            .bearer_auth(&self.token)
            .query(query)
            .send().await?.error_for_status()?
            .json().await?)
    }

    pub async fn user_id(&self, login: &str) -> Result<String> {
        #[derive(Deserialize)] struct U { id: String }
        #[derive(Deserialize)] struct R { data: Vec<U> }
        let r: R = self.get("https://api.twitch.tv/helix/users", &[("login", login)]).await?;
        r.data.into_iter().next().map(|u| u.id).context("channel not found")
    }

    /// Up to 100 clips in [`started_at`, `ended_at`] (RFC3339). Both bounds are
    /// required together — Twitch otherwise defaults ended_at to one WEEK after
    /// started_at, silently dropping anything more recent than that.
    pub async fn clips(&self, broadcaster_id: &str, started_at: &str, ended_at: &str) -> Result<Vec<ClipDto>> {
        #[derive(Deserialize)] struct R { data: Vec<ClipDto> }
        let r: R = self.get(
            "https://api.twitch.tv/helix/clips",
            &[("broadcaster_id", broadcaster_id), ("started_at", started_at), ("ended_at", ended_at), ("first", "100")],
        ).await?;
        Ok(r.data)
    }

    /// VOD start (created_at) as epoch ms, if the VOD still exists.
    pub async fn video_start_ms(&self, video_id: &str) -> Result<Option<i64>> {
        #[derive(Deserialize)] struct V { created_at: String }
        #[derive(Deserialize)] struct R { data: Vec<V> }
        let r: R = self.get("https://api.twitch.tv/helix/videos", &[("id", video_id)]).await?;
        Ok(r.data.into_iter().next()
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(&v.created_at).ok())
            .map(|d| d.timestamp_millis()))
    }
}
