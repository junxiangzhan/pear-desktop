// The `audio-url` backend of crossfade (src/plugins/crossfade/index.ts and
// src/plugins/utils/main/youtubei.ts), run in the page with youtubei.js
// (`./youtubei`).
import { handleLocally } from '@/platform/ipc';
import { getInnertube, poTokenFor } from './youtubei';

const LOG_PREFIX = '[pear-tauri]';

handleLocally('audio-url', async (videoId) => {
  try {
    const yt = await getInnertube();
    const poToken = await poTokenFor(yt, videoId as string);
    const info = await yt.getBasicInfo(videoId as string, { client: 'YTMUSIC', po_token: poToken });
    const url = new URL(
      await info.chooseFormat({ type: 'audio', quality: 'best' }).decipher(yt.session.player),
    );
    url.searchParams.set('pot', poToken);
    console.log(LOG_PREFIX, `[crossfade] Stream URL of ${String(videoId)} ready`);
    return url.toString();
  } catch (error) {
    // Like the Electron handler: an empty URL makes crossfade fall back to a hard cut
    console.error(LOG_PREFIX, `[crossfade] Failed to get the stream URL of ${String(videoId)}:`, error);
    return '';
  }
});
