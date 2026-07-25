//! The vertical layout: two crop boxes on the source frame, stacked into 9:16.
//! This is the "two boxes like Twitch's editor" model — pick a top region and a
//! bottom region of the high-res recording; each fills half of the 1080x1920
//! output. Persisted to layout.json so it's set once and reused for every clip.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A crop rectangle in SOURCE (recording) pixels.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Layout {
    /// Frame dimensions the boxes were drawn against (so the UI can round-trip).
    pub source_w: u32,
    pub source_h: u32,
    pub top: Rect,
    pub bottom: Rect,
    #[serde(default = "default_out_w")]
    pub out_w: u32,
    #[serde(default = "default_out_h")]
    pub out_h: u32,
}

fn default_out_w() -> u32 { 1080 }
fn default_out_h() -> u32 { 1920 }

impl Layout {
    /// ffmpeg `-filter_complex` producing `[v]`: crop each box, scale it to fill
    /// its half (no distortion — scale-to-cover then crop), and vstack the two.
    pub fn filter(&self) -> String {
        let ow = self.out_w;
        let half = self.out_h / 2;
        let crop = |r: &Rect| format!("crop={}:{}:{}:{}", r.w.max(2.0) as i64, r.h.max(2.0) as i64, r.x as i64, r.y as i64);
        format!(
            "[0:v]{t},scale={ow}:{half}:force_original_aspect_ratio=increase,crop={ow}:{half}[top];\
             [0:v]{b},scale={ow}:{half}:force_original_aspect_ratio=increase,crop={ow}:{half}[bot];\
             [top][bot]vstack=inputs=2[v]",
            t = crop(&self.top),
            b = crop(&self.bottom),
        )
    }
}

fn path() -> PathBuf {
    PathBuf::from("layout.json")
}

/// The saved layout, if any (None → callers fall back to the blurred-fill default).
pub fn load() -> Option<Layout> {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok())
}

pub fn save(layout: &Layout) -> Result<()> {
    std::fs::write(path(), serde_json::to_string_pretty(layout)?)?;
    Ok(())
}
