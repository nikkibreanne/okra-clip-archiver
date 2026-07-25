import { useMemo, useState } from 'react';
import ClipPreview from './ClipPreview.jsx';

const LABEL = {
  ready: 'Ready',
  'no-recording': 'Not mapped',
  unmappable: 'Waiting on VOD',
  skip: 'Too long',
};

const FILTERS = [
  { id: 'all', label: 'All' },
  { id: 'ready', label: 'Ready' },
  { id: 'blocked', label: 'Needs attention' },
];

export default function Clips({ clips, error, loading, onRefresh, onGoSettings, onGoMapping, previewId, onPreview }) {
  const [filter, setFilter] = useState('all');
  const [busy, setBusy] = useState({});

  const counts = useMemo(() => {
    const c = { total: 0, ready: 0, rendered: 0, 'no-recording': 0, unmappable: 0, skip: 0 };
    for (const x of clips || []) {
      c.total += 1;
      c[x.status] = (c[x.status] || 0) + 1;
      if (x.rendered) c.rendered += 1;
    }
    return c;
  }, [clips]);

  const shown = useMemo(() => {
    if (!clips) return [];
    if (filter === 'ready') return clips.filter((c) => c.status === 'ready');
    if (filter === 'blocked') return clips.filter((c) => c.status !== 'ready');
    return clips;
  }, [clips, filter]);

  async function render(id) {
    setBusy((b) => ({ ...b, [id]: { state: 'run' } }));
    try {
      const res = await fetch('/api/render', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ id }),
      });
      const text = await res.text();
      if (!res.ok) throw new Error(text);
      setBusy((b) => ({ ...b, [id]: { state: 'done' } }));
    } catch (e) {
      setBusy((b) => ({ ...b, [id]: { state: 'err', msg: String(e.message || e) } }));
    }
  }

  async function renderAll() {
    for (const c of shown.filter((x) => x.status === 'ready')) {
      // Sequential on purpose — parallel ffmpeg runs would thrash the disk/CPU.
      // eslint-disable-next-line no-await-in-loop
      await render(c.id);
    }
  }

  if (loading) return <div className="panel"><p className="muted">Loading clips from Twitch…</p></div>;

  if (error) {
    const notConfigured = /not configured/i.test(error);
    return (
      <div className="panel empty">
        <h2>{notConfigured ? 'Not configured yet' : 'Couldn’t load clips'}</h2>
        <p className="muted">{error}</p>
        {notConfigured ? (
          <button onClick={onGoSettings}>Open Settings</button>
        ) : (
          <button onClick={onRefresh}>Try again</button>
        )}
      </div>
    );
  }

  if (!clips?.length) {
    return (
      <div className="panel empty">
        <h2>No clips found</h2>
        <p className="muted">Nothing in the look-back window for this channel. Try a longer window in Settings, or make a clip on Twitch.</p>
        <button onClick={onRefresh}>Refresh</button>
      </div>
    );
  }

  const readyCount = shown.filter((c) => c.status === 'ready').length;

  return (
    <>
      <div className="toolbar">
        <div className="chips">
          {FILTERS.map((f) => (
            <button key={f.id} className={`chip ${filter === f.id ? 'on' : ''}`} onClick={() => setFilter(f.id)}>
              {f.label}
              <span className="count">
                {f.id === 'all' ? counts.total : f.id === 'ready' ? counts.ready : counts.total - counts.ready}
              </span>
            </button>
          ))}
        </div>
        <div className="grow" />
        {counts.rendered > 0 && <span className="muted tiny">{counts.rendered} already rendered</span>}
        <button className="ghost" onClick={() => fetch('/api/reveal', { method: 'POST' })}>Open output folder</button>
        <button onClick={renderAll} disabled={!readyCount}>
          Render {readyCount || 'all'} vertical{readyCount === 1 ? '' : 's'}
        </button>
      </div>

      {counts.ready === 0 && (
        <p className="msg warn">
          No clip is mapped to a local recording yet. Check <button className="linkish" onClick={onGoMapping}>VOD mapping</button> to
          pick the right file by hand, and make sure “Store past broadcasts” is on in Twitch so clips carry a VOD offset.
        </p>
      )}

      <ul className="clips">
        {shown.map((c) => {
          const b = busy[c.id];
          return (
            <li key={c.id} className={`clip ${c.status}`}>
              {c.thumbnail_url ? (
                <img className="thumb" src={c.thumbnail_url} alt="" loading="lazy" />
              ) : (
                <div className="thumb placeholder" />
              )}
              <div className="body">
                <div className="row">
                  <a className="title" href={c.url} target="_blank" rel="noreferrer">
                    {c.title || '(untitled clip)'}
                  </a>
                  <span className="dur">{Math.round(c.duration)}s</span>
                  <span className={`badge ${c.status}`}>{LABEL[c.status] || c.status}</span>
                </div>
                <div className="row sub muted tiny">
                  {c.creator && <span>clipped by {c.creator}</span>}
                  {c.created_at && <span>{new Date(c.created_at).toLocaleString()}</span>}
                  {c.rendered && <span className="ok">already rendered</span>}
                </div>
                {c.status === 'ready' ? (
                  <div className="row actions">
                    <code title={c.out_path}>{c.out_path}</code>
                    <button className="small ghost" onClick={() => onPreview(c.id)}>Preview</button>
                    <button className="small" disabled={b?.state === 'run'} onClick={() => render(c.id)}>
                      {b?.state === 'run' ? 'Rendering…' : b?.state === 'done' ? '✓ Rendered' : c.rendered ? 'Re-render' : 'Render vertical'}
                    </button>
                  </div>
                ) : (
                  <div className="row actions">
                    <span className="muted tiny grow">{c.reason}</span>
                    {c.status === 'no-recording' && (
                      <button className="small ghost" onClick={onGoMapping}>Map a recording</button>
                    )}
                  </div>
                )}
                {b?.state === 'err' && <div className="msg err tiny">{b.msg}</div>}
              </div>
            </li>
          );
        })}
      </ul>

      {previewId && clips.some((c) => c.id === previewId) && (
        <ClipPreview
          clip={clips.find((c) => c.id === previewId)}
          busy={busy[previewId]?.state === 'run'}
          onClose={() => onPreview(null)}
          onGoMapping={onGoMapping}
          onRender={render}
        />
      )}
    </>
  );
}
