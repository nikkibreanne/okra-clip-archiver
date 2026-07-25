import { useEffect, useRef, useState } from 'react';

/**
 * "Sign in with Twitch" using the device code flow: no client secret, no
 * developer console — the user enters a short code on twitch.tv and we poll.
 */
export default function TwitchAuth({ status, onChanged }) {
  const [device, setDevice] = useState(null);
  const [note, setNote] = useState(null);
  const [busy, setBusy] = useState(false);
  const timer = useRef(null);

  useEffect(() => () => clearInterval(timer.current), []);

  async function signIn() {
    setBusy(true);
    setNote(null);
    try {
      const res = await fetch('/api/auth/twitch/start', { method: 'POST' });
      if (!res.ok) throw new Error(await res.text());
      const d = await res.json();
      setDevice(d);
      poll(d);
    } catch (e) {
      setNote({ kind: 'err', text: String(e.message || e) });
      setBusy(false);
    }
  }

  function poll(d) {
    clearInterval(timer.current);
    const deadline = Date.now() + (d.expires_in || 1800) * 1000;
    timer.current = setInterval(async () => {
      if (Date.now() > deadline) {
        clearInterval(timer.current);
        setDevice(null);
        setBusy(false);
        setNote({ kind: 'err', text: 'That code expired — try again.' });
        return;
      }
      try {
        const res = await fetch('/api/auth/twitch/poll', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ device_code: d.device_code }),
        });
        const text = await res.text();
        if (!res.ok) throw new Error(text);
        const data = JSON.parse(text);
        if (data.pending) return; // user hasn't finished yet
        clearInterval(timer.current);
        setDevice(null);
        setBusy(false);
        setNote({ kind: 'ok', text: `Signed in as ${data.login}. Pulling clips from ${data.channel}.` });
        onChanged?.();
      } catch (e) {
        clearInterval(timer.current);
        setDevice(null);
        setBusy(false);
        setNote({ kind: 'err', text: String(e.message || e) });
      }
    }, Math.max(2, d.interval || 5) * 1000);
  }

  async function signOut() {
    await fetch('/api/auth/twitch/signout', { method: 'POST' });
    setNote({ kind: 'ok', text: 'Signed out.' });
    onChanged?.();
  }

  const signedIn = !!status?.signed_in_as;

  return (
    <section className="panel">
      <h2>Twitch account</h2>
      {signedIn ? (
        <>
          <p className="muted">
            Signed in as <b>{status.signed_in_as}</b>. This is only used to read clips — the app never posts to Twitch.
          </p>
          <div className="row" style={{ marginTop: 10 }}>
            <button className="ghost" onClick={signOut}>Sign out</button>
          </div>
        </>
      ) : device ? (
        <div className="device">
          <p className="muted">
            A Twitch page should have opened. Enter this code there to finish signing in:
          </p>
          <div className="usercode" title="Type this at twitch.tv/activate">{device.user_code}</div>
          <p className="muted tiny">
            Didn’t open?{' '}
            <a href={device.verification_uri} target="_blank" rel="noreferrer">Open twitch.tv/activate</a> · waiting for you to approve…
          </p>
        </div>
      ) : (
        <>
          <p className="muted">
            Sign in once so the app can read your clips. No passwords, no developer keys — you’ll enter a short code on
            Twitch’s own site.
          </p>
          {!status?.has_client_id && (
            <p className="msg warn">
              This build has no Twitch application ID baked in, so one-click sign-in is unavailable. Enter a Client ID
              below (from dev.twitch.tv/console/apps) and sign-in will work.
            </p>
          )}
          <div className="row" style={{ marginTop: 10 }}>
            <button onClick={signIn} disabled={busy || !status?.has_client_id}>
              {busy ? 'Starting…' : 'Sign in with Twitch'}
            </button>
          </div>
        </>
      )}
      {note && <p className={`msg ${note.kind === 'ok' ? 'ok' : 'err'}`}>{note.text}</p>}
    </section>
  );
}
