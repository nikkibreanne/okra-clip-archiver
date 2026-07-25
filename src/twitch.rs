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
}

impl Twitch {
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

    /// Up to 100 clips created after `started_at` (RFC3339).
    pub async fn clips(&self, broadcaster_id: &str, started_at: &str) -> Result<Vec<ClipDto>> {
        #[derive(Deserialize)] struct R { data: Vec<ClipDto> }
        let r: R = self.get(
            "https://api.twitch.tv/helix/clips",
            &[("broadcaster_id", broadcaster_id), ("started_at", started_at), ("first", "100")],
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
