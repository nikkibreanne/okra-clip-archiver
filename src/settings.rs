//! Application settings — the single source of truth for every knob, editable
//! from the portal's Settings page and persisted to a `.env` file.
//!
//! Design notes:
//!  * One struct, one env key per field (see FIELDS). Adding a setting = add a
//!    field + a FIELDS row; the API, the UI form, and .env round-tripping all
//!    pick it up automatically.
//!  * Values are held in a RwLock so a save takes effect immediately — no restart.
//!  * Writes MERGE into the existing .env: comments, ordering, and unknown keys
//!    a user hand-added are preserved.
//!  * Config lives in a per-user data dir (%APPDATA%/~.config) so an installed
//!    build under Program Files can still save without admin rights. A `.env`
//!    next to the exe or in the working dir is still READ (back-compat).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::RwLock,
};

/// Placeholder the API sends instead of a stored secret, and accepts back to mean
/// "leave unchanged" — so the UI never has to hold real secrets.
pub const MASK: &str = "••••••••";

/// Fields whose values are never sent to the browser in the clear.
pub const SECRET_FIELDS: &[&str] = &[
    "twitch_client_secret",
    "youtube_client_secret",
    "youtube_refresh_token",
    "tiktok_client_secret",
    "tiktok_refresh_token",
];

/// (struct field, .env key). The env keys of pre-existing settings are unchanged
/// for back-compat with hand-written .env files and the CLI flags.
pub const FIELDS: &[(&str, &str)] = &[
    // Twitch — the SOURCE of clips.
    ("twitch_client_id", "TWITCH_CLIENT_ID"),
    ("twitch_client_secret", "TWITCH_CLIENT_SECRET"),
    ("twitch_channel", "TWITCH_CHANNEL"),
    // Local files + selection.
    ("recordings_dir", "RECORDINGS_DIR"),
    ("out_dir", "OUT_DIR"),
    ("days", "LOOKBACK_DAYS"),
    ("pad_sec", "PAD_SEC"),
    ("max_clip_sec", "MAX_CLIP_SEC"),
    // Render tuning.
    ("video_crf", "VIDEO_CRF"),
    ("video_preset", "VIDEO_PRESET"),
    ("audio_bitrate", "AUDIO_BITRATE"),
    // YouTube Shorts upload TARGET (separate from the Twitch source).
    ("youtube_enabled", "YOUTUBE_ENABLED"),
    ("youtube_client_id", "YOUTUBE_CLIENT_ID"),
    ("youtube_client_secret", "YOUTUBE_CLIENT_SECRET"),
    ("youtube_refresh_token", "YOUTUBE_REFRESH_TOKEN"),
    ("youtube_channel_id", "YOUTUBE_CHANNEL_ID"),
    ("youtube_privacy", "YOUTUBE_PRIVACY"),
    ("youtube_title_template", "YOUTUBE_TITLE_TEMPLATE"),
    ("youtube_description_template", "YOUTUBE_DESCRIPTION_TEMPLATE"),
    ("youtube_tags", "YOUTUBE_TAGS"),
    // TikTok upload TARGET.
    ("tiktok_enabled", "TIKTOK_ENABLED"),
    ("tiktok_client_key", "TIKTOK_CLIENT_KEY"),
    ("tiktok_client_secret", "TIKTOK_CLIENT_SECRET"),
    ("tiktok_refresh_token", "TIKTOK_REFRESH_TOKEN"),
    ("tiktok_open_id", "TIKTOK_OPEN_ID"),
    ("tiktok_privacy", "TIKTOK_PRIVACY"),
    ("tiktok_title_template", "TIKTOK_TITLE_TEMPLATE"),
    // kennyBot clapperboard anchors.
    ("firebase_database_url", "FIREBASE_DATABASE_URL"),
];

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    // ── Twitch (clip source) ──
    pub twitch_client_id: String,
    pub twitch_client_secret: String,
    /// Channel login to pull clips FROM. Never defaulted to anyone's channel.
    pub twitch_channel: String,

    // ── Local files + selection ──
    pub recordings_dir: String,
    pub out_dir: String,
    pub days: i64,
    pub pad_sec: f64,
    /// Clips longer than this are skipped (Shorts/TikTok want ≤60s).
    pub max_clip_sec: f64,

    // ── Render tuning ──
    pub video_crf: i64,
    pub video_preset: String,
    pub audio_bitrate: String,

    // ── YouTube Shorts (upload target) ──
    pub youtube_enabled: bool,
    pub youtube_client_id: String,
    pub youtube_client_secret: String,
    pub youtube_refresh_token: String,
    pub youtube_channel_id: String,
    pub youtube_privacy: String,
    pub youtube_title_template: String,
    pub youtube_description_template: String,
    pub youtube_tags: String,

    // ── TikTok (upload target) ──
    pub tiktok_enabled: bool,
    pub tiktok_client_key: String,
    pub tiktok_client_secret: String,
    pub tiktok_refresh_token: String,
    pub tiktok_open_id: String,
    pub tiktok_privacy: String,
    pub tiktok_title_template: String,

    // ── kennyBot integration ──
    pub firebase_database_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            twitch_client_id: String::new(),
            twitch_client_secret: String::new(),
            twitch_channel: String::new(),
            recordings_dir: String::new(),
            out_dir: "out".into(),
            days: 30,
            pad_sec: 5.0,
            max_clip_sec: 60.0,
            video_crf: 18,
            video_preset: "medium".into(),
            audio_bitrate: "160k".into(),
            youtube_enabled: false,
            youtube_client_id: String::new(),
            youtube_client_secret: String::new(),
            youtube_refresh_token: String::new(),
            youtube_channel_id: String::new(),
            youtube_privacy: "private".into(),
            youtube_title_template: "{title} #Shorts".into(),
            youtube_description_template: "Clipped from {channel} on {date}\n{url}".into(),
            youtube_tags: "twitch,clips,shorts".into(),
            tiktok_enabled: false,
            tiktok_client_key: String::new(),
            tiktok_client_secret: String::new(),
            tiktok_refresh_token: String::new(),
            tiktok_open_id: String::new(),
            tiktok_privacy: "SELF_ONLY".into(),
            tiktok_title_template: "{title}".into(),
            firebase_database_url: String::new(),
        }
    }
}

impl Settings {
    /// Build from the process environment (dotenvy has already loaded .env files),
    /// falling back to defaults for anything unset or unparseable.
    pub fn from_env() -> Self {
        let defaults = serde_json::to_value(Settings::default()).unwrap();
        let mut map = defaults.as_object().unwrap().clone();
        for (field, env_key) in FIELDS {
            if let Ok(raw) = std::env::var(env_key) {
                if raw.trim().is_empty() {
                    continue;
                }
                let d = defaults.get(*field).unwrap();
                map.insert((*field).to_string(), coerce(d, &raw));
            }
        }
        serde_json::from_value(Value::Object(map)).unwrap_or_default()
    }

    /// (client_id, client_secret, channel) or an error naming what's still unset,
    /// so the portal can run unconfigured and show an actionable message.
    pub fn twitch_creds(&self) -> Result<(&str, &str, &str)> {
        let mut missing = Vec::new();
        if self.twitch_client_id.trim().is_empty() { missing.push("Twitch client ID"); }
        if self.twitch_client_secret.trim().is_empty() { missing.push("Twitch client secret"); }
        if self.twitch_channel.trim().is_empty() { missing.push("Twitch channel"); }
        if !missing.is_empty() {
            anyhow::bail!("not configured — set {} on the Settings page", missing.join(", "));
        }
        Ok((&self.twitch_client_id, &self.twitch_client_secret, &self.twitch_channel))
    }

    /// The same values as `.env` key/value pairs.
    fn env_pairs(&self) -> Vec<(String, String)> {
        let v = serde_json::to_value(self).unwrap();
        FIELDS
            .iter()
            .map(|(field, env_key)| {
                let raw = match v.get(*field) {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Bool(b)) => b.to_string(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
                ((*env_key).to_string(), raw)
            })
            .collect()
    }

    /// JSON for the browser, with secrets replaced by MASK when set.
    pub fn to_masked_json(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap();
        let obj = v.as_object_mut().unwrap();
        for key in SECRET_FIELDS {
            if let Some(Value::String(s)) = obj.get(*key) {
                let masked = if s.trim().is_empty() { String::new() } else { MASK.to_string() };
                obj.insert((*key).to_string(), Value::String(masked));
            }
        }
        v
    }

    /// Apply an incoming patch from the UI. Only present keys change, and a secret
    /// left as MASK keeps its stored value.
    pub fn apply_patch(&mut self, patch: &Value) -> Result<()> {
        let Some(patch) = patch.as_object() else {
            anyhow::bail!("settings payload must be an object");
        };
        let mut current = serde_json::to_value(&*self)?;
        let obj = current.as_object_mut().unwrap();
        let defaults = serde_json::to_value(Settings::default())?;

        for (field, _) in FIELDS {
            let Some(incoming) = patch.get(*field) else { continue };
            // Secrets: MASK (or absent) means "unchanged"; empty string clears.
            if SECRET_FIELDS.contains(field) {
                if incoming.as_str() == Some(MASK) {
                    continue;
                }
            }
            let d = defaults.get(*field).unwrap();
            // Tolerate strings for typed fields — HTML inputs always send strings.
            let coerced = match (d, incoming) {
                (_, Value::String(s)) if !matches!(d, Value::String(_)) => coerce(d, s),
                _ => incoming.clone(),
            };
            obj.insert((*field).to_string(), coerced);
        }
        *self = serde_json::from_value(current)?;
        self.normalize();
        Ok(())
    }

    /// Clamp/tidy values so a typo can't produce an unusable ffmpeg command.
    fn normalize(&mut self) {
        let d = Settings::default();
        self.twitch_channel = self.twitch_channel.trim().trim_start_matches('@').to_lowercase();
        if self.out_dir.trim().is_empty() { self.out_dir = d.out_dir; }
        self.days = self.days.clamp(1, 3650);
        self.pad_sec = self.pad_sec.clamp(0.0, 60.0);
        self.max_clip_sec = self.max_clip_sec.clamp(1.0, 600.0);
        self.video_crf = self.video_crf.clamp(0, 51);
        const PRESETS: &[&str] = &[
            "ultrafast", "superfast", "veryfast", "faster", "fast",
            "medium", "slow", "slower", "veryslow",
        ];
        if !PRESETS.contains(&self.video_preset.as_str()) { self.video_preset = d.video_preset; }
        if self.audio_bitrate.trim().is_empty() { self.audio_bitrate = d.audio_bitrate; }
        if !["private", "unlisted", "public"].contains(&self.youtube_privacy.as_str()) {
            self.youtube_privacy = d.youtube_privacy;
        }
        if !["SELF_ONLY", "MUTUAL_FOLLOW_FRIENDS", "PUBLIC_TO_EVERYONE"].contains(&self.tiktok_privacy.as_str()) {
            self.tiktok_privacy = d.tiktok_privacy;
        }
    }
}

/// Coerce a raw string into the JSON type of `default`.
fn coerce(default: &Value, raw: &str) -> Value {
    let t = raw.trim();
    match default {
        Value::Bool(_) => Value::Bool(matches!(t.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")),
        Value::Number(n) if n.is_f64() => t
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| default.clone()),
        Value::Number(_) => t
            .parse::<i64>()
            .ok()
            .map(|i| Value::Number(i.into()))
            .unwrap_or_else(|| default.clone()),
        _ => Value::String(raw.to_string()),
    }
}

// ── config location ─────────────────────────────────────────────────────────

/// Per-user config dir, created on demand. Writable even when the app itself is
/// installed under Program Files.
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("okra-clip-archiver");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Where saves go.
pub fn env_path() -> PathBuf {
    config_dir().join(".env")
}

/// Load every .env we honour, nearest-user-first. dotenvy does not overwrite
/// already-set vars, so the per-user file wins over a shipped/dev one.
pub fn load_dotenv_files() {
    let _ = dotenvy::from_path(env_path());
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let _ = dotenvy::from_path(dir.join(".env"));
        }
    }
    let _ = dotenvy::dotenv(); // working dir
}

/// Persist to the per-user .env, preserving comments, ordering, and unknown keys.
pub fn save_to_env_file(s: &Settings) -> Result<PathBuf> {
    let path = env_path();
    let pairs = s.env_pairs();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let mut out: Vec<String> = Vec::new();
    let mut written: Vec<&str> = Vec::new();

    for line in existing.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push(line.to_string());
            continue;
        }
        match trimmed.split_once('=') {
            Some((key, _)) => {
                let key = key.trim();
                match pairs.iter().find(|(k, _)| k == key) {
                    Some((k, v)) => {
                        out.push(format!("{k}={}", quote(v)));
                        written.push(pairs.iter().find(|(pk, _)| pk == k).map(|(pk, _)| pk.as_str()).unwrap());
                    }
                    None => out.push(line.to_string()), // unknown key — keep as-is
                }
            }
            None => out.push(line.to_string()),
        }
    }

    let missing: Vec<_> = pairs.iter().filter(|(k, _)| !written.contains(&k.as_str())).collect();
    if !missing.is_empty() {
        if out.last().map(|l| !l.trim().is_empty()).unwrap_or(false) {
            out.push(String::new());
        }
        if existing.trim().is_empty() {
            out.push("# okra-clip-archiver settings — managed by the Settings page.".into());
        }
        for (k, v) in missing {
            out.push(format!("{k}={}", quote(v)));
        }
    }

    let mut body = out.join("\n");
    body.push('\n');
    write_atomic(&path, &body)?;
    Ok(path)
}

/// Single-quote values that would otherwise be ambiguous (spaces, '#'), keeping
/// Windows backslashes literal. Single quotes inside are dropped rather than
/// producing an unparseable line.
fn quote(v: &str) -> String {
    if v.is_empty() {
        return String::new();
    }
    if v.contains(' ') || v.contains('#') || v.contains('\n') {
        format!("'{}'", v.replace('\'', "").replace('\n', " "))
    } else {
        v.to_string()
    }
}

fn write_atomic(path: &Path, body: &str) -> Result<()> {
    let tmp = path.with_extension("env.tmp");
    std::fs::write(&tmp, body)?;
    // Windows rename fails if the destination exists.
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ── live store ──────────────────────────────────────────────────────────────

static STORE: RwLock<Option<Settings>> = RwLock::new(None);

pub fn init(s: Settings) {
    *STORE.write().unwrap() = Some(s);
}

/// A snapshot of the current settings.
pub fn get() -> Settings {
    STORE.read().unwrap().clone().unwrap_or_default()
}

/// Apply a patch from the UI, persist it, and return the saved path.
pub fn update(patch: &Value) -> Result<PathBuf> {
    let mut next = get();
    next.apply_patch(patch)?;
    let path = save_to_env_file(&next)?;
    init(next);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_keeps_masked_secrets_and_clears_on_empty() {
        let mut s = Settings { twitch_client_secret: "real-secret".into(), ..Default::default() };
        s.apply_patch(&serde_json::json!({ "twitch_client_secret": MASK })).unwrap();
        assert_eq!(s.twitch_client_secret, "real-secret", "mask must not overwrite");
        s.apply_patch(&serde_json::json!({ "twitch_client_secret": "" })).unwrap();
        assert_eq!(s.twitch_client_secret, "", "empty explicitly clears");
    }

    #[test]
    fn patch_coerces_strings_from_html_inputs() {
        let mut s = Settings::default();
        s.apply_patch(&serde_json::json!({
            "days": "7", "pad_sec": "2.5", "video_crf": "20", "youtube_enabled": "true"
        }))
        .unwrap();
        assert_eq!(s.days, 7);
        assert_eq!(s.pad_sec, 2.5);
        assert_eq!(s.video_crf, 20);
        assert!(s.youtube_enabled);
    }

    #[test]
    fn normalize_clamps_and_rejects_bad_enums() {
        let mut s = Settings::default();
        s.apply_patch(&serde_json::json!({
            "twitch_channel": " @SomeOne ", "video_crf": 999, "video_preset": "bogus",
            "days": 0, "youtube_privacy": "nope", "out_dir": ""
        }))
        .unwrap();
        assert_eq!(s.twitch_channel, "someone", "trimmed, de-@'d, lowercased");
        assert_eq!(s.video_crf, 51);
        assert_eq!(s.video_preset, "medium");
        assert_eq!(s.days, 1);
        assert_eq!(s.youtube_privacy, "private");
        assert_eq!(s.out_dir, "out");
    }

    #[test]
    fn masked_json_hides_only_set_secrets() {
        let s = Settings { twitch_client_secret: "abc".into(), ..Default::default() };
        let v = s.to_masked_json();
        assert_eq!(v["twitch_client_secret"], MASK);
        assert_eq!(v["youtube_refresh_token"], "", "unset stays empty, not masked");
        assert_eq!(v["twitch_channel"], "", "non-secrets are sent in the clear");
    }

    #[test]
    fn every_field_has_an_env_key() {
        let v = serde_json::to_value(Settings::default()).unwrap();
        let n = v.as_object().unwrap().len();
        assert_eq!(n, FIELDS.len(), "FIELDS must cover every Settings field");
        for (field, _) in FIELDS {
            assert!(v.get(*field).is_some(), "unknown field in FIELDS: {field}");
        }
    }

    #[test]
    fn env_pairs_render_bools_and_numbers_plainly() {
        let s = Settings { youtube_enabled: true, days: 14, pad_sec: 3.5, ..Default::default() };
        let pairs = s.env_pairs();
        let get = |k: &str| pairs.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("YOUTUBE_ENABLED"), "true");
        assert_eq!(get("LOOKBACK_DAYS"), "14");
        assert_eq!(get("PAD_SEC"), "3.5");
    }

    #[test]
    fn quoting_protects_paths_with_spaces() {
        assert_eq!(quote(r"C:\Users\you\Videos"), r"C:\Users\you\Videos");
        assert_eq!(quote(r"C:\Users\John Doe\Videos"), r"'C:\Users\John Doe\Videos'");
        assert_eq!(quote(""), "");
    }
}
