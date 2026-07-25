import { useEffect, useState } from 'react';

const fmt = (ms) => (ms ? new Date(ms).toLocaleString() : '—');
const base = (p) => (p ? p.split(/[\\/]/).pop() : '');

/**
 * VOD → local-recording mapping. We normally infer this from timestamps; this
 * screen lets you override it with radio buttons when the guess is wrong or
 * impossible (renamed files, clock drift, expired VOD).
 */
export default function Mapping({ onChanged, onGoSettings }) {
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const [saving, setSaving] = useState('');
  const [note, setNote] = useState(null);

  async function load() {
    setError(null);
    try {
      const res = await fetch('/api/vods');
      if (!res.ok) throw new Error(await res.text());
      setData(await res.json());
    } catch (e) {
      setError(String(e.message || e));
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function save(video_id, recording_path, vod_zero_at_sec) {
    setSaving(video_id);
    setNote(null);
    try {
      const res = await fetch('/api/mapping', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ video_id, recording_path, vod_zero_at_sec }),
      });
      if (!res.ok) throw new Error(await res.text());
      await load();
      onChanged?.();
      setNote({ kind: 'ok', text: recording_path ? 'Mapping saved.' : 'Back to automatic matching.' });
    } catch (e) {
      setNote({ kind: 'err', text: String(e.message || e) });
    } finally {
      setSaving('');
    }
  }

  if (error) {
    return (
      <div className="panel empty">
        <h2>Couldn’t load VODs</h2>
        <p className="muted">{error}</p>
        <button onClick={/not configured/i.test(error) ? onGoSettings : load}>
          {/not configured/i.test(error) ? 'Open Settings' : 'Try again'}
        </button>
      </div>
    );
  }
  if (!data) return <div className="panel"><p className="muted">Loading VODs…</p></div>;

  const { vods, recordings } = data;

  if (!vods.length) {
    return (
      <div className="panel empty">
        <h2>No VODs yet</h2>
        <p className="muted">Clips carry the broadcast they came from. Once some clips load, their VODs show up here for mapping.</p>
      </div>
    );
  }

  return (
    <>
      <div className="panel">
        <h2>VOD → recording mapping</h2>
        <p className="muted">
          Each Twitch broadcast is matched to the local recording that was rolling at the time. If the guess is wrong —
          or the VOD expired so there’s nothing to compare — pick the right file yourself. <b>Offset</b> is where the
          broadcast’s 0:00 sits inside that file; nudge it if cuts land early or late.
        </p>
        {!recordings.length && (
          <p className="msg warn">
            No recordings found. Set your recordings folder in <button className="linkish" onClick={onGoSettings}>Settings</button>.
          </p>
        )}
        {note && <p className={`msg ${note.kind === 'ok' ? 'ok' : 'err'}`}>{note.text}</p>}
      </div>

      {vods.map((v) => (
        <VodCard
          key={v.video_id}
          vod={v}
          recordings={recordings}
          busy={saving === v.video_id}
          onSave={save}
        />
      ))}
    </>
  );
}

function VodCard({ vod, recordings, busy, onSave }) {
  const [offset, setOffset] = useState(String(Math.round(vod.vod_zero_at_sec)));

  useEffect(() => {
    setOffset(String(Math.round(vod.vod_zero_at_sec)));
  }, [vod.vod_zero_at_sec]);

  // The radio reflects the USER's choice: only a manual pin selects a file row.
  // An auto-match keeps "Automatic" selected (and names the file it found).
  const selected = vod.manual ? vod.mapped_path || '' : '';
  const offsetTarget = vod.mapped_path; // apply-offset pins whatever is in effect

  return (
    <section className={`panel vod ${vod.mapped_path ? '' : 'unmapped'}`}>
      <div className="vod-head">
        <div>
          <h3>
            VOD {vod.video_id}
            {vod.manual ? <span className="tagpill manual">manual</span> : vod.mapped_path ? <span className="tagpill auto">auto-matched</span> : <span className="tagpill none">unmapped</span>}
          </h3>
          <p className="muted tiny">
            {vod.clip_count} clip{vod.clip_count === 1 ? '' : 's'} · first clip {fmt(Date.parse(vod.first_clip_at))} ·{' '}
            {vod.vod_start_ms ? `broadcast started ${fmt(vod.vod_start_ms)}` : 'VOD expired or unavailable on Twitch'}
          </p>
        </div>
      </div>

      <div className="choices">
        <label className={`choice ${!selected ? 'on' : ''}`}>
          <input
            type="radio"
            name={`map-${vod.video_id}`}
            checked={!selected}
            disabled={busy}
            onChange={() => onSave(vod.video_id, null, 0)}
          />
          <span className="choice-body">
            <span className="choice-title">Automatic{vod.inferred_path ? ` — ${base(vod.inferred_path)}` : ' (no match found)'}</span>
            <span className="muted tiny">
              {vod.inferred_path ? 'Matched by comparing the broadcast start to recording start times.' : 'No recording overlaps this broadcast.'}
            </span>
          </span>
        </label>

        {recordings.map((r) => (
          <label key={r.path} className={`choice ${selected === r.path ? 'on' : ''}`}>
            <input
              type="radio"
              name={`map-${vod.video_id}`}
              checked={selected === r.path}
              disabled={busy}
              onChange={() => onSave(vod.video_id, r.path, Number(offset) || 0)}
            />
            <span className="choice-body">
              <span className="choice-title">{r.name}</span>
              <span className="muted tiny">
                starts {fmt(r.start_epoch_ms)} · {r.size_mb} MB
              </span>
            </span>
          </label>
        ))}
      </div>

      {offsetTarget && (
        <div className="offset-row">
          <label htmlFor={`off-${vod.video_id}`}>Broadcast 0:00 is at</label>
          <input
            id={`off-${vod.video_id}`}
            type="number"
            value={offset}
            step="1"
            disabled={busy}
            onChange={(e) => setOffset(e.target.value)}
          />
          <span className="muted tiny">seconds into the file</span>
          <button className="small" disabled={busy} onClick={() => onSave(vod.video_id, offsetTarget, Number(offset) || 0)}>
            {busy ? 'Saving…' : 'Apply offset'}
          </button>
          <span className="muted tiny">Cuts landing late? Decrease. Landing early? Increase.</span>
        </div>
      )}
    </section>
  );
}
