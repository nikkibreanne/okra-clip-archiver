import { useEffect, useState } from 'react';

const LABEL = {
  ready: 'Ready',
  'no-recording': 'No local recording',
  unmappable: 'Not mappable yet',
  skip: 'Skipped',
};

export default function App() {
  const [clips, setClips] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState({}); // id -> 'rendering' | 'done' | 'failed: …'

  async function load() {
    setError(null);
    setClips(null);
    try {
      const res = await fetch('/api/clips');
      if (!res.ok) throw new Error(await res.text());
      setClips(await res.json());
    } catch (e) {
      setError(String(e.message || e));
    }
  }

  useEffect(() => {
    load();
  }, []);

  async function render(id) {
    setBusy((b) => ({ ...b, [id]: 'rendering' }));
    try {
      const res = await fetch('/api/render', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ id }),
      });
      if (!res.ok) throw new Error(await res.text());
      setBusy((b) => ({ ...b, [id]: 'done' }));
    } catch (e) {
      setBusy((b) => ({ ...b, [id]: 'failed: ' + (e.message || e) }));
    }
  }

  const ready = clips?.filter((c) => c.status === 'ready').length ?? 0;

  return (
    <div className="wrap">
      <header>
        <h1>okra-clip-archiver</h1>
        <button onClick={load} disabled={!clips && !error}>Refresh</button>
      </header>

      {error && <p className="error">Couldn’t load clips: {error}</p>}
      {!clips && !error && <p className="muted">Loading clips…</p>}
      {clips && <p className="muted">{clips.length} clips ≤60s · {ready} ready to render</p>}

      <ul className="clips">
        {clips?.map((c) => (
          <li key={c.id} className="clip">
            <div className="meta">
              <a href={c.url} target="_blank" rel="noreferrer">{c.title || c.id}</a>
              <span className="dur">{Math.round(c.duration)}s</span>
              <span className={`badge ${c.status}`}>{LABEL[c.status] || c.status}</span>
            </div>
            {c.status === 'ready' ? (
              <div className="actions">
                <code title={c.out_path}>{c.out_path}</code>
                <button className="render" disabled={busy[c.id] === 'rendering'} onClick={() => render(c.id)}>
                  {busy[c.id] === 'rendering'
                    ? 'Rendering…'
                    : busy[c.id] === 'done'
                    ? '✓ Rendered'
                    : 'Render vertical'}
                </button>
                {typeof busy[c.id] === 'string' && busy[c.id].startsWith('failed') && (
                  <span className="error">{busy[c.id]}</span>
                )}
              </div>
            ) : (
              <div className="actions muted">{c.reason}</div>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
