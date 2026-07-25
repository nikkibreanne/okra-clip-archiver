//! okra-clip-archiver — pull a Twitch stream's clips and cut local high-res (4K)
//! verticals for Shorts/TikTok.
//!   (default)  dry-run/render in the terminal (`--run` to execute ffmpeg)
//!   serve      launch the web portal (opens your browser)

mod firebase;
mod pipeline;
mod planner;
mod server;
mod twitch;

use anyhow::Result;
use clap::{Parser, Subcommand};
use pipeline::Cfg;

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
    /// Output folder for rendered verticals
    #[arg(long, default_value = "out")]
    out_dir: String,
    /// Actually run ffmpeg (default: dry-run — print the commands only)
    #[arg(long)]
    run: bool,
    #[arg(long, env = "TWITCH_CLIENT_ID")]
    client_id: String,
    #[arg(long, env = "TWITCH_CLIENT_SECRET")]
    client_secret: String,
    /// Firebase RTDB base URL (reads the clipSync clapperboard anchors)
    #[arg(long, env = "FIREBASE_DATABASE_URL")]
    firebase_url: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Launch the web portal (opens your browser)
    Serve {
        #[arg(long, default_value_t = 8787)]
        port: u16,
    },
}

impl Args {
    fn cfg(&self) -> Cfg {
        Cfg {
            channel: self.channel.clone(),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            recordings: self.recordings.clone(),
            days: self.days,
            pad: self.pad,
            out_dir: self.out_dir.clone(),
            firebase_url: self.firebase_url.clone(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let args = Args::parse();
    let cfg = args.cfg();
    match args.command {
        Some(Command::Serve { port }) => server::serve(cfg, port).await,
        None => run_cli(cfg, args.run).await,
    }
}

async fn run_cli(cfg: Cfg, run: bool) -> Result<()> {
    if run {
        pipeline::ensure_ffmpeg().await?; // fail fast before any network calls
    }

    // Clapperboard anchors (informational for now; precise-alignment uses them later).
    if let Some(url) = &cfg.firebase_url {
        match firebase::anchors(url).await {
            Ok(a) => eprintln!("clipSync anchors: {}", a.len()),
            Err(e) => eprintln!("clipSync read failed (non-fatal): {e}"),
        }
    }

    let rows = pipeline::plan_rows(&cfg).await?;
    let ready = rows.iter().filter(|r| r.status == "ready").count();
    eprintln!("\n{} — {} clips ≤60s · mode {}\n", cfg.channel, rows.len(), if run { "RUN" } else { "DRY-RUN" });

    for r in &rows {
        println!("• {}  [{:.0}s]  {}", trunc(&r.title, 48), r.duration, r.url);
        if r.status == "ready" {
            println!(
                "  cut {:.2}s → {:.2}s  →  {}",
                r.in_sec.unwrap_or(0.0),
                r.out_sec.unwrap_or(0.0),
                r.out_path.as_deref().unwrap_or("")
            );
            if let Some(cmd) = &r.ffmpeg {
                println!("  $ {}", shell_join(cmd));
            }
            if run {
                match pipeline::render(r).await {
                    Ok(()) => println!("  ✓ rendered"),
                    Err(e) => println!("  ✗ render failed: {e}"),
                }
            }
        } else {
            let win = r
                .vod_offset
                .map(|o| format!("vod in={o}s out={}s", o + r.duration as i64))
                .unwrap_or_else(|| "vod_offset pending".into());
            println!("  {}: {}  ({win})", r.status, r.reason.as_deref().unwrap_or(""));
        }
    }
    eprintln!("\n{ready} ready{}", if run { " (rendered)" } else { " (dry-run — pass --run to render)" });
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
