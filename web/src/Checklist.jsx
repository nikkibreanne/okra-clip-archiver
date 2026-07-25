/**
 * First-run guidance. Instead of a bare "not configured" error, show exactly
 * which of the four setup steps are done and which are left.
 */
export default function Checklist({ status, onGoSettings }) {
  const steps = [
    {
      done: !!status?.signed_in_as,
      label: status?.signed_in_as ? `Signed in as ${status.signed_in_as}` : 'Sign in with Twitch',
      hint: 'A short code on Twitch’s own site — no passwords or developer keys.',
    },
    {
      done: !!status?.channel,
      label: status?.channel ? `Archiving twitch.tv/${status.channel}` : 'Choose which channel to archive',
      hint: 'Defaults to your own channel once you sign in.',
    },
    {
      done: (status?.recordings_found || 0) > 0,
      label:
        status?.recordings_found > 0
          ? `${status.recordings_found} local recording${status.recordings_found === 1 ? '' : 's'} found`
          : 'Point at your OBS recordings folder',
      hint: 'The high-quality video that clips get re-cut from.',
    },
    {
      done: !!status?.ffmpeg_ok,
      label: status?.ffmpeg_ok ? 'ffmpeg ready' : 'Install ffmpeg',
      hint: 'Bundled with the Windows installer; on Linux/WSL: sudo apt install ffmpeg',
    },
  ];
  const left = steps.filter((s) => !s.done).length;

  return (
    <div className="panel">
      <h2>{left ? 'Let’s get you set up' : 'Setup complete'}</h2>
      <p className="muted">
        {left
          ? 'Twitch clips are capped at your stream quality. This app re-cuts the same moments from your local recording, so they come out as good as what you recorded.'
          : 'Everything is configured — clips should load on the Clips tab.'}
      </p>
      <ol className="checklist">
        {steps.map((s) => (
          <li key={s.label} className={s.done ? 'done' : ''}>
            <span className="tick">{s.done ? '✓' : ''}</span>
            <span>
              <b>{s.label}</b>
              <span className="muted tiny"> — {s.hint}</span>
            </span>
          </li>
        ))}
      </ol>
      {left > 0 && <button onClick={onGoSettings}>Open Settings</button>}
    </div>
  );
}
