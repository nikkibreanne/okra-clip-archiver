//! okra-clip-archiver — pull a Twitch stream's clips and cut local high-res (4K)
//! verticals for Shorts/TikTok.
//!   serve      launch the web portal (opens your browser) — the main UI
//!   (default)  dry-run/render in the terminal (`--run` to execute ffmpeg)
//!
//! Settings live in a .env managed by the portal's Settings page (see settings.rs).
//! CLI flags override the saved values for that invocation.

mod auth;
mod firebase;
mod jobs;
mod layout;
mod mapping;
mod pipeline;
mod planner;
mod server;
mod settings;
mod twitch;

use anyhow::Result;
use clap::{Parser, Subcommand};
use settings::Settings;

#[derive(Parser)]
#[command(name = "okra-clip-archiver", version, about = "Cut local 4K verticals from a stream's Twitch clips")]
struct Args {
    /// Twitch channel login to pull clips from
    #[arg(long, env = "TWITCH_CHANNEL")]
    channel: Option<String>,
    /// Folder holding your OBS recordings (matched to clips by start time)
    #[arg(long, env = "RECORDINGS_DIR")]
    recordings: Option<String>,
    /// Look back this many days
    #[arg(long)]
    days: Option<i64>,
    /// Padding (seconds) added around each clip
    #[arg(long)]
    pad: Option<f64>,
    /// Output folder for rendered verticals
    #[arg(long)]
    out_dir: Option<String>,
    /// Actually run ffmpeg (default: dry-run — print the commands only)
    #[arg(long)]
    run: bool,
    #[arg(long, env = "TWITCH_CLIENT_ID")]
    client_id: Option<String>,
    #[arg(long, env = "TWITCH_CLIENT_SECRET")]
    client_secret: Option<String>,
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
    /// Saved settings with any explicit flags layered on top.
    fn resolve(&self) -> Settings {
        let mut s = Settings::from_env();
        if let Some(v) = &self.channel { s.twitch_channel = v.clone(); }
        if let Some(v) = &self.recordings { s.recordings_dir = v.clone(); }
        if let Some(v) = self.days { s.days = v; }
        if let Some(v) = self.pad { s.pad_sec = v; }
        if let Some(v) = &self.out_dir { s.out_dir = v.clone(); }
        if let Some(v) = &self.client_id { s.twitch_client_id = v.clone(); }
        if let Some(v) = &self.client_secret { s.twitch_client_secret = v.clone(); }
        if let Some(v) = &self.firebase_url { s.firebase_database_url = v.clone(); }
        s
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    settings::load_dotenv_files();

    // Double-clicked (no arguments at all) → open the portal. Anything else keeps
    // the CLI behaviour, so `--channel x --run` still works from a terminal.
    if std::env::args().count() == 1 {
        settings::init(Settings::from_env());
        banner();
        return server::serve(8787).await;
    }

    let args = Args::parse();
    settings::init(args.resolve());

    match args.command {
        Some(Command::Serve { port }) => {
            banner();
            server::serve(port).await
        }
        None => run_cli(args.run).await,
    }
}

/// Shown in the console window that Windows keeps open behind the browser, so a
/// non-technical user knows what it is and how to quit.
fn banner() {
    println!("┌──────────────────────────────────────────────┐");
    println!("│  okra-clip-archiver v{:<24}│", env!("CARGO_PKG_VERSION"));
    println!("│  Your browser should open automatically.     │");
    println!("│  Keep this window open while you work —      │");
    println!("│  closing it quits the app.                   │");
    println!("└──────────────────────────────────────────────┘");
}

async fn run_cli(run: bool) -> Result<()> {
    let s = settings::get();
    if run {
        pipeline::ensure_ffmpeg().await?; // fail fast before any network calls
    }

    // Clapperboard anchors (informational for now; precise alignment uses them later).
    if !s.firebase_database_url.trim().is_empty() {
        match firebase::anchors(&s.firebase_database_url).await {
            Ok(a) => eprintln!("clipSync anchors: {}", a.len()),
            Err(e) => eprintln!("clipSync read failed (non-fatal): {e}"),
        }
    }

    let rows = pipeline::plan_rows(&s).await?;
    let ready = rows.iter().filter(|r| r.status == "ready").count();
    eprintln!(
        "\n{} — {} clips · mode {}\n",
        if s.twitch_channel.is_empty() { "(no channel)" } else { &s.twitch_channel },
        rows.len(),
        if run { "RUN" } else { "DRY-RUN" }
    );

    for r in &rows {
        let title = if r.title.is_empty() { "(untitled)" } else { &r.title };
        println!("• {}  [{:.0}s]  {}", trunc(title, 48), r.duration, r.url);
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
