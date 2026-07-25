import { useEffect, useState } from 'react';

const t = (s) => {
  const v = Math.max(0, Math.round(s || 0));
  return `${Math.floor(v / 60)}:${String(v % 60).padStart(2, '0')}`;
};

/**
 * Twitch-clip-editor-style detail: play the window as it will be cut from the
 * LOCAL recording. This is the fastest way to confirm a VOD→recording mapping is
 * right before spending an encode on it.
 *
 * The player is always mounted and the "building…" note is an overlay that clears
 * on the first media event OR a timeout — never hide the <video> behind a state
 * flag, or a media event that doesn't fire leaves the user staring at a box.
 */
export default function ClipPreview({ clip, onClose, onRender, onGoMapping, busy }) {
  const [state, setState] = useState('loading');

  useEffect(() => {
    const id = setTimeout(() => setState((s) => (s === 'loading' ? 'ok' : s)), 12000);
    return () => clearTimeout(id);
  }, []);

  // Esc closes, like any modal.
  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div className="preview-overlay" onClick={onClose}>
      <div className="preview" onClick={(e) => e.stopPropagation()}>
        <div className="preview-head">
          <div className="grow">
            <h2>{clip.title || '(untitled clip)'}</h2>
            <p className="muted tiny">
              {Math.round(clip.duration)}s · cut {t(clip.in_sec)}–{t(clip.out_sec)} from {clip.local_path?.split(/[\\/]/).pop()}
              {clip.anchor_source === 'manual' && <span className="tagpill manual">manual mapping</span>}
              {clip.anchor_source === 'inferred' && <span className="tagpill auto">auto-matched</span>}
            </p>
          </div>
          <button className="ghost" onClick={onClose}>Close</button>
        </div>

        <div className="player">
          {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
          <video
            src={`/api/preview?id=${encodeURIComponent(clip.id)}`}
            controls
            autoPlay
            preload="auto"
            onLoadedMetadata={() => setState('ok')}
            onCanPlay={() => setState('ok')}
            onError={() => setState('error')}
          />
          {state !== 'ok' && (
            <div className={`player-note ${state === 'error' ? 'err' : ''}`}>
              {state === 'error'
                ? 'Couldn’t build a preview — the mapping may point at the wrong file, or ffmpeg couldn’t read it.'
                : 'Cutting the preview from your recording…'}
            </div>
          )}
        </div>

        <div className="preview-foot">
          <span className="muted tiny">Wrong moment? The VOD→recording mapping or its offset needs a nudge.</span>
          <button className="ghost" onClick={onGoMapping}>Fix mapping</button>
          <button disabled={busy} onClick={() => onRender(clip.id)}>
            {busy ? 'Rendering…' : 'Render vertical'}
          </button>
        </div>
      </div>
    </div>
  );
}
