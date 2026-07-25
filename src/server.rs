//! Web portal backend: a tiny axum server exposing the pipeline as JSON and
//! serving the React build (embedded in the exe via rust-embed).
//!
//!   GET  /api/status         → config summary, ffmpeg presence, recording count
//!   GET  /api/clips          → the planned clips
//!   POST /api/render         → { id } renders that clip's vertical
//!   GET  /api/settings       → current settings (secrets masked)
//!   POST /api/settings       → patch + persist settings (live, no restart)
//!   POST /api/settings/test  → verify the Twitch credentials + channel
//!   GET/POST /api/layout     → the two-box vertical layout
//!   GET  /api/frame[?id=]    → preview JPEG for the layout editor
//!   POST /api/reveal         → open the output folder in the file manager

use anyhow::Result;
use axum::{
    extract::Query,
    http::{header, StatusCode, Uri},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{layout, pipeline, settings};

// The built React app, baked into the binary in release builds.
#[derive(rust_embed::RustEmbed)]
#[folder = "web/dist"]
struct Assets;

pub async fn serve(port: u16) -> Result<()> {
    let app = Router::new()
        .route("/api/status", get(status))
        .route("/api/clips", get(clips))
        .route("/api/render", post(render))
        .route("/api/settings", get(get_settings).post(post_settings))
        .route("/api/settings/test", post(test_twitch))
        .route("/api/layout", get(get_layout).post(post_layout).delete(delete_layout))
        .route("/api/frame", get(frame))
        .route("/api/reveal", post(reveal))
        .route("/api/vods", get(vods))
        .route("/api/recordings", get(recordings))
        .route("/api/mapping", post(post_mapping))
        .route("/api/preview", get(preview))
        .route("/api/jobs", get(jobs_list).delete(jobs_clear))
        .route("/api/jobs/cancel", post(jobs_cancel))
        .route("/api/auth/twitch/start", post(auth_start))
        .route("/api/auth/twitch/poll", post(auth_poll))
        .route("/api/auth/twitch/signout", post(auth_signout))
        .fallback(static_handler);

    // Bind the first free port from `port` so a second launch doesn't just fail.
    let (listener, addr) = bind_from(port).await?;
    let url = format!("http://{addr}");
    println!("okra-clip-archiver portal → {url}");
    println!("settings file: {}", settings::env_path().display());
    let _ = open::that(&url);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn bind_from(port: u16) -> Result<(tokio::net::TcpListener, std::net::SocketAddr)> {
    for p in port..port.saturating_add(10) {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], p));
        if let Ok(l) = tokio::net::TcpListener::bind(addr).await {
            return Ok((l, addr));
        }
    }
    anyhow::bail!("no free port in {port}..{}", port + 10)
}

/// Serve an embedded UI asset, falling back to index.html (SPA).
async fn static_handler(uri: Uri) -> axum::response::Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (data, mime) = match Assets::get(path) {
        Some(c) => (c.data.into_owned(), mime_guess::from_path(path).first_or_octet_stream().to_string()),
        None => match Assets::get("index.html") {
            Some(c) => (c.data.into_owned(), "text/html".to_string()),
            None => return (StatusCode::NOT_FOUND, "UI not built").into_response(),
        },
    };
    ([(header::CONTENT_TYPE, mime)], data).into_response()
}

/// Everything the UI needs to explain itself before any Twitch call happens.
async fn status() -> Json<Value> {
    let s = settings::get();
    let (ffmpeg_ok, ffmpeg_msg) = match pipeline::ffmpeg_version().await {
        Ok(v) => (true, v),
        Err(e) => (false, e.to_string()),
    };
    let recordings = pipeline::scan_recordings(&s.recordings_dir);
    Json(json!({
        "channel": s.twitch_channel,
        "configured": s.twitch_ready().is_ok(),
        "missing": s.twitch_ready().err().map(|e| e.to_string()),
        "signed_in_as": s.twitch_user_login,
        "has_client_id": !s.twitch_client_id.trim().is_empty(),
        "recordings_dir": s.recordings_dir,
        "recordings_found": recordings.len(),
        "out_dir": s.out_dir,
        "days": s.days,
        "ffmpeg_ok": ffmpeg_ok,
        "ffmpeg": ffmpeg_msg,
        "has_layout": layout::load().is_some(),
        "settings_path": settings::env_path().to_string_lossy(),
        "uploads": {
            "youtube_enabled": s.youtube_enabled,
            "youtube_ready": !s.youtube_refresh_token.trim().is_empty(),
            "tiktok_enabled": s.tiktok_enabled,
            "tiktok_ready": !s.tiktok_refresh_token.trim().is_empty(),
        },
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn clips() -> Result<Json<Vec<pipeline::PlanRow>>, ApiError> {
    Ok(Json(pipeline::plan_rows(&settings::get()).await?))
}

#[derive(Deserialize)]
struct IdReq {
    id: String,
}

/// Queue one or more renders. Returns immediately — watch /api/jobs for progress.
#[derive(Deserialize)]
struct RenderReq {
    #[serde(default)]
    id: Option<String>,
    /// Queue every ready clip instead of a single id.
    #[serde(default)]
    all: bool,
}

async fn render(Json(req): Json<RenderReq>) -> Result<Json<Value>, ApiError> {
    let rows = pipeline::plan_rows(&settings::get()).await?;
    let targets: Vec<_> = if req.all {
        rows.iter().filter(|r| r.status == "ready").collect()
    } else {
        let id = req.id.clone().ok_or_else(|| anyhow::anyhow!("no clip specified"))?;
        rows.iter().filter(|r| r.id == id).collect()
    };
    if targets.is_empty() {
        return Err(anyhow::anyhow!("nothing to render").into());
    }

    let mut queued = 0;
    for row in targets {
        let (Some(cmd), Some(out)) = (row.ffmpeg.clone(), row.out_path.clone()) else { continue };
        let total = (row.out_sec.unwrap_or(0.0) - row.in_sec.unwrap_or(0.0)).max(0.1);
        let title = if row.title.is_empty() { row.id.clone() } else { row.title.clone() };
        // Already-queued clips are skipped rather than failing the whole batch.
        if crate::jobs::enqueue(row.id.clone(), title, total, cmd, out).is_ok() {
            queued += 1;
        }
    }
    Ok(Json(json!({ "ok": true, "queued": queued })))
}

async fn jobs_list() -> Json<Value> {
    Json(json!({ "jobs": crate::jobs::snapshot() }))
}

async fn jobs_cancel(Json(req): Json<IdReq>) -> Result<Json<Value>, ApiError> {
    crate::jobs::cancel(&req.id)?;
    Ok(Json(json!({ "ok": true })))
}

async fn jobs_clear() -> Json<Value> {
    crate::jobs::clear_finished();
    Json(json!({ "ok": true }))
}

/// Begin "Sign in with Twitch" — the UI shows the code and opens the URL.
async fn auth_start() -> Result<Json<Value>, ApiError> {
    let s = settings::get();
    let d = crate::auth::start_device(&s.twitch_client_id).await?;
    let _ = open::that(&d.verification_uri);
    Ok(Json(serde_json::to_value(d)?))
}

#[derive(Deserialize)]
struct PollReq {
    device_code: String,
}

/// One poll of the device flow. `pending: true` means keep polling.
async fn auth_poll(Json(req): Json<PollReq>) -> Result<Json<Value>, ApiError> {
    let s = settings::get();
    let Some(tok) = crate::auth::poll_device(&s.twitch_client_id, &req.device_code).await? else {
        return Ok(Json(json!({ "pending": true })));
    };

    // Identify the user so we can default the channel to their own.
    let tw = crate::twitch::Twitch::with_token(&s.twitch_client_id, &tok.access_token);
    let (_, login) = tw.current_user().await?;

    let mut patch = json!({
        "twitch_access_token": tok.access_token,
        "twitch_refresh_token": tok.refresh_token,
        "twitch_user_login": login,
    });
    if s.twitch_channel.trim().is_empty() {
        patch["twitch_channel"] = json!(login);
    }
    settings::update(&patch)?;
    Ok(Json(json!({ "ok": true, "login": login, "channel": settings::get().twitch_channel })))
}

async fn auth_signout() -> Result<Json<Value>, ApiError> {
    settings::update(&json!({
        "twitch_access_token": "",
        "twitch_refresh_token": "",
        "twitch_user_login": "",
    }))?;
    Ok(Json(json!({ "ok": true })))
}

async fn get_settings() -> Json<Value> {
    Json(json!({
        "settings": settings::get().to_masked_json(),
        "path": settings::env_path().to_string_lossy(),
        "mask": settings::MASK,
    }))
}

async fn post_settings(Json(patch): Json<Value>) -> Result<Json<Value>, ApiError> {
    let path = settings::update(&patch)?;
    Ok(Json(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "settings": settings::get().to_masked_json(),
    })))
}

/// Verify the saved Twitch app credentials and channel resolve.
async fn test_twitch() -> Result<Json<Value>, ApiError> {
    let s = settings::get();
    let channel = s.twitch_channel.clone();
    let tw = crate::twitch::Twitch::from_settings(&s).await?;
    let user = tw
        .user_id(&channel)
        .await
        .map_err(|_| anyhow::anyhow!("channel '{channel}' not found on Twitch"))?;
    Ok(Json(json!({ "ok": true, "message": format!("connected — '{channel}' resolved (id {user})") })))
}

async fn get_layout() -> Json<Option<layout::Layout>> {
    Json(layout::load())
}

async fn post_layout(Json(l): Json<layout::Layout>) -> Result<Json<Value>, ApiError> {
    layout::save(&l)?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_layout() -> Result<Json<Value>, ApiError> {
    layout::clear()?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct FrameQuery {
    #[serde(default)]
    id: Option<String>,
}

/// A JPEG frame from a clip's recording — the layout editor backdrop. Defaults to
/// the first mappable clip; `?id=` picks a specific one.
async fn frame(Query(q): Query<FrameQuery>) -> Result<axum::response::Response, ApiError> {
    let s = settings::get();
    let rows = pipeline::plan_rows(&s).await?;
    let row = match &q.id {
        Some(id) => rows.iter().find(|r| &r.id == id).ok_or_else(|| anyhow::anyhow!("clip not found"))?,
        None => rows
            .iter()
            .find(|r| r.status == "ready")
            .ok_or_else(|| anyhow::anyhow!("no clip maps to a local recording yet — set your recordings folder in Settings"))?,
    };
    let path = row.local_path.clone().ok_or_else(|| anyhow::anyhow!("that clip has no local recording"))?;
    let mid = ((row.in_sec.unwrap_or(0.0) + row.out_sec.unwrap_or(0.0)) / 2.0).max(0.0);
    let jpeg = pipeline::extract_frame(&path, mid).await?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store")], jpeg).into_response())
}

/// Open the rendered-output folder in Explorer/Finder/xdg-open.
async fn reveal() -> Result<Json<Value>, ApiError> {
    let dir = settings::get().out_dir;
    std::fs::create_dir_all(&dir).ok();
    let abs = std::fs::canonicalize(&dir).unwrap_or_else(|_| std::path::PathBuf::from(&dir));
    open::that(&abs).map_err(|e| anyhow::anyhow!("couldn't open {}: {e}", abs.display()))?;
    Ok(Json(json!({ "ok": true, "path": abs.to_string_lossy() })))
}

/// The VODs behind the current clips + how each maps to a local recording.
async fn vods() -> Result<Json<Value>, ApiError> {
    let s = settings::get();
    let fetched = pipeline::fetch(&s).await?;
    Ok(Json(json!({
        "vods": pipeline::vod_rows(&fetched),
        "recordings": pipeline::recording_rows(&s),
    })))
}

async fn recordings() -> Json<Value> {
    Json(json!({ "recordings": pipeline::recording_rows(&settings::get()) }))
}

#[derive(Deserialize)]
struct MappingReq {
    video_id: String,
    /// null / omitted clears the manual mapping (back to inference).
    #[serde(default)]
    recording_path: Option<String>,
    #[serde(default)]
    vod_zero_at_sec: Option<f64>,
}

async fn post_mapping(Json(req): Json<MappingReq>) -> Result<Json<Value>, ApiError> {
    let m = req.recording_path.filter(|p| !p.trim().is_empty()).map(|p| crate::mapping::Mapping {
        recording_path: p,
        vod_zero_at_sec: req.vod_zero_at_sec.unwrap_or(0.0),
    });
    crate::mapping::set(&req.video_id, m)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct PreviewQuery {
    id: String,
}

/// A small, browser-playable MP4 of the clip's window cut from the LOCAL
/// recording — this is how you eyeball whether a mapping is right before
/// rendering. Recordings are often MKV (which browsers can't play), so we
/// transcode a low-res proxy and cache it.
async fn preview(Query(q): Query<PreviewQuery>) -> Result<axum::response::Response, ApiError> {
    let s = settings::get();
    let rows = pipeline::plan_rows(&s).await?;
    let row = rows.iter().find(|r| r.id == q.id).ok_or_else(|| anyhow::anyhow!("clip not found"))?;
    let path = row.local_path.clone().ok_or_else(|| anyhow::anyhow!("this clip has no local recording mapped"))?;
    let (in_sec, out_sec) = (row.in_sec.unwrap_or(0.0), row.out_sec.unwrap_or(0.0));
    let bytes = pipeline::preview_clip(&row.id, &path, in_sec, out_sec - in_sec).await?;
    Ok((
        [(header::CONTENT_TYPE, "video/mp4"), (header::CACHE_CONTROL, "no-store")],
        bytes,
    )
        .into_response())
}

/// Turn any pipeline error into a 500 with the message (the UI shows it).
struct ApiError(anyhow::Error);

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(e)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError(e.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}
