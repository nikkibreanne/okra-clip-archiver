import { useCallback, useEffect, useState } from 'react';
import Clips from './Clips.jsx';
import LayoutEditor from './LayoutEditor.jsx';
import Mapping from './Mapping.jsx';
import Settings from './Settings.jsx';
import Uploads from './Uploads.jsx';

const TABS = [
  { id: 'clips', label: 'Clips' },
  { id: 'mapping', label: 'VOD mapping' },
  { id: 'layout', label: 'Vertical layout' },
  { id: 'uploads', label: 'Uploads' },
  { id: 'settings', label: 'Settings' },
];

// Hash is "tab" or "tab:param" (param currently = the clip open in the preview),
// so any view — including an open preview — is reload-safe and linkable.
const parseHash = () => {
  const [t, ...rest] = decodeURIComponent(window.location.hash.slice(1)).split(':');
  return { tab: TABS.some((x) => x.id === t) ? t : 'clips', param: rest.join(':') || null };
};

export default function App() {
  const [{ tab, param }, setRoute] = useState(parseHash);
  const setTab = useCallback((t, p = null) => {
    const next = p ? `${t}:${p}` : t;
    if (window.location.hash.slice(1) !== next) window.location.hash = next;
    setRoute({ tab: t, param: p });
  }, []);

  useEffect(() => {
    const onHash = () => setRoute(parseHash());
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);

  const [status, setStatus] = useState(null);
  const [clips, setClips] = useState(null);
  const [clipsError, setClipsError] = useState(null);
  const [loading, setLoading] = useState(true);

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await (await fetch('/api/status')).json());
    } catch {
      /* status is advisory */
    }
  }, []);

  const loadClips = useCallback(async () => {
    setLoading(true);
    setClipsError(null);
    try {
      const res = await fetch('/api/clips');
      if (!res.ok) throw new Error(await res.text());
      setClips(await res.json());
    } catch (e) {
      setClips(null);
      setClipsError(String(e.message || e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadStatus();
    loadClips();
  }, [loadStatus, loadClips]);

  const refreshAll = useCallback(() => {
    loadStatus();
    loadClips();
  }, [loadStatus, loadClips]);

  return (
    <div className="wrap">
      <header>
        <div className="brand">
          <h1>okra-clip-archiver</h1>
          {status?.version && <span className="ver">v{status.version}</span>}
        </div>
        <div className="grow" />
        <button className="ghost" onClick={refreshAll} disabled={loading}>
          {loading ? 'Loading…' : 'Refresh'}
        </button>
      </header>

      {status && (
        <div className="statusbar">
          <Chip
            ok={status.configured}
            label={status.configured ? `channel: ${status.channel}` : 'no channel set'}
            onClick={() => setTab('settings')}
          />
          <Chip
            ok={status.recordings_found > 0}
            label={
              status.recordings_dir
                ? `${status.recordings_found} recording${status.recordings_found === 1 ? '' : 's'}`
                : 'no recordings folder'
            }
            onClick={() => setTab('settings')}
          />
          <Chip ok={status.ffmpeg_ok} label={status.ffmpeg_ok ? 'ffmpeg ready' : 'ffmpeg missing'} title={status.ffmpeg} />
          <Chip ok={status.has_layout} label={status.has_layout ? 'custom layout' : 'default layout'} onClick={() => setTab('layout')} />
          <span className="grow" />
          <span className="muted tiny">last {status.days} days → {status.out_dir}</span>
        </div>
      )}

      <nav className="tabs">
        {TABS.map((t) => (
          <button key={t.id} className={`tab ${tab === t.id ? 'on' : ''}`} onClick={() => setTab(t.id)}>
            {t.label}
          </button>
        ))}
      </nav>

      {tab === 'clips' && (
        <Clips
          clips={clips}
          error={clipsError}
          loading={loading}
          onRefresh={refreshAll}
          onGoSettings={() => setTab('settings')}
          onGoMapping={() => setTab('mapping')}
          previewId={param}
          onPreview={(id) => setTab('clips', id)}
        />
      )}
      {tab === 'mapping' && <Mapping onChanged={refreshAll} onGoSettings={() => setTab('settings')} />}
      {tab === 'layout' && <LayoutEditor clips={clips} onGoSettings={() => setTab('settings')} />}
      {tab === 'uploads' && <Uploads status={status} onGoSettings={() => setTab('settings')} />}
      {tab === 'settings' && <Settings onSaved={refreshAll} />}
    </div>
  );
}

function Chip({ ok, label, title, onClick }) {
  return (
    <button className={`stat ${ok ? 'good' : 'bad'}`} title={title} onClick={onClick} disabled={!onClick}>
      <span className="dot" />
      {label}
    </button>
  );
}
