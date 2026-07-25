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
the **installer** (`-setup.exe`, bundles ffmpeg) or the portable **zip**.

Double-click it — the portal opens in your browser and a small console window stays
open behind it (closing that window quits the app). Then:

1. **Settings → Sign in with Twitch.** You'll type a short code on Twitch's own site.
   No passwords, no developer keys.
2. **Point it at your OBS recordings folder.**
3. Go to **Clips** and hit Render.

The first-run screen shows these steps with ticks as you complete them.

## The portal
`okra-clip-archiver serve` (what the shortcut runs) opens five tabs:

- **Clips** — every clip in your look-back window with thumbnails and status (ready /
  waiting on VOD / not mapped). **Preview** plays the exact window as it will be cut
  from your local recording; **Render** queues it. The render queue runs one job at a
  time with live progress, encode speed, ETA, and **Cancel**.
- **VOD mapping** — clips are matched to the recording that was rolling at the time.
  When that guess is wrong (renamed files, clock drift, an expired VOD), pick the file
  yourself with radio buttons and nudge the offset until cuts land where you expect.
- **Vertical layout** — draw **two boxes** on a real frame from your recording; they're
  stacked into a 1080×1920 vertical. **Lock to 9:16** keeps each box the shape of half
  the output so nothing is stretched. Set once, reused by every render.
- **Uploads** — what YouTube Shorts and TikTok posting will require. Not built yet.
- **Settings** — **Sign in with Twitch** (a short code on Twitch's own site; no
  passwords, no developer keys) plus everything else: which channel to archive,
  recordings and output folders, look-back/padding/max length, render quality, and
  the upload targets. Secrets are masked. Changes apply immediately — no restart.

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
- `src/auth.rs` — "Sign in with Twitch" (OAuth device code flow, no client secret)
- `src/jobs.rs` — the render queue: progress, ETA, cancellation
- `src/mapping.rs` — manual VOD → recording overrides
- `src/settings.rs` — settings model, `.env` round-tripping, live store (unit-tested)
- `src/planner.rs` — pure clip→cut planning (unit-tested)
- `src/pipeline.rs` — fetch + plan + render, shared by the CLI and the portal
- `src/layout.rs` — the two-box vertical layout → ffmpeg crop/vstack filter
- `src/server.rs` — axum API + embedded React app
- `src/twitch.rs` / `src/firebase.rs` — Helix reads / clipSync anchors
- `web/` — the React portal

## Troubleshooting

**“Waiting on VOD”** — Twitch publishes a clip's position in the broadcast a few
minutes after the clip is made, and only if *Store past broadcasts* is on
(Twitch → Creator Dashboard → Settings → Stream → VOD settings). Refresh later.

**“Not mapped”** — no local recording is matched to that broadcast. Open **VOD
mapping** and pick the file yourself. This also covers renamed files, copied files
(their timestamps change), and VODs that have already expired.

**Cuts land early or late** — the mapping is on the right file but the alignment is
off. On **VOD mapping**, adjust *“Broadcast 0:00 is at N seconds into the file”*:
decrease it if cuts land late, increase it if they land early. Use **Preview** on a
clip to check without spending an encode.

**No clips at all** — check the channel name and the look-back window in Settings.
Twitch only keeps VODs 14–60 days, and clips older than their VOD can't be aligned.

**Renders are slow** — that's ffmpeg re-encoding. Raise *CRF* or pick a faster
*x264 preset* in Settings → Render quality. The queue runs one job at a time on
purpose; you can cancel any of them.

## Roadmap
- [ ] Uploaders: YouTube Data API v3 `videos.insert`; TikTok Content Posting API
      (credentials are already configurable on the Settings page)
- [ ] Precise alignment via kennyBot's marker clip (`markerClipId`)
- [ ] Upload queue + history
