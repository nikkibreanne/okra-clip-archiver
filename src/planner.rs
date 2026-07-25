//! Pure planning: map a Twitch clip to a local cut + an ffmpeg command. No IO,
//! no network, no clock — so it unit-tests cleanly. Ported from the kennyBot JS
//! prototype (which is now removed there; this is the home for it).
//!
//! The mapping: a clip carries `vod_offset` (seconds into the VOD) + `duration`.
//! The VOD is only a TIMELINE RULER (it's ≤1080p); the pixels come from the local
//! recording. wall-clock of the moment = vod_start + vod_offset; local offset =
//! that − recording_start. Using real epochs absorbs the OBS-vs-VOD start gap.

use chrono::{Local, TimeZone};

/// 16:9 centered over a blurred fill → 1080x1920. Needs no crop boxes, so it runs
/// out of the box; swap for a crop+vstack template once the OBS layout is fixed.
pub const DEFAULT_VERTICAL_FILTER: &str = "[0:v]split=2[bg][fg];[bg]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,gblur=sigma=20[bgb];[fg]scale=1080:-1[fgs];[bgb][fgs]overlay=(W-w)/2:(H-h)/2[v]";

#[derive(Debug, Clone)]
pub struct Recording {
    pub path: String,
    pub start_epoch_ms: i64,
    pub duration_sec: Option<f64>,
}

// `title`/`video_id`/`url` are carried for the render/naming/upload stages.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Clip {
    pub id: String,
    pub title: String,
    pub duration_sec: f64,
    pub vod_offset_sec: Option<i64>,
    pub video_id: Option<String>,
    pub created_at_ms: i64,
    pub url: String,
}

#[derive(Debug, PartialEq)]
pub enum Plan {
    Ready { in_sec: f64, out_sec: f64, local_path: String, out_path: String, ffmpeg: Vec<String> },
    Skip(String),
    Unmappable(String),
    NoRecording(String),
}

/// Everything about HOW to cut, from the user's settings.
#[derive(Debug, Clone)]
pub struct RenderOpts {
    pub pad_sec: f64,
    pub max_clip_sec: f64,
    pub filter: String,
    pub out_dir: String,
    pub crf: i64,
    pub preset: String,
    pub audio_bitrate: String,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            pad_sec: 5.0,
            max_clip_sec: 60.0,
            filter: DEFAULT_VERTICAL_FILTER.to_string(),
            out_dir: "out".into(),
            crf: 18,
            preset: "medium".into(),
            audio_bitrate: "160k".into(),
        }
    }
}

/// OBS default filename ("2026-07-24 20-15-30.*") → epoch ms (local), or None.
pub fn parse_obs_filename_epoch(name: &str) -> Option<i64> {
    let re = regex::Regex::new(r"(\d{4})-(\d{2})-(\d{2})[ _T](\d{2})-(\d{2})-(\d{2})").ok()?;
    let c = re.captures(name)?;
    let n = |i: usize| c.get(i).unwrap().as_str().parse::<u32>().ok();
    let dt = Local
        .with_ymd_and_hms(n(1)? as i32, n(2)?, n(3)?, n(4)?, n(5)?, n(6)?)
        .single()?;
    Some(dt.timestamp_millis())
}

/// Pick the recording that was rolling at a wall-clock instant: the latest one
/// that started at or before it (and, if its duration is known, hadn't ended).
pub fn find_recording<'a>(recordings: &'a [Recording], at_epoch_ms: i64) -> Option<&'a Recording> {
    let mut best: Option<&Recording> = None;
    for r in recordings {
        if r.start_epoch_ms <= at_epoch_ms && best.map_or(true, |b| r.start_epoch_ms > b.start_epoch_ms) {
            best = Some(r);
        }
    }
    if let Some(b) = best {
        if let Some(d) = b.duration_sec {
            if at_epoch_ms as f64 > b.start_epoch_ms as f64 + d * 1000.0 {
                return None;
            }
        }
    }
    best
}

/// Where a VOD's t=0 sits inside a local recording — the single thing needed to
/// turn a clip's `vod_offset` into a position in the file. Built either by
/// inference (`anchor_from_inference`) or from a manual mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    pub path: String,
    pub vod_zero_at_sec: f64,
    /// True when the user pinned this by hand rather than us guessing.
    pub manual: bool,
}

/// Infer the anchor from wall-clock: the VOD started `vod_start - rec_start` into
/// the recording.
pub fn anchor_from_inference(recording: &Recording, vod_start_epoch_ms: i64) -> Anchor {
    Anchor {
        path: recording.path.clone(),
        vod_zero_at_sec: (vod_start_epoch_ms - recording.start_epoch_ms) as f64 / 1000.0,
        manual: false,
    }
}

/// Build the extraction plan for one clip.
pub fn plan_clip(clip: &Clip, anchor: Option<&Anchor>, opts: &RenderOpts) -> Plan {
    if clip.duration_sec > opts.max_clip_sec {
        return Plan::Skip(format!("longer than {:.0}s", opts.max_clip_sec));
    }
    let Some(vod_offset) = clip.vod_offset_sec else {
        return Plan::Unmappable("Twitch hasn't published this clip's VOD position yet (it appears minutes later, and needs VODs enabled)".into());
    };
    let Some(anchor) = anchor else {
        return Plan::NoRecording("no local recording is mapped to this VOD".into());
    };

    let moment_local_sec = anchor.vod_zero_at_sec + vod_offset as f64;
    let in_sec = (moment_local_sec - opts.pad_sec).max(0.0);
    let out_sec = moment_local_sec + clip.duration_sec + opts.pad_sec;

    let safe: String = clip.id.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
        .collect();
    let stamp = chrono::DateTime::from_timestamp_millis(clip.created_at_ms)
        .map(|d| d.format("%Y-%m-%d-%H-%M-%S").to_string())
        .unwrap_or_else(|| clip.created_at_ms.to_string());
    let out_path = format!("{}/{stamp}_{safe}.mp4", opts.out_dir.trim_end_matches(['/', '\\']));

    let ffmpeg = vec![
        "ffmpeg".into(), "-y".into(),
        "-ss".into(), format!("{in_sec:.2}"),
        "-i".into(), anchor.path.clone(),
        "-t".into(), format!("{:.2}", out_sec - in_sec),
        "-filter_complex".into(), opts.filter.clone(),
        "-map".into(), "[v]".into(), "-map".into(), "0:a?".into(),
        "-c:v".into(), "libx264".into(), "-crf".into(), opts.crf.to_string(), "-preset".into(), opts.preset.clone(),
        "-c:a".into(), "aac".into(), "-b:a".into(), opts.audio_bitrate.clone(),
        out_path.clone(),
    ];
    Plan::Ready { in_sec, out_sec, local_path: anchor.path.clone(), out_path, ffmpeg }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> i64 { Local.with_ymd_and_hms(2026, 7, 24, 20, 0, 0).single().unwrap().timestamp_millis() }

    #[test]
    fn parses_obs_filename() {
        let e = parse_obs_filename_epoch("2026-07-24 20-15-30.mkv").unwrap();
        assert_eq!(e, Local.with_ymd_and_hms(2026, 7, 24, 20, 15, 30).single().unwrap().timestamp_millis());
        assert_eq!(parse_obs_filename_epoch("not-a-recording.mp4"), None);
    }

    #[test]
    fn finds_the_rolling_recording() {
        let recs = vec![
            Recording { path: "a.mkv".into(), start_epoch_ms: base(), duration_sec: None },
            Recording { path: "b.mkv".into(), start_epoch_ms: base() + 3_600_000, duration_sec: None },
        ];
        assert_eq!(find_recording(&recs, base() + 60_000).unwrap().path, "a.mkv");
        assert_eq!(find_recording(&recs, base() + 3_660_000).unwrap().path, "b.mkv");
        assert!(find_recording(&recs, base() - 1000).is_none());
    }

    #[test]
    fn rejects_a_gap_past_known_duration() {
        let recs = vec![Recording { path: "a.mkv".into(), start_epoch_ms: base(), duration_sec: Some(100.0) }];
        assert_eq!(find_recording(&recs, base() + 50_000).unwrap().path, "a.mkv");
        assert!(find_recording(&recs, base() + 200_000).is_none());
    }

    #[test]
    fn maps_vod_offset_absorbing_start_gap() {
        // OBS started recording 30s BEFORE the VOD; clip is 100s into the VOD, 20s long.
        let rec = Recording { path: "/rec/a.mkv".into(), start_epoch_ms: base() - 30_000, duration_sec: None };
        let anchor = anchor_from_inference(&rec, base());
        assert_eq!(anchor.vod_zero_at_sec, 30.0, "VOD t=0 is 30s into the file");
        let clip = Clip {
            id: "Clip 1!".into(), title: "t".into(), duration_sec: 20.0, vod_offset_sec: Some(100),
            video_id: Some("v1".into()), created_at_ms: base() + 100_000, url: "u".into(),
        };
        match plan_clip(&clip, Some(&anchor), &RenderOpts::default()) {
            Plan::Ready { in_sec, out_sec, ffmpeg, out_path, .. } => {
                assert_eq!(in_sec, 125.0); // 100s into VOD = 130s into local; −5 pad
                assert_eq!(out_sec, 155.0);
                let i = ffmpeg.iter().position(|a| a == "-ss").unwrap();
                assert_eq!(ffmpeg[i + 1], "125.00");
                assert!(ffmpeg.contains(&"/rec/a.mkv".to_string()));
                assert!(out_path.ends_with("_Clip_1_.mp4"));
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn degrades_cleanly() {
        let clip = Clip {
            id: "x".into(), title: "t".into(), duration_sec: 20.0, vod_offset_sec: Some(100),
            video_id: Some("v1".into()), created_at_ms: base(), url: "u".into(),
        };
        let o = RenderOpts::default();
        let anchor = Anchor { path: "/rec/a.mkv".into(), vod_zero_at_sec: 0.0, manual: false };
        let long = Clip { duration_sec: 90.0, ..clip.clone() };
        assert!(matches!(plan_clip(&long, Some(&anchor), &o), Plan::Skip(_)));
        let novod = Clip { vod_offset_sec: None, ..clip.clone() };
        assert!(matches!(plan_clip(&novod, Some(&anchor), &o), Plan::Unmappable(_)));
        assert!(matches!(plan_clip(&clip, None, &o), Plan::NoRecording(_)));
    }

    #[test]
    fn a_manual_anchor_overrides_inference_and_offsets_the_cut() {
        // User pinned the VOD: its t=0 is 12.5s into this file.
        let anchor = Anchor { path: "/rec/manual.mkv".into(), vod_zero_at_sec: 12.5, manual: true };
        let clip = Clip {
            id: "x".into(), title: "t".into(), duration_sec: 10.0, vod_offset_sec: Some(100),
            video_id: Some("v1".into()), created_at_ms: base(), url: "u".into(),
        };
        let opts = RenderOpts { pad_sec: 2.0, ..Default::default() };
        match plan_clip(&clip, Some(&anchor), &opts) {
            Plan::Ready { in_sec, out_sec, local_path, .. } => {
                assert_eq!(in_sec, 110.5); // 12.5 + 100 − 2
                assert_eq!(out_sec, 124.5); // 12.5 + 100 + 10 + 2
                assert_eq!(local_path, "/rec/manual.mkv");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn a_negative_anchor_clamps_the_in_point_to_zero() {
        // Recording started AFTER the VOD — the clip may sit before the file begins.
        let anchor = Anchor { path: "/rec/a.mkv".into(), vod_zero_at_sec: -50.0, manual: true };
        let clip = Clip {
            id: "x".into(), title: "t".into(), duration_sec: 10.0, vod_offset_sec: Some(10),
            video_id: Some("v1".into()), created_at_ms: base(), url: "u".into(),
        };
        match plan_clip(&clip, Some(&anchor), &RenderOpts::default()) {
            Plan::Ready { in_sec, .. } => assert_eq!(in_sec, 0.0, "never seeks before the file start"),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn render_opts_flow_into_the_ffmpeg_command() {
        let rec = Recording { path: "/rec/a.mkv".into(), start_epoch_ms: base(), duration_sec: None };
        let clip = Clip {
            id: "x".into(), title: "t".into(), duration_sec: 10.0, vod_offset_sec: Some(0),
            video_id: Some("v".into()), created_at_ms: base(), url: "u".into(),
        };
        let opts = RenderOpts {
            crf: 23, preset: "veryfast".into(), audio_bitrate: "96k".into(),
            out_dir: "renders/".into(), pad_sec: 0.0, ..Default::default()
        };
        match plan_clip(&clip, Some(&anchor_from_inference(&rec, base())), &opts) {
            Plan::Ready { ffmpeg, out_path, .. } => {
                let at = |flag: &str| ffmpeg[ffmpeg.iter().position(|a| a == flag).unwrap() + 1].clone();
                assert_eq!(at("-crf"), "23");
                assert_eq!(at("-preset"), "veryfast");
                assert_eq!(at("-b:a"), "96k");
                assert!(out_path.starts_with("renders/"), "trailing slash not doubled: {out_path}");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn max_clip_sec_is_configurable() {
        let clip = Clip {
            id: "x".into(), title: "t".into(), duration_sec: 90.0, vod_offset_sec: Some(0),
            video_id: Some("v".into()), created_at_ms: base(), url: "u".into(),
        };
        let rec = Recording { path: "/rec/a.mkv".into(), start_epoch_ms: base(), duration_sec: None };
        let opts = RenderOpts { max_clip_sec: 120.0, ..Default::default() };
        let anchor = anchor_from_inference(&rec, base());
        assert!(matches!(plan_clip(&clip, Some(&anchor), &opts), Plan::Ready { .. }));
    }
}
