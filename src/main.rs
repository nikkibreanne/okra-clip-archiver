//! okra-clip-archiver — pull a Twitch stream's clips and cut local high-res
//! (4K) verticals for Shorts/TikTok. This scaffold is the DRY-RUN core: enumerate
//! ≤60s clips, resolve each to a local recording + in/out, and print the exact
//! ffmpeg command it would run (`--run` to execute). A React GUI + uploaders come
//! next; the planner (src/planner.rs) is fully unit-tested.

mod firebase;
mod planner;
mod twitch;

use anyhow::Result;
use clap::Parser;
use std::collections::{HashMap, HashSet};

use planner::{find_recording, parse_obs_filename_epoch, plan_clip, Clip, Plan, Recording, DEFAULT_VERTICAL_FILTER};
use twitch::ClipDto;

#[derive(Parser)]
#[command(name = "okra-clip-archiver", about = "Cut local 4K verticals from a stream's Twitch clips")]
struct Args {
    /// Twitch channel login
    #[arg(long, env = "TWITCH_CHANNEL")]
    channel: String,
    /// Folder holding your OBS recordings (matched to clips by start time)
    #[arg(long, env = "RECORDINGS_DIR")]
    recordings: Option<String>,
    /// Look back this many days
    #[arg(long, default_value_t = 30)]
    days: i64,
    /// Padding (seconds) added around each clip
    #[arg(long, default_value_t = 5.0)]
    pad: f64,
    /// Actually run ffmpeg (default: dry-run — print the commands only)
    #[arg(long)]
    run: bool,
    /// Output folder for rendered verticals
    #[arg(long, default_value = "out")]
    out_dir: String,
    #[arg(long, env = "TWITCH_CLIENT_ID")]
    client_id: String,
    #[arg(long, env = "TWITCH_CLIENT_SECRET")]
    client_secret: String,
    /// Firebase RTDB base URL (reads the clipSync clapperboard anchors)
    #[arg(long, env = "FIREBASE_DATABASE_URL")]
    firebase_url: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let args = Args::parse();

    let tw = twitch::Twitch::app_token(&args.client_id, &args.client_secret).await?;
    let broadcaster_id = tw.user_id(&args.channel).await?;

    let started_at = (chrono::Utc::now() - chrono::Duration::days(args.days)).to_rfc3339();
    let mut clips = tw.clips(&broadcaster_id, &started_at).await?;
    clips.retain(|c| c.duration <= 60.0);

    // VOD start times, one lookup per unique video id (the timeline anchor).
    let mut vod_start: HashMap<String, i64> = HashMap::new();
    let video_ids: HashSet<String> = clips.iter().map(|c| c.video_id.clone()).filter(|v| !v.is_empty()).collect();
    for vid in video_ids {
        if let Ok(Some(ms)) = tw.video_start_ms(&vid).await {
            vod_start.insert(vid, ms);
        }
    }

    // Clapperboard anchors (informational in the scaffold; the precise-alignment
    // refinement will prefer these over the VOD-start estimate).
    if let Some(url) = &args.firebase_url {
        match firebase::anchors(url).await {
            Ok(a) => eprintln!("clipSync anchors: {}", a.len()),
            Err(e) => eprintln!("clipSync read failed (non-fatal): {e}"),
        }
    }

    let recordings = scan_recordings(args.recordings.as_deref());
    eprintln!(
        "\n{} — {} clips ≤60s · {} recording(s) · mode {}\n",
        args.channel, clips.len(), recordings.len(), if args.run { "RUN" } else { "DRY-RUN" }
    );

    let mut ready = 0usize;
    for c in &clips {
        let clip = to_clip(c);
        let vs = vod_start.get(&c.video_id).copied();
        let moment = match (vs, c.vod_offset) { (Some(s), Some(o)) => Some(s + o * 1000), _ => None };
        let rec = moment.and_then(|m| find_recording(&recordings, m));
        let plan = plan_clip(&clip, vs, rec, args.pad, DEFAULT_VERTICAL_FILTER, &args.out_dir);

        println!("• {}  [{:.0}s]  {}", trunc(&c.title, 48), c.duration, c.url);
        match &plan {
            Plan::Ready { in_sec, out_sec, out_path, ffmpeg, .. } => {
                ready += 1;
                println!("  cut {in_sec:.2}s → {out_sec:.2}s  →  {out_path}");
                println!("  $ {}", shell_join(ffmpeg));
                if args.run {
                    run_ffmpeg(ffmpeg).await?;
                    println!("  ✓ rendered");
                }
            }
            Plan::Skip(r) | Plan::Unmappable(r) | Plan::NoRecording(r) => {
                let win = c.vod_offset.map(|o| format!("vod in={o}s out={}s", o + c.duration as i64))
                    .unwrap_or_else(|| "vod_offset pending".into());
                println!("  {r}  ({win})");
            }
        }
    }
    eprintln!("\n{ready} ready{}", if args.run { " (rendered)" } else { " (dry-run — pass --run to render)" });
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

async fn run_ffmpeg(cmd: &[String]) -> Result<()> {
    if let Some(parent) = std::path::Path::new(cmd.last().unwrap()).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let status = tokio::process::Command::new(&cmd[0]).args(&cmd[1..]).status().await?;
    anyhow::ensure!(status.success(), "ffmpeg exited with {status}");
    Ok(())
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n).collect::<String>() + "…" }
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| if a.chars().any(|c| c == ' ' || c == '[' || c == ']') { format!("{a:?}") } else { a.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}
