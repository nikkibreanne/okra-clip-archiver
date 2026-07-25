//! Manual VOD → local-recording mappings.
//!
//! Normally we infer which recording was rolling when a clip happened by
//! comparing the VOD's start time to the recording's start time. That fails when
//! OBS filenames were renamed, the clock drifted, the file was copied (mtime
//! reset), or the VOD has already expired. This module lets the user pin a VOD to
//! a specific file by hand — and nudge the alignment — from the portal.
//!
//! `vod_zero_at_sec` is the position IN THE RECORDING where the VOD's t=0 sits.
//! A clip at `vod_offset` seconds into the VOD is therefore at
//! `vod_zero_at_sec + vod_offset` seconds into the file.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Mapping {
    pub recording_path: String,
    #[serde(default)]
    pub vod_zero_at_sec: f64,
}

pub type Mappings = BTreeMap<String, Mapping>; // video_id → mapping

fn path() -> PathBuf {
    crate::settings::config_dir().join("mappings.json")
}

pub fn load() -> Mappings {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_all(m: &Mappings) -> Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(p, serde_json::to_string_pretty(m)?)?;
    Ok(())
}

/// Pin a VOD to a recording (or pass `None` to clear it and fall back to inference).
pub fn set(video_id: &str, mapping: Option<Mapping>) -> Result<()> {
    let mut all = load();
    match mapping {
        Some(m) => { all.insert(video_id.to_string(), m); }
        None => { all.remove(video_id); }
    }
    save_all(&all)
}
