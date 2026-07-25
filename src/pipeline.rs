//! Shared pipeline used by BOTH the CLI and the web server: fetch a channel's
//! clips, plan each against the local recordings, and render on demand. Keeping
//! this in one place means the GUI and the terminal produce identical results.

use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::planner::{find_recording, parse_obs_filename_epoch, plan_clip, Clip, Plan, Recording, DEFAULT_VERTICAL_FILTER};
use crate::twitch::{ClipDto, Twitch};

/// Everything the pipeline needs; built from CLI args or the server config.
#[derive(Clone)]
pub struct Cfg {
    pub channel: String,
    pub client_id: String,
    pub client_secret: String,
    pub recordings: Option<String>,
    pub days: i64,
    pub pad: f64,
    pub out_dir: String,
    pub firebase_url: Option<String>,
}

/// One clip's plan, JSON-serialized for the web UI.
#[derive(Serialize, Clone)]
pub struct PlanRow {
    pub id: String,
    pub title: String,
    pub url: String,
    pub duration: f64,
    pub status: String, // ready | skip | unmappable | no-recording
    pub reason: Option<String>,
    pub in_sec: Option<f64>,
    pub out_sec: Option<f64>,
    pub local_path: Option<String>,
    pub out_path: Option<String>,
    pub ffmpeg: Option<Vec<String>>,
    pub vod_offset: Option<i64>,
}

/// Fetch + plan every ≤60s clip for the channel.
pub async fn plan_rows(cfg: &Cfg) -> Result<Vec<PlanRow>> {
    let tw = Twitch::app_token(&cfg.client_id, &cfg.client_secret).await?;
    let broadcaster_id = tw.user_id(&cfg.channel).await?;

    let now = chrono::Utc::now();
    let started_at = (now - chrono::Duration::days(cfg.days)).to_rfc3339();
    let ended_at = now.to_rfc3339();
    let mut clips = tw.clips(&broadcaster_id, &started_at, &ended_at).await?;
    clips.retain(|c| c.duration <= 60.0);

    // VOD start times, one lookup per unique video id (the timeline anchor).
    let mut vod_start: HashMap<String, i64> = HashMap::new();
    let video_ids: HashSet<String> = clips.iter().map(|c| c.video_id.clone()).filter(|v| !v.is_empty()).collect();
    for vid in video_ids {
        if let Ok(Some(ms)) = tw.video_start_ms(&vid).await {
            vod_start.insert(vid, ms);
        }
    }

    let recordings = scan_recordings(cfg.recordings.as_deref());

    // The saved two-box layout, if any; otherwise the blurred-fill fallback.
    let filter = crate::layout::load().map(|l| l.filter()).unwrap_or_else(|| DEFAULT_VERTICAL_FILTER.to_string());

    let mut rows = Vec::new();
    for c in &clips {
        let clip = to_clip(c);
        let vs = vod_start.get(&c.video_id).copied();
        let moment = match (vs, c.vod_offset) { (Some(s), Some(o)) => Some(s + o * 1000), _ => None };
        let rec = moment.and_then(|m| find_recording(&recordings, m));
        let plan = plan_clip(&clip, vs, rec, cfg.pad, &filter, &cfg.out_dir);

        let mut row = PlanRow {
            id: c.id.clone(), title: c.title.clone(), url: c.url.clone(), duration: c.duration,
            status: String::new(), reason: None, in_sec: None, out_sec: None,
            local_path: None, out_path: None, ffmpeg: None, vod_offset: c.vod_offset,
        };
        match plan {
            Plan::Ready { in_sec, out_sec, local_path, out_path, ffmpeg } => {
                row.status = "ready".into();
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
    let cmd = row.ffmpeg.as_ref().ok_or_else(|| anyhow::anyhow!("clip is not renderable ({})", row.status))?;
    if let Some(parent) = std::path::Path::new(cmd.last().unwrap()).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let status = tokio::process::Command::new(&cmd[0]).args(&cmd[1..]).status().await?;
    anyhow::ensure!(status.success(), "ffmpeg exited with {status}");
    Ok(())
}

/// Extract a single JPEG frame from a recording at time `t` (seconds) — the
/// backdrop the layout editor draws the two boxes on. Returns the JPEG bytes.
pub async fn extract_frame(path: &str, t: f64) -> Result<Vec<u8>> {
    let out = tokio::process::Command::new("ffmpeg")
        .args(["-ss", &format!("{t:.2}"), "-i", path, "-frames:v", "1", "-f", "image2pipe", "-vcodec", "mjpeg", "-"])
        .output()
        .await?;
    anyhow::ensure!(out.status.success(), "ffmpeg frame extract failed");
    anyhow::ensure!(!out.stdout.is_empty(), "ffmpeg produced no frame");
    Ok(out.stdout)
}

/// Verify ffmpeg is launchable, with an actionable message if it isn't.
pub async fn ensure_ffmpeg() -> Result<()> {
    tokio::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => anyhow::anyhow!(
                "ffmpeg not found on PATH — install it (WSL/Linux: `sudo apt install -y ffmpeg`; \
                 Windows: put ffmpeg.exe next to this tool) then re-run"
            ),
            _ => anyhow::anyhow!("couldn't run ffmpeg: {e}"),
        })?;
    Ok(())
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

fn scan_recordings(dir: Option<&str>) -> Vec<Recording> {
    let mut out = Vec::new();
    let Some(dir) = dir else { return out };
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
