import { useEffect, useState } from 'react';
import { SECTIONS } from './schema.js';

export default function Settings({ onSaved }) {
  const [values, setValues] = useState(null);
  const [path, setPath] = useState('');
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [note, setNote] = useState(null); // { kind: 'ok'|'err', text }
  const [testing, setTesting] = useState(false);

  async function load() {
    try {
      const res = await fetch('/api/settings');
      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();
      setValues(data.settings);
      setPath(data.path);
      setDirty(false);
    } catch (e) {
      setNote({ kind: 'err', text: String(e.message || e) });
    }
  }

  useEffect(() => {
    load();
  }, []);

  function set(key, v) {
    setValues((s) => ({ ...s, [key]: v }));
    setDirty(true);
    setNote(null);
  }

  async function save() {
    setSaving(true);
    setNote(null);
    try {
      const res = await fetch('/api/settings', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(values),
      });
      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();
      setValues(data.settings);
      setPath(data.path);
      setDirty(false);
      setNote({ kind: 'ok', text: 'Saved — changes are live.' });
      onSaved?.();
    } catch (e) {
      setNote({ kind: 'err', text: String(e.message || e) });
    } finally {
      setSaving(false);
    }
  }

  async function test() {
    setTesting(true);
    setNote(null);
    try {
      const res = await fetch('/api/settings/test', { method: 'POST' });
      const text = await res.text();
      if (!res.ok) throw new Error(text);
      setNote({ kind: 'ok', text: JSON.parse(text).message });
    } catch (e) {
      setNote({ kind: 'err', text: String(e.message || e) });
    } finally {
      setTesting(false);
    }
  }

  if (!values) {
    return (
      <div className="panel">
        {note ? <p className="msg err">{note.text}</p> : <p className="muted">Loading settings…</p>}
      </div>
    );
  }

  return (
    <div className="settings">
      <div className="sticky-bar">
        <div className="grow">
          {note && <span className={`msg ${note.kind === 'ok' ? 'ok' : 'err'}`}>{note.text}</span>}
          {!note && dirty && <span className="muted">Unsaved changes</span>}
          {!note && !dirty && <span className="muted mono tiny">{path}</span>}
        </div>
        <button className="ghost" onClick={test} disabled={testing}>
          {testing ? 'Testing…' : 'Test Twitch connection'}
        </button>
        <button onClick={save} disabled={saving || !dirty}>
          {saving ? 'Saving…' : 'Save settings'}
        </button>
      </div>

      {SECTIONS.map((section) => (
        <section className="panel" key={section.id}>
          <h2>{section.title}</h2>
          {section.blurb && <p className="muted">{section.blurb}</p>}
          {section.pending && <p className="pending">{section.pending}</p>}
          <div className="fields">
            {section.fields.map((f) => (
              <Field key={f.key} f={f} value={values[f.key]} onChange={(v) => set(f.key, v)} />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

function Field({ f, value, onChange }) {
  const id = `f-${f.key}`;
  const common = { id, value: value ?? '', onChange: (e) => onChange(e.target.value) };

  if (f.type === 'checkbox') {
    return (
      <label className="field check" htmlFor={id}>
        <input id={id} type="checkbox" checked={!!value} onChange={(e) => onChange(e.target.checked)} />
        <span>{f.label}</span>
      </label>
    );
  }

  return (
    <div className={`field ${f.type === 'textarea' ? 'wide' : ''}`}>
      <label htmlFor={id}>{f.label}</label>
      {f.type === 'select' ? (
        <select {...common}>
          {f.options.map((o) => (
            <option key={o} value={o}>{o}</option>
          ))}
        </select>
      ) : f.type === 'textarea' ? (
        <textarea {...common} rows={3} placeholder={f.placeholder} />
      ) : (
        <input
          {...common}
          type={f.type === 'password' ? 'password' : f.type === 'number' ? 'number' : 'text'}
          placeholder={f.placeholder}
          min={f.min}
          max={f.max}
          step={f.step}
          autoComplete={f.type === 'password' ? 'new-password' : 'off'}
          spellCheck="false"
        />
      )}
      {f.hint && <span className="hint">{f.hint}</span>}
    </div>
  );
}
