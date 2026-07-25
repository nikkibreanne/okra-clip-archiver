//! Web portal backend: a tiny axum server that exposes the pipeline as JSON and
//! serves the React build (embedded in the exe via rust-embed). `serve` opens it
//! in the browser.
//!   GET  /api/clips   → the planned clips
//!   POST /api/render  → { id } renders that clip's vertical with ffmpeg
//!   GET/POST /api/layout → the two-box vertical layout
//!   GET  /api/frame   → a preview JPEG for the layout editor

use anyhow::Result;
use axum::{
    extract::State,
    http::{header, StatusCode, Uri},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::pipeline::{self, Cfg, PlanRow};

// The built React app, baked into the binary in release builds (read from
// web/dist on disk in debug builds, so `npm run dev` iteration still works).
#[derive(rust_embed::RustEmbed)]
#[folder = "web/dist"]
struct Assets;

#[derive(Clone)]
struct AppState {
    cfg: Arc<Cfg>,
}

pub async fn serve(cfg: Cfg, port: u16) -> Result<()> {
    let state = AppState { cfg: Arc::new(cfg) };

    let app = Router::new()
        .route("/api/clips", get(clips))
        .route("/api/render", post(render))
        .route("/api/layout", get(get_layout).post(post_layout))
        .route("/api/frame", get(frame))
        .fallback(static_handler)
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let url = format!("http://{addr}");
    println!("okra-clip-archiver portal → {url}");
    let _ = open::that(&url);
    axum::serve(listener, app).await?;
    Ok(())
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

async fn clips(State(s): State<AppState>) -> Result<Json<Vec<PlanRow>>, ApiError> {
    Ok(Json(pipeline::plan_rows(&s.cfg).await?))
}

#[derive(Deserialize)]
struct RenderReq {
    id: String,
}

async fn render(State(s): State<AppState>, Json(req): Json<RenderReq>) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = pipeline::plan_rows(&s.cfg).await?;
    let row = rows
        .into_iter()
        .find(|r| r.id == req.id)
        .ok_or_else(|| anyhow::anyhow!("clip not found"))?;
    pipeline::render(&row).await?;
    Ok(Json(serde_json::json!({ "ok": true, "out": row.out_path })))
}

async fn get_layout() -> Json<Option<crate::layout::Layout>> {
    Json(crate::layout::load())
}

async fn post_layout(Json(l): Json<crate::layout::Layout>) -> Result<Json<serde_json::Value>, ApiError> {
    crate::layout::save(&l)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// A JPEG frame from the first ready clip's recording — the layout editor backdrop.
async fn frame(State(s): State<AppState>) -> Result<axum::response::Response, ApiError> {
    let rows = pipeline::plan_rows(&s.cfg).await?;
    let ready = rows
        .iter()
        .find(|r| r.status == "ready")
        .ok_or_else(|| anyhow::anyhow!("no ready clip to preview — pass --recordings so a clip maps to a recording"))?;
    let path = ready.local_path.clone().unwrap();
    let mid = ((ready.in_sec.unwrap_or(0.0) + ready.out_sec.unwrap_or(0.0)) / 2.0).max(0.0);
    let jpeg = pipeline::extract_frame(&path, mid).await?;
    Ok(([(header::CONTENT_TYPE, "image/jpeg")], jpeg).into_response())
}

/// Turn any pipeline error into a 500 with the message (the UI shows it).
struct ApiError(anyhow::Error);

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response()
    }
}
