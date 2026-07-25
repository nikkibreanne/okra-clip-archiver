//! Web portal backend: a tiny axum server that exposes the pipeline as JSON and
//! serves the React build. `okra-clip-archiver serve` opens it in the browser.
//!   GET  /api/clips   → the planned clips (same data the CLI prints)
//!   POST /api/render  → { id } renders that clip's vertical with ffmpeg

use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
use tower_http::services::ServeDir;

use crate::pipeline::{self, Cfg, PlanRow};

#[derive(Clone)]
struct AppState {
    cfg: Arc<Cfg>,
}

pub async fn serve(cfg: Cfg, port: u16) -> Result<()> {
    let web = web_dir();
    let state = AppState { cfg: Arc::new(cfg) };

    let app = Router::new()
        .route("/api/clips", get(clips))
        .route("/api/render", post(render))
        .route("/api/layout", get(get_layout).post(post_layout))
        .route("/api/frame", get(frame))
        .fallback_service(ServeDir::new(&web))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let url = format!("http://{addr}");
    println!("okra-clip-archiver portal → {url}");
    if !web.join("index.html").exists() {
        eprintln!("(no UI build at {} — run `npm --prefix web install && npm --prefix web run build`)", web.display());
    }
    let _ = open::that(&url);
    axum::serve(listener, app).await?;
    Ok(())
}

/// Locate the built React app: `web/dist` next to the working dir, else next to the exe.
fn web_dir() -> PathBuf {
    let cwd = PathBuf::from("web/dist");
    if cwd.exists() {
        return cwd;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("web/dist");
            if p.exists() {
                return p;
            }
        }
    }
    cwd
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
    Ok(([(axum::http::header::CONTENT_TYPE, "image/jpeg")], jpeg).into_response())
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
