import { useEffect, useState } from 'react';

const eta = (s) => {
  if (s == null) return null;
  const v = Math.max(0, Math.round(s));
  if (v < 60) return `${v}s left`;
  return `${Math.floor(v / 60)}m ${String(v % 60).padStart(2, '0')}s left`;
};

const LABEL = {
  queued: 'Waiting in line',
  running: 'Encoding',
  done: 'Finished',
  error: 'Failed',
  canceled: 'Canceled',
};

const TIP = {
  queued: 'Renders run one at a time so they don’t slow each other down.',
  running: 'ffmpeg is cutting and re-encoding this clip from your local recording.',
  done: 'Written to your output folder.',
  error: 'ffmpeg stopped with an error — the message is shown here.',
  canceled: 'Stopped before finishing; any partial file was removed.',
};

/**
 * Live render queue: progress, ETA, and cancel. Polls only while something is
 * actually running, so an idle app makes no requests.
 */
export default function Jobs({ onFinished }) {
  const [jobs, setJobs] = useState([]);

  useEffect(() => {
    let alive = true;
    let hadActive = false;

    async function tick() {
      try {
        const res = await fetch('/api/jobs');
        const data = await res.json();
        if (!alive) return;
        setJobs(data.jobs || []);
        const active = (data.jobs || []).some((j) => j.state === 'queued' || j.state === 'running');
        if (hadActive && !active) onFinished?.(); // refresh clip list once the queue drains
        hadActive = active;
      } catch {
        /* transient */
      }
    }

    tick();
    const id = setInterval(tick, 800);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [onFinished]);

  if (!jobs.length) return null;

  const active = jobs.filter((j) => j.state === 'queued' || j.state === 'running').length;

  async function cancel(id) {
    await fetch('/api/jobs/cancel', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ id }),
    });
  }

  return (
    <section className="panel jobs">
      <div className="row">
        <h2 className="grow">
          Render queue {active > 0 && <span className="muted tiny">· {active} to go</span>}
        </h2>
        {active === 0 && (
          <button className="ghost small" onClick={() => fetch('/api/jobs', { method: 'DELETE' }).then(() => setJobs([]))}>
            Clear
          </button>
        )}
      </div>

      {jobs.map((j) => (
        <div className={`job ${j.state}`} key={j.id}>
          <div className="row">
            <span className="job-title grow" title={j.out_path || ''}>{j.title}</span>
            <span className="muted tiny" title={TIP[j.state]}>
              {LABEL[j.state]}
              {j.state === 'queued' && j.queue_position ? ` · #${j.queue_position}` : ''}
              {j.state === 'running' && j.speed ? ` · ${j.speed.toFixed(1)}× realtime` : ''}
            </span>
            {j.state === 'running' && <span className="muted tiny">{eta(j.eta_sec) || 'estimating…'}</span>}
            {(j.state === 'queued' || j.state === 'running') && (
              <button className="tiny-btn" onClick={() => cancel(j.id)} title="Stop this render">
                Cancel
              </button>
            )}
          </div>
          {(j.state === 'running' || j.state === 'queued') && (
            <div className="bar" title={`${Math.round(j.percent)}%`}>
              <div className="fill" style={{ width: `${j.state === 'queued' ? 0 : j.percent}%` }} />
            </div>
          )}
          {j.error && <div className="msg err tiny">{j.error}</div>}
        </div>
      ))}
    </section>
  );
}
