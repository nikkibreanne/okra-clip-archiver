// Declarative settings schema — the Settings page renders itself from this, so
// adding a knob is one entry here plus one field in src/settings.rs.

export const SECTIONS = [
  {
    id: 'twitch',
    title: 'Twitch — where clips come from',
    blurb:
      'Create an app at dev.twitch.tv/console/apps to get the ID and secret. The channel is the source of clips — set it to whichever channel you are archiving.',
    fields: [
      { key: 'twitch_channel', label: 'Channel login', type: 'text', placeholder: 'e.g. scasplte2', hint: 'The part after twitch.tv/ — no @.' },
      { key: 'twitch_client_id', label: 'Client ID', type: 'text' },
      { key: 'twitch_client_secret', label: 'Client secret', type: 'password' },
    ],
  },
  {
    id: 'files',
    title: 'Recordings & output',
    blurb: 'Clips are matched to the local recording that was rolling at that moment, then re-cut from it at full resolution.',
    fields: [
      {
        key: 'recordings_dir',
        label: 'OBS recordings folder',
        type: 'text',
        placeholder: 'C:\\Users\\you\\Videos',
        hint: 'Keep OBS’s default filename format (%CCYY-%MM-%DD %hh-%mm-%ss) so start times can be read.',
      },
      { key: 'out_dir', label: 'Output folder', type: 'text', placeholder: 'out' },
      { key: 'days', label: 'Look back (days)', type: 'number', min: 1, max: 3650, hint: 'Twitch keeps VODs 14–60 days; clips older than their VOD can’t be aligned.' },
      { key: 'pad_sec', label: 'Padding (seconds)', type: 'number', step: 0.5, min: 0, max: 60, hint: 'Extra time kept on each side of the clip.' },
      { key: 'max_clip_sec', label: 'Max clip length (seconds)', type: 'number', min: 1, max: 600, hint: 'Shorts/TikTok want 60s or less.' },
    ],
  },
  {
    id: 'render',
    title: 'Render quality',
    blurb: 'Applies to every vertical this app cuts. Lower CRF = better quality and bigger files.',
    fields: [
      { key: 'video_crf', label: 'CRF (quality)', type: 'number', min: 0, max: 51, hint: '18 is visually lossless-ish; 23 is a good small-file default.' },
      {
        key: 'video_preset',
        label: 'x264 preset',
        type: 'select',
        options: ['ultrafast', 'superfast', 'veryfast', 'faster', 'fast', 'medium', 'slow', 'slower', 'veryslow'],
        hint: 'Slower = smaller file, longer encode.',
      },
      { key: 'audio_bitrate', label: 'Audio bitrate', type: 'text', placeholder: '160k' },
    ],
  },
  {
    id: 'youtube',
    title: 'YouTube Shorts — upload target',
    blurb:
      'Credentials for the channel you post to (separate from the Twitch source). Create an OAuth client in Google Cloud with the YouTube Data API enabled.',
    pending: 'Credentials are stored now; automatic uploading is not wired up yet.',
    fields: [
      { key: 'youtube_enabled', label: 'Upload to YouTube', type: 'checkbox' },
      { key: 'youtube_channel_id', label: 'Channel ID or handle', type: 'text', placeholder: 'UC… or @handle' },
      { key: 'youtube_client_id', label: 'OAuth client ID', type: 'text' },
      { key: 'youtube_client_secret', label: 'OAuth client secret', type: 'password' },
      { key: 'youtube_refresh_token', label: 'Refresh token', type: 'password', hint: 'From a one-time OAuth consent for the youtube.upload scope.' },
      { key: 'youtube_privacy', label: 'Privacy', type: 'select', options: ['private', 'unlisted', 'public'] },
      { key: 'youtube_title_template', label: 'Title template', type: 'text', hint: 'Placeholders: {title} {channel} {date} {url}' },
      { key: 'youtube_description_template', label: 'Description template', type: 'textarea' },
      { key: 'youtube_tags', label: 'Tags (comma separated)', type: 'text' },
    ],
  },
  {
    id: 'tiktok',
    title: 'TikTok — upload target',
    blurb: 'Uses the Content Posting API. TikTok requires a manual app review before you can post to a real account.',
    pending: 'Credentials are stored now; automatic uploading is not wired up yet.',
    fields: [
      { key: 'tiktok_enabled', label: 'Upload to TikTok', type: 'checkbox' },
      { key: 'tiktok_open_id', label: 'Account open ID', type: 'text' },
      { key: 'tiktok_client_key', label: 'Client key', type: 'text' },
      { key: 'tiktok_client_secret', label: 'Client secret', type: 'password' },
      { key: 'tiktok_refresh_token', label: 'Refresh token', type: 'password' },
      {
        key: 'tiktok_privacy',
        label: 'Privacy',
        type: 'select',
        options: ['SELF_ONLY', 'MUTUAL_FOLLOW_FRIENDS', 'PUBLIC_TO_EVERYONE'],
      },
      { key: 'tiktok_title_template', label: 'Caption template', type: 'text', hint: 'Placeholders: {title} {channel} {date} {url}' },
    ],
  },
  {
    id: 'kennybot',
    title: 'kennyBot integration (optional)',
    blurb: 'Reads the clip-sync “clapperboard” anchors kennyBot writes on !start, for more precise alignment.',
    fields: [{ key: 'firebase_database_url', label: 'Firebase RTDB URL', type: 'text', placeholder: 'https://…firebasedatabase.app' }],
  },
];
