//! Shared pipeline used by BOTH the CLI and the web server: fetch a channel's
//! clips, plan each against the local recordings, and render on demand. Keeping
//! this in one place means the GUI and the terminal produce identical results.
//! Every knob comes from `Settings` (see settings.rs), so a change on the
//! Settings page takes effect on the next request.

use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::planner::{find_recording, parse_obs_filename_epoch, plan_clip, Clip, Plan, Recording, RenderOpts, DEFAULT_VERTICAL_FILTER};
use crate::settings::Settings;
use crate::twitch::{ClipDto, Twitch};

/// One clip's plan, JSON-serialized for the web UI.
#[derive(Serialize, Clone)]
pub struct PlanRow {
    pub id: String,
    pub title: String,
    pub url: String,
    pub thumbnail_url: String,
    pub creator: String,
    pub created_at: String,
    pub duration: f64,
    pub status: String, // ready | skip | unmappable | no-recording
    pub reason: Option<String>,
    pub in_sec: Option<f64>,
    pub out_sec: Option<f64>,
    pub local_path: Option<String>,
    pub out_path: Option<String>,
    pub ffmpeg: Option<Vec<String>>,
    pub vod_offset: Option<i64>,
    /// True when out_path already exists on disk (already rendered).
    pub rendered: bool,
}

/// Build the ffmpeg options for the current settings + saved layout.
pub fn render_opts(s: &Settings) -> RenderOpts {
    RenderOpts {
        pad_sec: s.pad_sec,
        max_clip_sec: s.max_clip_sec,
        filter: crate::layout::load().map(|l| l.filter()).unwrap_or_else(|| DEFAULT_VERTICAL_FILTER.to_string()),
        out_dir: s.out_dir.clone(),
        crf: s.video_crf,
        preset: s.video_preset.clone(),
        audio_bitrate: s.audio_bitrate.clone(),
    }
}

/// Fetch + plan every clip for the configured channel.
pub async fn plan_rows(s: &Settings) -> Result<Vec<PlanRow>> {
    let (client_id, client_secret, channel) = s.twitch_creds()?;
    let tw = Twitch::app_token(client_id, client_secret).await?;
    let broadcaster_id = tw.user_id(channel).await?;

    let now = chrono::Utc::now();
    let started_at = (now - chrono::Duration::days(s.days)).to_rfc3339();
    let ended_at = now.to_rfc3339();
    let clips = tw.clips(&broadcaster_id, &started_at, &ended_at).await?;

    // VOD start times, one lookup per unique video id (the timeline anchor).
    let mut vod_start: HashMap<String, i64> = HashMap::new();
    let video_ids: HashSet<String> = clips.iter().map(|c| c.video_id.clone()).filter(|v| !v.is_empty()).collect();
    for vid in video_ids {
        if let Ok(Some(ms)) = tw.video_start_ms(&vid).await {
            vod_start.insert(vid, ms);
        }
    }

    let recordings = scan_recordings(s.recordings_dir.as_str());
    let opts = render_opts(s);

    let mut rows = Vec::new();
    for c in &clips {
        let clip = to_clip(c);
        let vs = vod_start.get(&c.video_id).copied();
        let moment = match (vs, c.vod_offset) { (Some(st), Some(o)) => Some(st + o * 1000), _ => None };
        let rec = moment.and_then(|m| find_recording(&recordings, m));
        let plan = plan_clip(&clip, vs, rec, &opts);

        let mut row = PlanRow {
            id: c.id.clone(),
            title: c.title.trim().to_string(),
            url: c.url.clone(),
            thumbnail_url: c.thumbnail_url.clone(),
            creator: c.creator_name.clone(),
            created_at: c.created_at.clone(),
            duration: c.duration,
            status: String::new(), reason: None, in_sec: None, out_sec: None,
            local_path: None, out_path: None, ffmpeg: None, vod_offset: c.vod_offset,
            rendered: false,
        };
        match plan {
            Plan::Ready { in_sec, out_sec, local_path, out_path, ffmpeg } => {
                row.status = "ready".into();
                row.rendered = std::path::Path::new(&out_path).exists();
                row.in_sec = Some(in_sec);
                row.out_sec = Some(out_sec);
                row.local_path = Some(local_path);
                row.out_path = Some(out_path);
                row.ffmpeg = Some(ffmpeg);
            }
            Plan::Skip(r) => { row.status = "skip".into(); row.reason = Some(r); }
            Plan::Unmappable(r) => { row.status = "unmappable".into(); row.reason = Some(r); }
            Plan::NoRecording(r) => { row.status = "no-recording".into(); row.reason = Some(r); }
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Render one planned (ready) clip with ffmpeg.
pub async fn render(row: &PlanRow) -> Result<()> {
    let cmd = row.ffmpeg.as_ref().ok_or_else(|| anyhow::anyhow!("this clip can't be rendered ({})", row.status))?;
    if let Some(parent) = std::path::Path::new(cmd.last().unwrap()).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let out = tokio::process::Command::new(&cmd[0]).args(&cmd[1..]).output().await?;
    if !out.status.success() {
        // ffmpeg's last stderr line is the useful part — surface it in the UI.
        let err = String::from_utf8_lossy(&out.stderr);
        let tail = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("ffmpeg failed");
        anyhow::bail!("{tail}");
    }
    Ok(())
}

/// Extract a single JPEG frame from a recording at time `t` (seconds) — the
/// backdrop the layout editor draws the two boxes on.
pub async fn extract_frame(path: &str, t: f64) -> Result<Vec<u8>> {
    let out = tokio::process::Command::new("ffmpeg")
        .args(["-ss", &format!("{t:.2}"), "-i", path, "-frames:v", "1", "-f", "image2pipe", "-vcodec", "mjpeg", "-"])
        .output()
        .await?;
    anyhow::ensure!(out.status.success(), "ffmpeg couldn't read a frame from {path}");
    anyhow::ensure!(!out.stdout.is_empty(), "ffmpeg produced no frame");
    Ok(out.stdout)
}

/// Is ffmpeg launchable? (Ok(version-ish string) or an actionable error.)
pub async fn ffmpeg_version() -> Result<String> {
    let out = tokio::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => anyhow::anyhow!(
                "ffmpeg not found — install it (Linux/WSL: `sudo apt install -y ffmpeg`; \
                 Windows: keep ffmpeg.exe next to the app)"
            ),
            _ => anyhow::anyhow!("couldn't run ffmpeg: {e}"),
        })?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().next().unwrap_or("ffmpeg").to_string())
}

/// CLI helper: fail fast with a clear message.
pub async fn ensure_ffmpeg() -> Result<()> {
    ffmpeg_version().await.map(|_| ())
}

fn to_clip(c: &ClipDto) -> Clip {
    Clip {
        id: c.id.clone(),
        title: c.title.clone(),
        duration_sec: c.duration,
        vod_offset_sec: c.vod_offset,
        video_id: Some(c.video_id.clone()),
        created_at_ms: chrono::DateTime::parse_from_rfc3339(&c.created_at).map(|d| d.timestamp_millis()).unwrap_or(0),
        url: c.url.clone(),
    }
}

/// Video files in `dir`, with their start time from the OBS filename (preferred)
/// or the file's mtime.
pub fn scan_recordings(dir: &str) -> Vec<Recording> {
    let mut out = Vec::new();
    if dir.trim().is_empty() {
        return out;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for entry in rd.flatten() {
        let path = entry.path();
        let ext_ok = matches!(
            path.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
            Some("mkv" | "mp4" | "mov" | "flv" | "ts" | "m4v")
        );
        if !ext_ok { continue; }
        let name = entry.file_name().to_string_lossy().to_string();
        let start = parse_obs_filename_epoch(&name).or_else(|| {
            entry.metadata().ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
        });
        if let Some(start_epoch_ms) = start {
            out.push(Recording { path: path.to_string_lossy().to_string(), start_epoch_ms, duration_sec: None });
        }
    }
    out.sort_by_key(|r| r.start_epoch_ms);
    out
}
