import { useEffect, useRef, useState } from 'react';

const OUT_W = 1080;
const OUT_H = 1920;
// Each box fills HALF the 9:16 output, i.e. a 1080x960 slot (9:8). A crop with
// that same shape maps in with no stretching — that's what "lock" enforces.
const HALF_ASPECT = OUT_W / (OUT_H / 2); // 1.125

/**
 * Boxes are stored NORMALIZED (0..1 of the frame), never in screen pixels. That
 * keeps them correct when the window resizes, when the preview is drawn at a
 * different size than it was drawn at last time, and when converting to source
 * pixels on save. Aspect locking has to account for the frame's own aspect:
 * a box that is `w` wide in normalized units is `w * frameW` px wide, so for a
 * 9:8 pixel shape its normalized height is `w * frameAspect / HALF_ASPECT`.
 */
const lockedH = (w, frameAspect) => (w * frameAspect) / HALF_ASPECT;

/** Two distinct, non-overlapping starting boxes that both fit in any frame. */
function defaultBoxes(frameAspect, locked) {
  if (!locked) {
    return { top: { x: 0, y: 0, w: 1, h: 0.5 }, bottom: { x: 0, y: 0.5, w: 1, h: 0.5 } };
  }
  let w = 0.5;
  let h = lockedH(w, frameAspect);
  if (h > 1) {
    h = 1;
    w = h / frameAspect * HALF_ASPECT;
  }
  return { top: { x: 0, y: 0, w, h }, bottom: { x: 1 - w, y: 1 - h, w, h } };
}

export default function LayoutEditor({ clips, onGoSettings }) {
  const imgRef = useRef(null);
  const drag = useRef(null);
  const [display, setDisplay] = useState(null); // rendered size, for px<->normalized
  const [natural, setNatural] = useState(null); // source frame size
  const [boxes, setBoxes] = useState(null); // normalized
  const [saved, setSaved] = useState(null);
  const [frameId, setFrameId] = useState('');
  const [note, setNote] = useState(null);
  const [frameState, setFrameState] = useState('loading'); // loading | ok | error
  const [locked, setLocked] = useState(true);

  const readyClips = (clips || []).filter((c) => c.status === 'ready');
  const frameAspect = natural ? natural.w / natural.h : 16 / 9;

  useEffect(() => {
    fetch('/api/layout').then((r) => r.json()).then(setSaved).catch(() => {});
  }, []);

  // A saved layout is in SOURCE px → normalize it.
  useEffect(() => {
    if (saved?.source_w) {
      const n = (r) => ({
        x: r.x / saved.source_w,
        y: r.y / saved.source_h,
        w: r.w / saved.source_w,
        h: r.h / saved.source_h,
      });
      setBoxes({ top: n(saved.top), bottom: n(saved.bottom) });
    }
  }, [saved]);

  // Track the rendered size so drags convert correctly at any window size.
  useEffect(() => {
    const el = imgRef.current;
    if (!el || frameState !== 'ok') return undefined;
    const measure = () => setDisplay({ w: el.offsetWidth, h: el.offsetHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [frameState]);

  function onImgLoad() {
    const img = imgRef.current;
    if (!img?.naturalWidth) return;
    setNatural({ w: img.naturalWidth, h: img.naturalHeight });
    setDisplay({ w: img.offsetWidth, h: img.offsetHeight });
    setFrameState('ok');
    setBoxes((b) => b || defaultBoxes(img.naturalWidth / img.naturalHeight, locked));
  }

  /** Keep a normalized box inside the frame; when locked, hold the 9:8 shape. */
  function clamp(box, keepAspect = locked) {
    let { x, y, w, h } = box;
    w = Math.min(Math.max(w, 0.05), 1);
    h = Math.min(Math.max(h, 0.05), 1);
    if (keepAspect) {
      h = lockedH(w, frameAspect);
      if (h > 1) {
        h = 1;
        w = (h / frameAspect) * HALF_ASPECT;
      }
    }
    x = Math.min(Math.max(x, 0), 1 - w);
    y = Math.min(Math.max(y, 0), 1 - h);
    return { x, y, w, h };
  }

  function relock(on) {
    setLocked(on);
    if (boxes) {
      const fix = (b) => {
        if (!on) return b;
        let w = b.w;
        let h = lockedH(w, frameAspect);
        if (h > 1) {
          h = 1;
          w = (h / frameAspect) * HALF_ASPECT;
        }
        return clamp({ ...b, w, h }, false);
      };
      setBoxes({ top: fix(boxes.top), bottom: fix(boxes.bottom) });
    }
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
    if (!d || !display) return;
    const dx = (e.clientX - d.startX) / display.w; // normalized deltas
    const dy = (e.clientY - d.startY) / display.h;
    const o = d.orig;
    let next;
    if (d.mode === 'move') {
      next = { ...o, x: o.x + dx, y: o.y + dy };
    } else if (locked) {
      // Locked: either direction grows the box, so a mostly-vertical drag still works.
      const grow = Math.abs(dy * frameAspect) > Math.abs(dx) ? (dy * frameAspect) / HALF_ASPECT : dx;
      next = { ...o, w: o.w + grow };
    } else {
      next = { ...o, w: o.w + dx, h: o.h + dy };
    }
    setBoxes((b) => ({ ...b, [d.which]: clamp(next) }));
  }

  function onUp() {
    drag.current = null;
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
  }

  const nudge = (which, patch) => setBoxes((b) => ({ ...b, [which]: clamp({ ...b[which], ...patch }) }));

  async function save() {
    if (!boxes || !natural) return;
    const px = (r) => ({
      x: r.x * natural.w,
      y: r.y * natural.h,
      w: r.w * natural.w,
      h: r.h * natural.h,
    });
    setNote(null);
    try {
      const res = await fetch('/api/layout', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          source_w: natural.w,
          source_h: natural.h,
          top: px(boxes.top),
          bottom: px(boxes.bottom),
          out_w: OUT_W,
          out_h: OUT_H,
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
    setBoxes(defaultBoxes(frameAspect, locked));
    setNote({ kind: 'ok', text: 'Layout cleared — renders fall back to the widescreen fit.' });
  }

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
  const pct = (v) => `${(v * 100).toFixed(2)}%`;

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
        <label className="lock" title="Keep each box shaped like half of a 9:16 video, so nothing gets stretched when the two are stacked.">
          <input type="checkbox" checked={locked} onChange={(e) => relock(e.target.checked)} />
          <span>Lock to 9:16</span>
        </label>
        <button className="ghost" onClick={reset} title="Forget this layout and go back to the default widescreen fit">Reset</button>
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
        <img ref={imgRef} src={src} alt="" onLoad={onImgLoad} onError={() => setFrameState('error')} />
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
              style={{
                left: pct(boxes[which].x),
                top: pct(boxes[which].y),
                width: pct(boxes[which].w),
                height: pct(boxes[which].h),
              }}
              onPointerDown={(e) => startDrag(e, which, 'move')}
            >
              <span className="box-label">{which}</span>
              <div className="handle" onPointerDown={(e) => startDrag(e, which, 'resize')} />
            </div>
          ))}
      </div>

      {frameState === 'ok' && boxes && natural && (
        <div className="box-numbers">
          {['top', 'bottom'].map((which) => {
            const b = boxes[which];
            return (
              <div className="box-row" key={which}>
                <span className={`tag ${which}`}>{which}</span>
                <span className="mono tiny">
                  {Math.round(b.w * natural.w)}×{Math.round(b.h * natural.h)} at {Math.round(b.x * natural.w)},
                  {Math.round(b.y * natural.h)}
                </span>
                <button className="tiny-btn" onClick={() => nudge(which, { x: 0, w: 1 })} title="Span the full width of the frame">
                  full width
                </button>
                <button
                  className="tiny-btn"
                  onClick={() => nudge(which, { x: (1 - b.w) / 2 })}
                  title="Centre this box horizontally"
                >
                  centre
                </button>
                {!locked && (
                  <button className="tiny-btn" onClick={() => nudge(which, { h: 0.5 })} title="Half the frame height">
                    half height
                  </button>
                )}
              </div>
            );
          })}
          <p className="muted tiny">
            {locked
              ? 'Locked: each box keeps the shape of half a 9:16 video, so the stack is never stretched.'
              : 'Unlocked: boxes can be any shape — they’ll be squeezed to fit their half of the output.'}
          </p>
        </div>
      )}
    </section>
  );
}
