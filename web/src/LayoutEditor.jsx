import { useEffect, useRef, useState } from 'react';

const OUT_W = 1080;
const OUT_H = 1920;

function defaultBoxes(w, h) {
  return { top: { x: 0, y: 0, w, h: h / 2 }, bottom: { x: 0, y: h / 2, w, h: h / 2 } };
}

/**
 * Two-box vertical layout editor. Boxes are kept in DISPLAY pixels while editing
 * and converted to SOURCE pixels on save, so the same layout works regardless of
 * how large the preview is drawn.
 */
export default function LayoutEditor({ clips, onGoSettings }) {
  const imgRef = useRef(null);
  const drag = useRef(null);
  const [display, setDisplay] = useState(null);
  const [natural, setNatural] = useState(null);
  const [boxes, setBoxes] = useState(null);
  const [saved, setSaved] = useState(null);
  const [frameId, setFrameId] = useState('');
  const [note, setNote] = useState(null);
  const [frameState, setFrameState] = useState('loading'); // loading | ok | error

  const readyClips = (clips || []).filter((c) => c.status === 'ready');

  useEffect(() => {
    fetch('/api/layout')
      .then((r) => r.json())
      .then(setSaved)
      .catch(() => {});
  }, []);

  // Apply a saved layout once the display scale is known (either load order).
  useEffect(() => {
    if (saved?.source_w && display) {
      const sx = display.w / saved.source_w;
      const sy = display.h / saved.source_h;
      const conv = (r) => ({ x: r.x * sx, y: r.y * sy, w: r.w * sx, h: r.h * sy });
      setBoxes({ top: conv(saved.top), bottom: conv(saved.bottom) });
    }
  }, [saved, display]);

  function onImgLoad() {
    const img = imgRef.current;
    if (!img?.naturalWidth) return;
    const d = { w: img.offsetWidth, h: img.offsetHeight };
    setDisplay(d);
    setNatural({ w: img.naturalWidth, h: img.naturalHeight });
    setBoxes((b) => b || defaultBoxes(d.w, d.h));
    setFrameState('ok');
  }

  function clamp(box) {
    if (!display) return box;
    let { x, y, w, h } = box;
    w = Math.max(24, Math.min(w, display.w));
    h = Math.max(24, Math.min(h, display.h));
    x = Math.max(0, Math.min(x, display.w - w));
    y = Math.max(0, Math.min(y, display.h - h));
    return { x, y, w, h };
  }

  function startDrag(e, which, mode) {
    e.preventDefault();
    e.stopPropagation();
    drag.current = { which, mode, startX: e.clientX, startY: e.clientY, orig: boxes[which] };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  }
  function onMove(e) {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.startX;
    const dy = e.clientY - d.startY;
    const o = d.orig;
    const next = d.mode === 'move'
      ? { x: o.x + dx, y: o.y + dy, w: o.w, h: o.h }
      : { x: o.x, y: o.y, w: o.w + dx, h: o.h + dy };
    setBoxes((b) => ({ ...b, [d.which]: clamp(next) }));
  }
  function onUp() {
    drag.current = null;
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
  }

  function nudge(which, patch) {
    setBoxes((b) => ({ ...b, [which]: clamp({ ...b[which], ...patch }) }));
  }

  async function save() {
    if (!boxes || !natural || !display) return;
    const sx = natural.w / display.w;
    const sy = natural.h / display.h;
    const conv = (r) => ({ x: r.x * sx, y: r.y * sy, w: r.w * sx, h: r.h * sy });
    setNote(null);
    try {
      const res = await fetch('/api/layout', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          source_w: natural.w, source_h: natural.h,
          top: conv(boxes.top), bottom: conv(boxes.bottom),
          out_w: OUT_W, out_h: OUT_H,
        }),
      });
      if (!res.ok) throw new Error(await res.text());
      setNote({ kind: 'ok', text: 'Saved — every render now stacks these two boxes.' });
    } catch (e) {
      setNote({ kind: 'err', text: String(e.message || e) });
    }
  }

  async function reset() {
    await fetch('/api/layout', { method: 'DELETE' }).catch(() => {});
    setSaved(null);
    setBoxes(display ? defaultBoxes(display.w, display.h) : null);
    setNote({ kind: 'ok', text: 'Layout cleared — renders fall back to the widescreen fit.' });
  }

  // No mappable clip → explain instead of showing a broken image.
  if (!readyClips.length) {
    return (
      <div className="panel empty">
        <h2>Vertical layout</h2>
        <p className="muted">
          The editor needs one clip that maps to a local recording, so it can show you a real frame to draw on.
          Set your <b>recordings folder</b> in Settings and make sure Twitch “Store past broadcasts” is on.
        </p>
        <button onClick={onGoSettings}>Open Settings</button>
      </div>
    );
  }

  const src = `/api/frame${frameId ? `?id=${encodeURIComponent(frameId)}` : ''}`;

  return (
    <section className="panel">
      <div className="editor-head">
        <div className="grow">
          <h2>Vertical layout — pick two boxes</h2>
          <p className="muted">
            Drag a box to move it, drag its corner to resize. The <b>top</b> box fills the upper half of the 1080×1920
            output, the <b>bottom</b> box the lower half. Set it once — every render reuses it.
          </p>
        </div>
        <button className="ghost" onClick={reset}>Reset</button>
        <button onClick={save} disabled={frameState !== 'ok'}>Save layout</button>
      </div>

      {note && <p className={`msg ${note.kind === 'ok' ? 'ok' : 'err'}`}>{note.text}</p>}

      <div className="frame-picker">
        <label htmlFor="frame-clip">Preview frame from</label>
        <select
          id="frame-clip"
          value={frameId}
          onChange={(e) => {
            setFrameId(e.target.value);
            setFrameState('loading');
          }}
        >
          <option value="">first mappable clip</option>
          {readyClips.map((c) => (
            <option key={c.id} value={c.id}>{c.title || '(untitled)'}</option>
          ))}
        </select>
        {frameState === 'loading' && <span className="muted tiny">extracting frame…</span>}
      </div>

      <div className="stage">
        <img
          ref={imgRef}
          src={src}
          alt=""
          className={frameState === 'ok' ? '' : 'hidden'}
          onLoad={onImgLoad}
          onError={() => setFrameState('error')}
        />
        {frameState === 'error' && (
          <div className="frame-fallback">
            Couldn’t extract a preview frame. Check that ffmpeg is available and the recording file is readable.
          </div>
        )}
        {frameState === 'ok' &&
          boxes &&
          ['top', 'bottom'].map((which) => (
            <div
              key={which}
              className={`box ${which}`}
              style={{ left: boxes[which].x, top: boxes[which].y, width: boxes[which].w, height: boxes[which].h }}
              onPointerDown={(e) => startDrag(e, which, 'move')}
            >
              <span className="box-label">{which}</span>
              <div className="handle" onPointerDown={(e) => startDrag(e, which, 'resize')} />
            </div>
          ))}
      </div>

      {frameState === 'ok' && boxes && natural && display && (
        <div className="box-numbers">
          {['top', 'bottom'].map((which) => {
            const sx = natural.w / display.w;
            const sy = natural.h / display.h;
            const b = boxes[which];
            return (
              <div className="box-row" key={which}>
                <span className={`tag ${which}`}>{which}</span>
                <span className="mono tiny">
                  {Math.round(b.w * sx)}×{Math.round(b.h * sy)} at {Math.round(b.x * sx)},{Math.round(b.y * sy)} (source px)
                </span>
                <button className="tiny-btn" onClick={() => nudge(which, { x: 0, w: display.w })}>full width</button>
                <button className="tiny-btn" onClick={() => nudge(which, { h: display.h / 2 })}>half height</button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
