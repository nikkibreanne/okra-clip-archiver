//! Read the clip-sync "clapperboard" anchors kennyBot writes on `!start`. The
//! `clipSync` RTDB path is public-read, so this needs no credentials — just the
//! database URL. Anchors let the archiver align a stream precisely (and, later,
//! use the marker clip's vod_offset as the exact VOD anchor).

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug, Clone)]
pub struct SyncAnchor {
    #[serde(rename = "startedAt")]
    pub started_at: Option<i64>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(rename = "markerClipId", default)]
    pub marker_clip_id: Option<String>,
}

/// All anchors, keyed by session id. `db_url` e.g. https://okrafans-default-rtdb.…app
pub async fn anchors(db_url: &str) -> Result<HashMap<String, SyncAnchor>> {
    let url = format!("{}/clipSync.json", db_url.trim_end_matches('/'));
    let map: Option<HashMap<String, SyncAnchor>> =
        reqwest::get(&url).await?.error_for_status()?.json().await?;
    Ok(map.unwrap_or_default())
}
