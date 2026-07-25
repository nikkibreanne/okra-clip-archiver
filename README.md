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

## Build & run
Needs the **Rust toolchain** (`rustup` → `cargo`) and **ffmpeg** on PATH at runtime.

```bash
cargo test          # the planner is fully unit-tested
cargo run -- --channel <login> --days 30 --recordings /path/to/obs/recordings
# add --run to actually render (default is a dry-run that prints ffmpeg commands)
```

Config via flags or env (`.env` supported): `TWITCH_CLIENT_ID`, `TWITCH_CLIENT_SECRET`,
`TWITCH_CHANNEL`, `RECORDINGS_DIR`, `FIREBASE_DATABASE_URL`.

## Layout
- `src/planner.rs` — pure clip→cut planning (unit-tested)
- `src/twitch.rs` — Helix reads (app token)
- `src/firebase.rs` — reads the clipSync anchors
- `src/main.rs` — dry-run/render CLI

## Roadmap
- [ ] `axum` backend serving a **React** portal (load a recording → see its clips → cut/queue)
- [ ] Precise alignment via the marker clip (`markerClipId`)
- [ ] Uploaders: YouTube Data API v3 `videos.insert`; TikTok Content Posting API
