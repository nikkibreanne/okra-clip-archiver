import { useEffect, useRef, useState } from 'react';

const OUT_W = 1080;
const OUT_H = 1920;

// Default: top box = upper half, bottom box = lower half (display pixels).
function defaultBoxes(w, h) {
  return {
    top: { x: 0, y: 0, w, h: h / 2 },
    bottom: { x: 0, y: h / 2, w, h: h / 2 },
  };
}

export default function BoxEditor() {
  const imgRef = useRef(null);
  const drag = useRef(null); // { which, mode, startX, startY, orig }
  const [display, setDisplay] = useState(null); // shown image size (px)
  const [natural, setNatural] = useState(null); // source frame size (px)
  const [boxes, setBoxes] = useState(null); // display-pixel rects
  const [saved, setSaved] = useState(null); // persisted layout (source rects)
  const [status, setStatus] = useState('');
  const [err, setErr] = useState(null);

  useEffect(() => {
    fetch('/api/layout').then((r) => r.json()).then(setSaved).catch(() => {});
  }, []);

  // Apply a saved layout once we know the display scale (handles either load order).
  useEffect(() => {
    if (saved && saved.source_w && display) {
      const sx = display.w / saved.source_w;
      const sy = display.h / saved.source_h;
      const conv = (r) => ({ x: r.x * sx, y: r.y * sy, w: r.w * sx, h: r.h * sy });
      setBoxes({ top: conv(saved.top), bottom: conv(saved.bottom) });
    }
  }, [saved, display]);

  function onImgLoad() {
    const img = imgRef.current;
    const d = { w: img.offsetWidth, h: img.offsetHeight };
    setDisplay(d);
    setNatural({ w: img.naturalWidth, h: img.naturalHeight });
    setBoxes((b) => b || defaultBoxes(d.w, d.h));
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
    const next =
      d.mode === 'move'
        ? { x: o.x + dx, y: o.y + dy, w: o.w, h: o.h }
        : { x: o.x, y: o.y, w: o.w + dx, h: o.h + dy };
    setBoxes((b) => ({ ...b, [d.which]: clamp(next) }));
  }
  function onUp() {
    drag.current = null;
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
  }

  async function save() {
    if (!boxes || !natural || !display) return;
    const sx = natural.w / display.w;
    const sy = natural.h / display.h;
    const conv = (r) => ({ x: r.x * sx, y: r.y * sy, w: r.w * sx, h: r.h * sy });
    const payload = {
      source_w: natural.w,
      source_h: natural.h,
      top: conv(boxes.top),
      bottom: conv(boxes.bottom),
      out_w: OUT_W,
      out_h: OUT_H,
    };
    setStatus('saving…');
    setErr(null);
    try {
      const res = await fetch('/api/layout', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(payload),
      });
      if (!res.ok) throw new Error(await res.text());
      setStatus('saved ✓ — new renders will use these boxes');
    } catch (e) {
      setStatus('');
      setErr(String(e.message || e));
    }
  }

  return (
    <section className="editor">
      <div className="editor-head">
        <h2>Vertical layout — pick two boxes</h2>
        <button onClick={save} disabled={!boxes}>Save layout</button>
        {status && <span className="muted">{status}</span>}
      </div>
      <p className="muted">
        Drag each box to move, drag its corner to resize. The <b>top</b> box fills the upper half of the
        9:16 and the <b>bottom</b> box the lower half. Set once — every render uses it.
      </p>
      {err && <p className="error">{err}</p>}
      <div className="stage">
        <img
          ref={imgRef}
          src="/api/frame"
          alt="preview frame"
          onLoad={onImgLoad}
          onError={() => setErr('No preview frame — make sure a clip is “ready” (pass --recordings).')}
        />
        {boxes &&
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
    </section>
  );
}
