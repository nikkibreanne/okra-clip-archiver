# okra-clip-archiver

Pulls a Twitch channel's clips and cuts **local high-res (4K) verticals** ready for
YouTube Shorts / TikTok. Twitch clips are capped at your stream resolution (~1080p);
the 4K only ever exists in your **local OBS recording**, so this tool maps each clip
back to that recording and re-cuts it at full quality.

Standalone by design — **not** part of [kennyBot](https://github.com/nikkibreanne/kennyBot).
The only coupling is a small read-only contract (below).

## How it works
1. **Get Clips** (Twitch Helix) → each clip carries `video_id`, `vod_offset`, `duration`.
2. **Get Videos** → the VOD's start time (the timeline anchor). The VOD is only a
   *ruler*; the pixels come from your local recording.
3. Map `local_offset = (vod_start + vod_offset) − recording_start`, pad, and
   **ffmpeg**-cut the segment, reframing 16:9 → 9:16.
4. (next) queue + upload to Shorts / TikTok.

### The kennyBot contract
On `!start`, kennyBot writes a "clapperboard" anchor to Firebase RTDB at the
public-read path `clipSync/{sessionId}` = `{ startedAt, channel, markerClipId? }`.
This tool reads it (`FIREBASE_DATABASE_URL`, no credentials) to align a stream
precisely — the marker clip's `vod_offset` pins the VOD timeline exactly.

## Install (Windows)
Grab the latest [release](https://github.com/nikkibreanne/okra-clip-archiver/releases):
the **installer** (`-setup.exe`, bundles ffmpeg) or the portable **zip**. Launch it and
the portal opens in your browser — everything is configured on the **Settings** page.

## The portal
`okra-clip-archiver serve` (what the shortcut runs) opens three tabs:

- **Clips** — every clip in your look-back window with its status (ready / waiting on
  VOD / no local recording), thumbnails, and a Render button per clip or for the batch.
- **Vertical layout** — draw **two boxes** on a real frame from your recording; they're
  stacked into a 1080×1920 vertical. Set once, reused by every render.
- **Settings** — edits the `.env` for you: Twitch app + source channel, recordings and
  output folders, look-back/padding/max length, render quality (CRF, preset, audio),
  and the YouTube / TikTok upload targets. Secrets are masked; "Test Twitch
  connection" verifies your credentials. Changes apply immediately — no restart.

Settings are stored per-user (`%APPDATA%\okra-clip-archiver\.env`, or
`~/.config/okra-clip-archiver/.env`), so an installed copy needs no admin rights. A
`.env` next to the exe or in the working directory is also read.

## Build from source
Needs the **Rust toolchain** (`rustup` → `cargo`), **Node** (for the UI), and **ffmpeg**
at runtime.

```bash
npm --prefix web install && npm --prefix web run build   # UI is embedded in the exe
cargo test                                              # planner + settings unit tests
cargo run -- serve                                      # the portal
cargo run -- --channel <login> --recordings <dir>       # CLI dry-run (--run to render)
```

CLI flags override the saved settings for that run.

## Layout
- `src/settings.rs` — settings model, `.env` round-tripping, live store (unit-tested)
- `src/planner.rs` — pure clip→cut planning (unit-tested)
- `src/pipeline.rs` — fetch + plan + render, shared by the CLI and the portal
- `src/layout.rs` — the two-box vertical layout → ffmpeg crop/vstack filter
- `src/server.rs` — axum API + embedded React app
- `src/twitch.rs` / `src/firebase.rs` — Helix reads / clipSync anchors
- `web/` — the React portal

## Roadmap
- [ ] Uploaders: YouTube Data API v3 `videos.insert`; TikTok Content Posting API
      (credentials are already configurable on the Settings page)
- [ ] Precise alignment via kennyBot's marker clip (`markerClipId`)
- [ ] Upload queue + history
