// The downloader backend (src/plugins/downloader/main/index.ts), split in
// two: the page finds the stream and the metadata with youtubei.js
// (`./youtubei`, it needs BotGuard), Rust downloads, tags and saves the file
// (`src-tauri/src/plugins/downloader.rs`). There is no ffmpeg, so files keep
// YouTube's own audio format (AAC in MP4, `.m4a`) whatever preset is chosen.
import { YTNodes, type YT, type YTMusic } from 'youtubei.js';

import { t } from '@/i18n';
import { getSongInfo } from '@/providers/song-info-front';

import { emitLocal, handleLocally, ipcRenderer } from './ipc';
import { getInnertube, poTokenFor } from './youtubei';

interface SongOptions {
  /** A folder chosen for this download (download on finish) */
  folder?: string;
  /** A playlist's folder, inside the download folder */
  subfolder?: string;
  track?: number;
  /** No button feedback or progress bar: part of a playlist, or in the background */
  quiet?: boolean;
  onProgress?: (ratio: number) => void;
}

const progressListeners = new Map<number, (ratio: number) => void>();
let nextDownloadId = 1;

ipcRenderer.on('downloader:progress', (_, id, ratio) =>
  progressListeners.get(id as number)?.(ratio as number),
);

const setProgressBar = (value: number) => ipcRenderer.send('downloader:progress-bar', value);

const feedback = (message?: string) => emitLocal('downloader-feedback', message);

const showDialog = (title: string, message: string, detail: string, error = false) => {
  ipcRenderer.invoke('downloader:dialog', { title, message, detail, error }).catch(console.error);
};

// Like `sendError`
const sendError = (error: unknown, source?: string) => {
  setProgressBar(-1);
  feedback();
  const songNameMessage = source ? `\nin ${source}` : '';
  const cause =
    // oxlint-disable-next-line typescript/no-base-to-string
    error instanceof Error && error.cause ? `\n\n${String(error.cause)}` : '';
  const message = `${String(error)}${songNameMessage}${cause}`;
  console.error(message);
  showDialog(
    t('plugins.downloader.backend.dialog.error.title'),
    t('plugins.downloader.backend.dialog.error.message'),
    message,
    true,
  );
};

const getVideoId = (url: string) => URL.parse(url)?.searchParams.get('v') ?? null;

// Playlist radio modifier needs to be cut from playlist ID
const INVALID_PLAYLIST_MODIFIER = 'RDAMPL';

const getPlaylistId = (url?: URL | null) => {
  const result = url?.searchParams.get('list') || url?.searchParams.get('playlist');
  return result?.startsWith(INVALID_PLAYLIST_MODIFIER)
    ? result.slice(INVALID_PLAYLIST_MODIFIER.length)
    : result;
};

const playabilityError = (info: YTMusic.TrackInfo | YT.VideoInfo) => {
  const status = info.playability_status;
  if (status?.status === 'LOGIN_REQUIRED') {
    return new Error(`[${status.status}] ${status.reason}`);
  }
  if (status?.status === 'UNPLAYABLE') {
    const errorScreen = status.error_screen as YTNodes.PlayerErrorMessage | null;
    return new Error(
      `[${status.status}] ${errorScreen?.reason.text}: ${errorScreen?.subreason.text}`,
    );
  }
  return null;
};

const downloadSongUnsafe = async (
  videoId: string,
  options: SongOptions,
  setName: (name: string) => void,
) => {
  const yt = await getInnertube();
  const poToken = await poTokenFor(yt, videoId);
  const track = await yt.music.getInfo(videoId, { po_token: poToken });
  const basic = track.basic_info;
  const title = (basic.title ?? '').normalize('NFC');
  const artist = (basic.author ?? '').normalize('NFC');
  setName(artist ? `${artist} - ${title}` : title);
  const album = track.player_overlays?.browser_media_session
    ?.as(YTNodes.BrowserMediaSession)
    .album?.text?.normalize('NFC');
  const cover = basic.thumbnail?.find((it) => !it.url.endsWith('.webp'))?.url;

  // Like Electron: try to bypass the age restriction with the TV client
  let info: YTMusic.TrackInfo | YT.VideoInfo = track;
  if (info.playability_status?.status === 'LOGIN_REQUIRED') {
    info = await yt.getBasicInfo(videoId, { client: 'TV_EMBEDDED', po_token: poToken });
  }
  const error = playabilityError(info);
  if (error) throw error;

  // AAC in MP4: the audio format tags can be written to without remuxing
  const format = info.chooseFormat({ type: 'audio', quality: 'best', format: 'mp4' });
  const url = new URL(await format.decipher(yt.session.player));
  url.searchParams.set('pot', poToken);

  console.info(
    t('plugins.downloader.backend.feedback.download-info', { artist, title, videoId }),
  );
  const id = nextDownloadId++;
  progressListeners.set(id, (ratio) => {
    options.onProgress?.(ratio);
    if (options.quiet) return;
    feedback(
      t('plugins.downloader.backend.feedback.download-progress', {
        percent: Math.floor(ratio * 100),
      }),
    );
    setProgressBar(ratio);
  });
  try {
    const result = (await ipcRenderer.invoke('downloader:save', {
      id,
      url: url.toString(),
      contentLength: format.content_length,
      title,
      artist,
      album: album && album !== 'N/A' ? album : undefined,
      track: options.track,
      cover,
      folder: options.folder,
      subfolder: options.subfolder,
    })) as { path: string; skipped?: boolean };
    if (!result.skipped) {
      console.info(t('plugins.downloader.backend.feedback.done', { filePath: result.path }));
    }
  } finally {
    progressListeners.delete(id);
  }
};

const downloadSong = async (videoId: string | null, source: string, options: SongOptions = {}) => {
  let name: string | undefined;
  try {
    if (!options.quiet) {
      feedback(t('plugins.downloader.backend.feedback.downloading'));
      setProgressBar(2);
    }
    if (!videoId) throw new Error(t('plugins.downloader.backend.feedback.video-id-not-found'));
    await downloadSongUnsafe(videoId, options, (it) => (name = it));
    if (!options.quiet) {
      feedback();
      setProgressBar(-1);
    }
  } catch (error) {
    sendError(error, name ?? source);
  }
};

export const downloadPlaylist = async (givenUrl?: string | URL) => {
  const url = URL.parse(givenUrl ?? '') ?? new URL(location.href);
  const playingUrl = getSongInfo().url;
  const playlistId = getPlaylistId(url) || getPlaylistId(playingUrl ? URL.parse(playingUrl) : null);
  if (!playlistId) {
    sendError(new Error(t('plugins.downloader.backend.feedback.playlist-id-not-found')));
    return;
  }

  console.log(t('plugins.downloader.backend.feedback.trying-to-get-playlist-id', { playlistId }));
  feedback(t('plugins.downloader.backend.feedback.getting-playlist-info'));
  const items: YTNodes.MusicResponsiveListItem[] = [];
  const collect = (playlist: YTMusic.Playlist) =>
    items.push(
      ...(playlist.items ?? []).filter(
        (item): item is YTNodes.MusicResponsiveListItem =>
          item instanceof YTNodes.MusicResponsiveListItem,
      ),
    );
  let playlist: YTMusic.Playlist;
  try {
    const yt = await getInnertube();
    playlist = await yt.music.getPlaylist(playlistId);
    collect(playlist);
  } catch (error) {
    sendError(
      new Error(
        t('plugins.downloader.backend.feedback.playlist-is-mix-or-private', {
          error: String(error),
        }),
      ),
    );
    return;
  }
  if (!playlist?.items?.length) {
    sendError(new Error(t('plugins.downloader.backend.feedback.playlist-is-empty')));
    return;
  }

  const normalPlaylistTitle =
    playlist.header && 'title' in playlist.header ? playlist.header.title?.text : undefined;
  const playlistTitle =
    normalPlaylistTitle ??
    playlist.page.contents_memo
      ?.get('MusicResponsiveListItemFlexColumn')
      ?.at(2)
      ?.as(YTNodes.MusicResponsiveListItemFlexColumn)?.title?.text ??
    'NO_TITLE';
  const isAlbum = !normalPlaylistTitle;

  try {
    while (playlist.has_continuation) {
      playlist = await playlist.getContinuation();
      collect(playlist);
    }
  } catch (error) {
    sendError(error, playlistTitle);
    return;
  }

  if (items.length === 1) {
    feedback(t('plugins.downloader.backend.feedback.playlist-has-only-one-song'));
    await downloadSong(items[0].id ?? null, playlistTitle);
    return;
  }

  const subfolder = playlistTitle.normalize('NFC');
  let folder: { path: string; exists: boolean };
  try {
    folder = (await ipcRenderer.invoke('downloader:prepare-folder', subfolder)) as typeof folder;
  } catch (error) {
    sendError(error, playlistTitle);
    return;
  }
  if (folder.exists) {
    sendError(
      new Error(
        t('plugins.downloader.backend.feedback.folder-already-exists', {
          playlistFolder: folder.path,
        }),
      ),
    );
    return;
  }

  showDialog(
    t('plugins.downloader.backend.dialog.start-download-playlist.title'),
    t('plugins.downloader.backend.dialog.start-download-playlist.message', { playlistTitle }),
    t('plugins.downloader.backend.dialog.start-download-playlist.detail', {
      playlistSize: items.length,
    }),
  );

  setProgressBar(2);
  try {
    for (const [index, song] of items.entries()) {
      feedback(
        t('plugins.downloader.backend.feedback.downloading-counter', {
          current: index + 1,
          total: items.length,
        }),
      );
      await downloadSong(song.id ?? null, `${song.author?.name} - ${song.title}`, {
        subfolder,
        track: isAlbum ? index + 1 : undefined,
        quiet: true,
        onProgress: (ratio) => setProgressBar((index + ratio) / items.length),
      });
      setProgressBar((index + 1) / items.length);
    }
  } finally {
    setProgressBar(-1);
    feedback();
  }
};

// src/plugins/downloader/renderer.tsx invokes these
handleLocally('download-song', (url) => downloadSong(getVideoId(url as string), url as string));
handleLocally('download-playlist-request', (url) => downloadPlaylist(url as string));

// The Rust side watches the song info for "download on finish"
ipcRenderer.on('downloader:download-on-finish', (_, url, folder) =>
  downloadSong(getVideoId(url as string), url as string, {
    folder: (folder as string | null) ?? undefined,
    quiet: true,
  }),
);
