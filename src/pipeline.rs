//! Shared pipeline used by BOTH the CLI and the web server: fetch a channel's
//! clips, plan each against the local recordings, and render on demand. Keeping
//! this in one place means the GUI and the terminal produce identical results.
//! Every knob comes from `Settings` (see settings.rs), so a change on the
//! Settings page takes effect on the next request.

use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::planner::{
    anchor_from_inference, find_recording, parse_obs_filename_epoch, plan_clip, Anchor, Clip, Plan, Recording, RenderOpts,
    DEFAULT_VERTICAL_FILTER,
};
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
    pub video_id: String,
    /// How this clip found its recording: "manual" | "inferred" | "none".
    pub anchor_source: String,
}

/// A VOD (Twitch broadcast) that the fetched clips came from, with how it maps to
/// a local recording — the data behind the mapping UI.
#[derive(Serialize, Clone)]
pub struct VodRow {
    pub video_id: String,
    pub clip_count: usize,
    pub first_clip_at: String,
    /// VOD start (epoch ms) if Twitch still has it — null once the VOD expires.
    pub vod_start_ms: Option<i64>,
    pub inferred_path: Option<String>,
    pub mapped_path: Option<String>,
    pub vod_zero_at_sec: f64,
    pub manual: bool,
}

/// A local recording file offered in the mapping UI.
#[derive(Serialize, Clone)]
pub struct RecordingRow {
    pub path: String,
    pub name: String,
    pub start_epoch_ms: i64,
    pub size_mb: u64,
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

/// Everything fetched from Twitch for one pass, so clips + VOD mapping share a
/// single round of API calls.
pub struct Fetched {
    pub clips: Vec<ClipDto>,
    pub vod_start: HashMap<String, i64>,
    pub recordings: Vec<Recording>,
}

pub async fn fetch(s: &Settings) -> Result<Fetched> {
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

    Ok(Fetched { clips, vod_start, recordings: scan_recordings(s.recordings_dir.as_str()) })
}

/// Resolve a VOD's anchor: a manual mapping always wins; otherwise infer it from
/// wall-clock. Returns None when neither is possible.
fn resolve_anchor(
    video_id: &str,
    vod_start: Option<i64>,
    recordings: &[Recording],
    manual: &crate::mapping::Mappings,
) -> Option<Anchor> {
    if let Some(m) = manual.get(video_id) {
        return Some(Anchor { path: m.recording_path.clone(), vod_zero_at_sec: m.vod_zero_at_sec, manual: true });
    }
    let start = vod_start?;
    find_recording(recordings, start).map(|r| anchor_from_inference(r, start))
}

/// The VODs behind the fetched clips, with their inferred/manual mapping.
pub fn vod_rows(f: &Fetched) -> Vec<VodRow> {
    let manual = crate::mapping::load();
    let mut by_vod: HashMap<String, (usize, String)> = HashMap::new();
    for c in &f.clips {
        if c.video_id.is_empty() {
            continue;
        }
        let e = by_vod.entry(c.video_id.clone()).or_insert((0, c.created_at.clone()));
        e.0 += 1;
        if c.created_at < e.1 {
            e.1 = c.created_at.clone();
        }
    }
    let mut rows: Vec<VodRow> = by_vod
        .into_iter()
        .map(|(video_id, (clip_count, first_clip_at))| {
            let vod_start = f.vod_start.get(&video_id).copied();
            let inferred = vod_start.and_then(|st| find_recording(&f.recordings, st).map(|r| anchor_from_inference(r, st)));
            let anchor = resolve_anchor(&video_id, vod_start, &f.recordings, &manual);
            VodRow {
                video_id,
                clip_count,
                first_clip_at,
                vod_start_ms: vod_start,
                inferred_path: inferred.map(|a| a.path),
                mapped_path: anchor.as_ref().map(|a| a.path.clone()),
                vod_zero_at_sec: anchor.as_ref().map(|a| a.vod_zero_at_sec).unwrap_or(0.0),
                manual: anchor.map(|a| a.manual).unwrap_or(false),
            }
        })
        .collect();
    rows.sort_by(|a, b| b.first_clip_at.cmp(&a.first_clip_at));
    rows
}

/// The local recording files available for mapping.
pub fn recording_rows(s: &Settings) -> Vec<RecordingRow> {
    scan_recordings(&s.recordings_dir)
        .into_iter()
        .map(|r| RecordingRow {
            name: std::path::Path::new(&r.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| r.path.clone()),
            size_mb: std::fs::metadata(&r.path).map(|m| m.len() / 1_048_576).unwrap_or(0),
            start_epoch_ms: r.start_epoch_ms,
            path: r.path,
        })
        .collect()
}

/// Fetch + plan every clip for the configured channel.
pub async fn plan_rows(s: &Settings) -> Result<Vec<PlanRow>> {
    plan_from(s, &fetch(s).await?)
}

/// Plan against an already-fetched set (so one request can do both).
pub fn plan_from(s: &Settings, f: &Fetched) -> Result<Vec<PlanRow>> {
    let opts = render_opts(s);
    let manual = crate::mapping::load();

    let mut rows = Vec::new();
    for c in &f.clips {
        let clip = to_clip(c);
        let vs = f.vod_start.get(&c.video_id).copied();
        let anchor = resolve_anchor(&c.video_id, vs, &f.recordings, &manual);
        let plan = plan_clip(&clip, anchor.as_ref(), &opts);

        let mut row = PlanRow {
            video_id: c.video_id.clone(),
            anchor_source: match &anchor {
                Some(a) if a.manual => "manual".into(),
                Some(_) => "inferred".into(),
                None => "none".into(),
            },
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

/// The codecs in a media file, as ffprobe reports them.
async fn probe_codecs(path: &str) -> (Option<String>, Option<String>) {
    let out = tokio::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,codec_name", "-of", "csv=p=0", path])
        .output()
        .await;
    let (mut v, mut a) = (None, None);
    if let Ok(o) = out {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            match line.trim().split_once(',') {
                Some((name, "video")) if v.is_none() => v = Some(name.to_string()),
                Some((name, "audio")) if a.is_none() => a = Some(name.to_string()),
                _ => {}
            }
        }
    }
    (v, a)
}

/// A browser-playable MP4 of one clip window, cut from the local recording.
/// Recordings are usually MKV, which browsers can't play — but the streams inside
/// are normally H.264/AAC, so we REMUX (stream copy) instead of re-encoding:
/// ~0.3s and full quality, versus ~10s and 480p for a transcode. Anything else
/// (HEVC, PCM audio, …) falls back to a small transcode. Cached per clip+window.
pub async fn preview_clip(clip_id: &str, path: &str, start: f64, dur: f64) -> Result<Vec<u8>> {
    let cache_dir = crate::settings::config_dir().join("cache");
    let _ = std::fs::create_dir_all(&cache_dir);
    let safe: String = clip_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    let file = cache_dir.join(format!("prev-{safe}-{:.0}-{:.0}.mp4", start, dur));

    if let Ok(b) = std::fs::read(&file) {
        if !b.is_empty() {
            return Ok(b);
        }
    }

    let (vcodec, acodec) = probe_codecs(path).await;
    let browser_safe_v = matches!(vcodec.as_deref(), Some("h264"));
    let browser_safe_a = matches!(acodec.as_deref(), None | Some("aac") | Some("mp3"));
    let out_path = file.to_string_lossy().to_string();
    let (ss, t) = (format!("{start:.2}"), format!("{dur:.2}"));

    let mut args: Vec<&str> = vec!["-y", "-v", "error", "-ss", &ss, "-i", path, "-t", &t];
    if browser_safe_v && browser_safe_a {
        // Stream copy — seeks to the nearest keyframe, so the window can be a
        // second or two wider than the exact cut. Fine for eyeballing alignment.
        args.extend_from_slice(&["-c", "copy", "-avoid_negative_ts", "make_zero"]);
    } else {
        args.extend_from_slice(&[
            "-vf", "scale=-2:480", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "30",
            "-c:a", "aac", "-b:a", "96k",
        ]);
    }
    args.extend_from_slice(&["-movflags", "+faststart", &out_path]);

    let out = tokio::process::Command::new("ffmpeg").args(&args).output().await?;
    if !out.status.success() || std::fs::metadata(&file).map(|m| m.len() == 0).unwrap_or(true) {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("ffmpeg failed");
        anyhow::bail!("couldn't build a preview: {tail}");
    }
    Ok(std::fs::read(&file)?)
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
