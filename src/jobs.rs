//! Render job queue with live progress, ETA, and cancellation.
//!
//! Renders run ONE at a time (parallel ffmpeg runs just thrash the disk and make
//! every clip slower), so a batch is a queue you can watch and cancel. Progress
//! comes from `ffmpeg -progress pipe:1`, which emits `out_time_us=` and `speed=`
//! as it works: percent = out_time / clip length, and ETA = remaining / speed.

use anyhow::Result;
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::{io::AsyncBufReadExt, sync::oneshot};

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Queued,
    Running,
    Done,
    Error,
    Canceled,
}

#[derive(Serialize, Clone)]
pub struct Job {
    pub id: String,
    pub title: String,
    pub state: State,
    pub percent: f64,
    /// Seconds of wall-clock left, from ffmpeg's reported speed.
    pub eta_sec: Option<f64>,
    /// ffmpeg's encode speed multiplier (e.g. 2.5 = 2.5x realtime).
    pub speed: Option<f64>,
    pub out_path: Option<String>,
    pub error: Option<String>,
    pub queue_position: Option<usize>,
}

struct Registry {
    jobs: HashMap<String, Job>,
    /// Cancel channels for jobs not yet finished.
    cancels: HashMap<String, oneshot::Sender<()>>,
    /// Ids in submission order, so we can report "3rd in line".
    order: Vec<String>,
}

fn registry() -> &'static Mutex<Registry> {
    static R: OnceLock<Mutex<Registry>> = OnceLock::new();
    R.get_or_init(|| {
        Mutex::new(Registry { jobs: HashMap::new(), cancels: HashMap::new(), order: Vec::new() })
    })
}

/// One render at a time.
fn gate() -> &'static tokio::sync::Semaphore {
    static S: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    S.get_or_init(|| tokio::sync::Semaphore::new(1))
}

/// Every job, newest first, with queue positions filled in.
pub fn snapshot() -> Vec<Job> {
    let r = registry().lock().unwrap();
    let mut waiting = 0usize;
    let mut out: Vec<Job> = r
        .order
        .iter()
        .filter_map(|id| r.jobs.get(id).cloned())
        .map(|mut j| {
            if j.state == State::Queued {
                waiting += 1;
                j.queue_position = Some(waiting);
            }
            j
        })
        .collect();
    out.reverse();
    out
}

/// True if this clip already has a job that hasn't finished.
fn is_active(id: &str) -> bool {
    registry()
        .lock()
        .unwrap()
        .jobs
        .get(id)
        .map(|j| matches!(j.state, State::Queued | State::Running))
        .unwrap_or(false)
}

fn update(id: &str, f: impl FnOnce(&mut Job)) {
    if let Some(j) = registry().lock().unwrap().jobs.get_mut(id) {
        f(j);
    }
}

/// Ask a job to stop. A queued job never starts; a running one has its ffmpeg
/// killed and its partial output removed.
pub fn cancel(id: &str) -> Result<()> {
    let tx = {
        let mut r = registry().lock().unwrap();
        match r.jobs.get(id) {
            Some(j) if matches!(j.state, State::Queued | State::Running) => {}
            Some(_) => anyhow::bail!("that render already finished"),
            None => anyhow::bail!("no such render"),
        }
        r.cancels.remove(id)
    };
    if let Some(tx) = tx {
        let _ = tx.send(());
    }
    update(id, |j| {
        if j.state == State::Queued {
            j.state = State::Canceled; // never started — settle it immediately
        }
    });
    Ok(())
}

/// Drop finished jobs from the list.
pub fn clear_finished() {
    let mut r = registry().lock().unwrap();
    let keep: Vec<String> = r
        .order
        .iter()
        .filter(|id| {
            r.jobs
                .get(*id)
                .map(|j| matches!(j.state, State::Queued | State::Running))
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    r.jobs.retain(|id, _| keep.contains(id));
    r.order = keep;
}

/// Queue a render. Returns immediately; watch progress via `snapshot()`.
pub fn enqueue(id: String, title: String, total_sec: f64, cmd: Vec<String>, out_path: String) -> Result<()> {
    if is_active(&id) {
        anyhow::bail!("that clip is already rendering");
    }
    let (tx, rx) = oneshot::channel();
    {
        let mut r = registry().lock().unwrap();
        r.jobs.insert(
            id.clone(),
            Job {
                id: id.clone(),
                title,
                state: State::Queued,
                percent: 0.0,
                eta_sec: None,
                speed: None,
                out_path: Some(out_path.clone()),
                error: None,
                queue_position: None,
            },
        );
        r.cancels.insert(id.clone(), tx);
        r.order.retain(|x| x != &id);
        r.order.push(id.clone());
    }

    tokio::spawn(async move {
        let permit = gate().acquire().await;
        // Cancelled while waiting in line?
        if registry().lock().unwrap().jobs.get(&id).map(|j| j.state == State::Canceled).unwrap_or(true) {
            return;
        }
        update(&id, |j| {
            j.state = State::Running;
            j.queue_position = None;
        });

        let result = run(&id, total_sec, &cmd, rx).await;
        drop(permit);

        match result {
            Ok(true) => update(&id, |j| {
                j.state = State::Done;
                j.percent = 100.0;
                j.eta_sec = Some(0.0);
            }),
            Ok(false) => {
                let _ = std::fs::remove_file(&out_path); // don't leave a truncated file
                update(&id, |j| j.state = State::Canceled);
            }
            Err(e) => update(&id, |j| {
                j.state = State::Error;
                j.error = Some(e.to_string());
            }),
        }
        registry().lock().unwrap().cancels.remove(&id);
    });
    Ok(())
}

/// Run ffmpeg, streaming progress. Ok(false) = cancelled.
async fn run(id: &str, total_sec: f64, cmd: &[String], mut cancel_rx: oneshot::Receiver<()>) -> Result<bool> {
    if let Some(parent) = std::path::Path::new(cmd.last().unwrap()).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // `-progress pipe:1` prints machine-readable progress; `-nostats` silences the
    // human version. Both go before the output path, hence the insert.
    let mut args: Vec<String> = cmd[1..].to_vec();
    let out_arg = args.pop().unwrap();
    args.extend(["-progress".into(), "pipe:1".into(), "-nostats".into()]);
    args.push(out_arg);

    let mut child = tokio::process::Command::new(&cmd[0])
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let job_id = id.to_string();

    // Parse progress on a side task so we never block waiting on the child.
    let reader = tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Some((key, value)) = line.split_once('=') else { continue };
            match key.trim() {
                "out_time_us" | "out_time_ms" => {
                    // ffmpeg's out_time_ms is actually microseconds — both are µs.
                    if let Ok(us) = value.trim().parse::<f64>() {
                        let done = us / 1_000_000.0;
                        let pct = if total_sec > 0.0 { (done / total_sec * 100.0).clamp(0.0, 99.9) } else { 0.0 };
                        update(&job_id, |j| {
                            j.percent = pct;
                            if let Some(sp) = j.speed.filter(|s| *s > 0.0) {
                                j.eta_sec = Some(((total_sec - done).max(0.0) / sp).min(86_400.0));
                            }
                        });
                    }
                }
                "speed" => {
                    if let Ok(sp) = value.trim().trim_end_matches('x').parse::<f64>() {
                        if sp > 0.0 {
                            update(&job_id, |j| j.speed = Some(sp));
                        }
                    }
                }
                _ => {}
            }
        }
    });

    // Keep the tail of stderr so a failure can say why.
    let errbuf = Arc::new(Mutex::new(String::new()));
    let eb = errbuf.clone();
    let errreader = tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let mut b = eb.lock().unwrap();
            *b = line;
        }
    });

    let status = tokio::select! {
        s = child.wait() => s?,
        _ = &mut cancel_rx => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            reader.abort();
            errreader.abort();
            return Ok(false);
        }
    };
    let _ = reader.await;
    let _ = errreader.await;

    if !status.success() {
        let tail = errbuf.lock().unwrap().clone();
        anyhow::bail!(if tail.is_empty() { "ffmpeg failed".into() } else { tail });
    }
    Ok(true)
}
