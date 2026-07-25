/**
 * Uploads — deliberately a stub. Rendering verticals works today; posting them
 * does not. This screen documents exactly what each platform will require so the
 * work is understood before it's built, rather than half-wired and misleading.
 */
const PLAN = [
  {
    id: 'youtube',
    name: 'YouTube Shorts',
    difficulty: 'Straightforward',
    steps: [
      'Enable the YouTube Data API v3 in Google Cloud and create an OAuth client (desktop app).',
      'One-time consent in this app for the youtube.upload scope; store the refresh token.',
      'Upload with a resumable videos.insert — vertical + ≤60s is auto-classified as a Short.',
      'Title/description/tags come from the templates on the Settings page.',
    ],
    limits: 'Quota is the real constraint: an upload costs ~1,600 of the default 10,000 units/day, so roughly 6 uploads per day unless you request more.',
  },
  {
    id: 'tiktok',
    name: 'TikTok',
    difficulty: 'Gated on approval',
    steps: [
      'Register the app and request Content Posting API access.',
      'Pass TikTok’s manual app review — required before posting to a real account.',
      'One-time OAuth per account; store the refresh token.',
      'Direct Post the rendered MP4 (≤1GB), or send to inbox for manual publishing.',
    ],
    limits: 'Capped at 25 posts per account per day. Until the review clears, the practical path is rendering to a folder and posting by hand.',
  },
];

export default function Uploads({ status, onGoSettings }) {
  const u = status?.uploads;
  return (
    <>
      <div className="panel">
        <h2>Uploads — not built yet</h2>
        <p className="muted">
          This app currently gets you as far as a finished vertical MP4 in your output folder. Automatic posting is the
          next feature; credentials for both platforms are already configurable on the{' '}
          <button className="linkish" onClick={onGoSettings}>Settings</button> page so nothing has to change when it lands.
        </p>
      </div>

      {PLAN.map((p) => {
        const enabled = u?.[`${p.id}_enabled`];
        const ready = u?.[`${p.id}_ready`];
        return (
          <section className="panel" key={p.id}>
            <div className="row">
              <h3 className="grow">{p.name}</h3>
              <span className="tagpill none">{p.difficulty}</span>
              <span className={`tagpill ${enabled ? 'auto' : 'none'}`}>{enabled ? 'enabled in settings' : 'disabled'}</span>
              <span className={`tagpill ${ready ? 'auto' : 'none'}`}>{ready ? 'token saved' : 'no token'}</span>
            </div>
            <ol className="plan">
              {p.steps.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ol>
            <p className="muted tiny">{p.limits}</p>
          </section>
        );
      })}
    </>
  );
}
