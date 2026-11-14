// The play queue, kept by the UI (YTM's queue store is gone with its
// interface). Built from /next as YTM does: the songs of a playlist or radio,
// continued page by page, then by the automix (radio) YTM appends at the end.
// "Play next" and "Add to queue" fetch the songs through /music/get_queue.
import { batch, createSignal, untrack } from 'solid-js';

import { innertube } from './innertube';
import {
  loadVideo,
  markLoading,
  onEnded,
  playerTime,
  seekTo,
  stop,
} from './player';

import { browseTarget, parseMenu } from '../model/parse';

import type { BrowseTarget, Endpoint, MenuEntry } from '../model/types';

export interface NowPlaying {
  videoId: string;
  title: string;
  artist: string;
  thumbnail?: string;
  /** The artist's page */
  artistOpen?: BrowseTarget;
}

export interface QueueEntry {
  key: string;
  videoId: string;
  title: string;
  artist: string;
  thumbnail?: string;
  duration?: string;
  /** The artist's page (the byline's artist link) */
  artistOpen?: BrowseTarget;
  /** The album's name (the byline's album link) */
  album?: string;
  /** What starts it (its watch endpoint, with its player params) */
  endpoint?: Endpoint;
  menu: MenuEntry[];
}

type Runs =
  | { runs?: { text: string; navigationEndpoint?: Endpoint }[] }
  | undefined;
const text = (value: Runs) =>
  value?.runs?.map((run) => run.text).join('') ?? '';

interface PanelVideo {
  videoId: string;
  title?: Runs;
  shortBylineText?: Runs;
  longBylineText?: Runs;
  lengthText?: Runs;
  selected?: boolean;
  thumbnail?: { thumbnails?: { url: string; width?: number }[] };
  navigationEndpoint?: Endpoint;
  menu?: unknown;
}

const panelVideo = (item: unknown): PanelVideo | undefined => {
  const it = item as {
    playlistPanelVideoRenderer?: PanelVideo;
    playlistPanelVideoWrapperRenderer?: {
      primaryRenderer: { playlistPanelVideoRenderer?: PanelVideo };
    };
  };
  return (
    it.playlistPanelVideoRenderer ??
    it.playlistPanelVideoWrapperRenderer?.primaryRenderer
      .playlistPanelVideoRenderer
  );
};

const largest = (thumbnails?: { url: string; width?: number }[]) =>
  thumbnails?.reduce((best, thumb) =>
    (thumb.width ?? 0) > (best.width ?? 0) ? thumb : best,
  ).url;

let keySeed = 0;

/** A byline's first link to a page of `pageType` ("Artist • Album • Year") */
const linkOf = (byline: Runs, pageType: BrowseTarget['pageType']) => {
  for (const run of byline?.runs ?? []) {
    const target = run.navigationEndpoint
      ? browseTarget(run.navigationEndpoint as never)
      : undefined;
    if (target?.pageType === pageType) return { text: run.text, target };
  }
  return undefined;
};

const toEntry = (video: PanelVideo): QueueEntry => ({
  key: `q${keySeed++}:${video.videoId}`,
  videoId: video.videoId,
  title: text(video.title),
  artist: text(video.shortBylineText ?? video.longBylineText),
  thumbnail: largest(video.thumbnail?.thumbnails),
  duration: text(video.lengthText),
  artistOpen: (
    linkOf(video.longBylineText, 'artist') ??
    linkOf(video.shortBylineText, 'artist')
  )?.target,
  album: linkOf(video.longBylineText, 'album')?.text,
  endpoint: video.navigationEndpoint,
  menu: parseMenu(video.menu),
});

interface Panel {
  contents?: unknown[];
  continuations?: Record<string, { continuation?: string }>[];
}

/** A queue page: its songs, which one is selected, what comes after it */
const readPanel = (panel: Panel | undefined) => {
  const entries: QueueEntry[] = [];
  let selected = -1;
  let automix: Endpoint | undefined;
  for (const item of panel?.contents ?? []) {
    const video = panelVideo(item);
    if (video) {
      if (video.selected && selected < 0) selected = entries.length;
      entries.push(toEntry(video));
      continue;
    }
    const preview = (
      item as {
        automixPreviewVideoRenderer?: {
          content?: {
            automixPlaylistVideoRenderer?: { navigationEndpoint?: Endpoint };
          };
        };
      }
    ).automixPreviewVideoRenderer;
    automix ??=
      preview?.content?.automixPlaylistVideoRenderer?.navigationEndpoint;
  }
  const continuation = panel?.continuations
    ?.map((entry) => Object.values(entry)[0]?.continuation)
    .find(Boolean);
  return { entries, selected, continuation, automix };
};

type WatchNext = {
  contents?: {
    singleColumnMusicWatchNextResultsRenderer?: {
      tabbedRenderer?: {
        watchNextTabbedResultsRenderer?: {
          tabs?: {
            tabRenderer?: {
              content?: {
                musicQueueRenderer?: {
                  content?: { playlistPanelRenderer?: Panel };
                };
              };
            };
          }[];
        };
      };
    };
  };
  continuationContents?: { playlistPanelContinuation?: Panel };
  playerOverlays?: {
    playerOverlayRenderer?: {
      actions?: { likeButtonRenderer?: { likeStatus?: string } }[];
    };
  };
};

const queuePanel = (response: WatchNext) =>
  response.contents?.singleColumnMusicWatchNextResultsRenderer?.tabbedRenderer
    ?.watchNextTabbedResultsRenderer?.tabs?.[0]?.tabRenderer?.content
    ?.musicQueueRenderer?.content?.playlistPanelRenderer;

const [queue, setQueue] = createSignal<QueueEntry[]>([]);
const [current, setCurrent] = createSignal<QueueEntry | null>(null);
const [likeStatus, setLikeStatus] = createSignal('INDIFFERENT');
const [repeatMode, setRepeatMode] = createSignal('NONE');
const [queueHasMore, setQueueHasMore] = createSignal(false);

export { likeStatus, queue, queueHasMore, repeatMode, setLikeStatus };

/** The song being played (the queue entry), or none */
export const currentEntry = current;

/** The song in the player bar */
export const nowPlaying = (): NowPlaying | null => {
  const entry = current();
  return entry
    ? {
        videoId: entry.videoId,
        title: entry.title,
        artist: entry.artist,
        thumbnail: entry.thumbnail,
        artistOpen: entry.artistOpen,
      }
    : null;
};

let continuation: string | undefined;
let automix: Endpoint | undefined;
let loading = false;

const updateMore = () => setQueueHasMore(!!continuation || !!automix);

const playerParams = (endpoint?: Endpoint) =>
  (endpoint?.watchEndpoint as { playerParams?: string } | undefined)
    ?.playerParams;

/**
 * The current song's like state, from its /next (as YTM's player bar).
 * YouTube now sends it in a view model (`likeStatusEntity`, inside the
 * player overlay's action bar); the older `likeButtonRenderer` is the
 * fallback.
 */
const readLike = (response: WatchNext) => {
  let like: string | undefined;
  JSON.stringify(response.playerOverlays ?? {}, (key, value: unknown) => {
    if (!like && key === 'likeStatusEntity') {
      like = (value as { likeStatus?: string } | undefined)?.likeStatus;
    }
    return value;
  });
  like ??= response.playerOverlays?.playerOverlayRenderer?.actions?.find(
    (action) => action.likeButtonRenderer,
  )?.likeButtonRenderer?.likeStatus;
  setLikeStatus(like ?? 'INDIFFERENT');
};

let likeRequest = 0;

const playEntry = (entry: QueueEntry) => {
  // A queue play outdates a playlist still loading
  playRequest++;
  setCurrent(entry);
  loadVideo(entry.videoId, playerParams(entry.endpoint));
  setLikeStatus('INDIFFERENT');
  const request = ++likeRequest;
  innertube<WatchNext>('/next', { videoId: entry.videoId })
    .then((response) => request === likeRequest && readLike(response))
    .catch(() => {});
};

/**
 * Starts a watch endpoint (a song, an album, a playlist, a radio): its queue
 * replaces the current one, from the song it selects.
 */
let playRequest = 0;

export const playEndpoint = async (endpoint: Endpoint) => {
  const watch = (endpoint.watchEndpoint ?? endpoint.watchPlaylistEndpoint) as
    | Record<string, unknown>
    | undefined;
  if (!watch) return;
  // Only the latest tap plays: an earlier, slower answer is dropped
  const request = ++playRequest;
  markLoading();
  let response: WatchNext;
  try {
    response = await innertube<WatchNext>('/next', {
    videoId: watch.videoId,
    playlistId: watch.playlistId,
    params: watch.params,
    playerParams: watch.playerParams,
    index: watch.index,
    isAudioOnly: true,
    enablePersistentPlaylistPanel: true,
    tunerSettingValue: 'AUTOMIX_SETTING_NORMAL',
    });
  } catch (error) {
    if (request === playRequest) markLoading(false);
    throw error;
  }
  if (request !== playRequest) return;
  const page = readPanel(queuePanel(response));
  if (!page.entries.length) {
    markLoading(false);
    return;
  }
  const start = page.selected >= 0 ? page.selected : 0;
  continuation = page.continuation;
  automix = page.automix;
  batch(() => {
    setQueue(page.entries);
    updateMore();
  });
  const first = page.entries[start];
  setCurrent(first);
  loadVideo(first.videoId, playerParams(first.endpoint) ?? (watch.playerParams as string | undefined));
  readLike(response);
};

/**
 * The queue's next songs: its own continuation, then the automix (a radio of
 * the queue) once the playlist is done, as YTM appends it
 */
export const loadMoreQueue = async () => {
  if (loading || (!continuation && !automix)) return;
  loading = true;
  try {
    let page: ReturnType<typeof readPanel>;
    if (continuation) {
      const response = await innertube<WatchNext>('/next', {
        continuation,
        isAudioOnly: true,
        enablePersistentPlaylistPanel: true,
      });
      page = readPanel(response.continuationContents?.playlistPanelContinuation);
      continuation = page.continuation;
    } else {
      const watch = automix?.watchPlaylistEndpoint as
        | Record<string, unknown>
        | undefined;
      automix = undefined;
      const response = await innertube<WatchNext>('/next', {
        playlistId: watch?.playlistId,
        params: watch?.params,
        isAudioOnly: true,
        enablePersistentPlaylistPanel: true,
        tunerSettingValue: 'AUTOMIX_SETTING_NORMAL',
      });
      page = readPanel(queuePanel(response));
      continuation = page.continuation;
      // The radio starts with the queue's own songs again: only new ones
      const known = new Set(untrack(queue).map((entry) => entry.videoId));
      page.entries = page.entries.filter((entry) => !known.has(entry.videoId));
    }
    setQueue((entries) => [...entries, ...page.entries]);
  } finally {
    loading = false;
    updateMore();
  }
};

const indexOfCurrent = () => {
  const entry = current();
  return entry ? queue().indexOf(entry) : -1;
};

/** The next song; at the end, more of the queue, else (repeat all) the first */
export const next = async () => {
  const index = indexOfCurrent();
  if (index + 1 >= queue().length && queueHasMore()) await loadMoreQueue();
  const entries = queue();
  const following = entries[index + 1];
  if (following) playEntry(following);
  else if (repeatMode() === 'ALL' && entries.length) playEntry(entries[0]);
};

/** As YTM: a few seconds in, back to the start; else the song before */
export const previous = () => {
  const index = indexOfCurrent();
  if (playerTime() > 3 || index <= 0) {
    seekTo(0);
    return;
  }
  playEntry(queue()[index - 1]);
};

/** Plays a song of the queue */
export const playQueueEntry = (entry: QueueEntry) => {
  if (queue().includes(entry)) playEntry(entry);
};

// A song's end, from the player's event (no reactive scope to track in)
onEnded(() =>
  untrack(() => {
    if (repeatMode() === 'ONE') {
      const entry = current();
      if (entry) playEntry(entry);
      return;
    }
    next().catch(console.error);
  }),
);

/** NONE -> ALL -> ONE, as YTM's repeat button cycles */
export const cycleRepeat = () =>
  setRepeatMode((mode) =>
    mode === 'NONE' ? 'ALL' : mode === 'ALL' ? 'ONE' : 'NONE',
  );

/** Shuffles the rest of the queue around the current song, now first */
export const shuffle = () => {
  const entry = current();
  const rest = queue().filter((candidate) => candidate !== entry);
  for (let i = rest.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [rest[i], rest[j]] = [rest[j], rest[i]];
  }
  setQueue(entry ? [entry, ...rest] : rest);
};

/** Moves an entry to `target`'s place (dragging in the queue) */
export const moveQueueEntry = (entry: QueueEntry, target: QueueEntry) => {
  const entries = [...queue()];
  const from = entries.indexOf(entry);
  const to = entries.indexOf(target);
  if (from < 0 || to < 0 || from === to) return;
  entries.splice(from, 1);
  entries.splice(to, 0, entry);
  setQueue(entries);
};

/** Takes a song out of the queue; the playing one gives way to the next */
export const removeFromQueue = (entry: QueueEntry) => {
  const entries = queue();
  const index = entries.indexOf(entry);
  if (index < 0) return;
  const rest = entries.filter((candidate) => candidate !== entry);
  setQueue(rest);
  if (entry === current()) {
    const following = rest[index];
    if (following) playEntry(following);
    else clearQueue();
  }
};

/** "Dismiss queue": nothing plays, the player bar goes */
export const clearQueue = () => {
  continuation = undefined;
  automix = undefined;
  batch(() => {
    setQueue([]);
    setCurrent(null);
    updateMore();
  });
  stop();
};

/**
 * "Play next" / "Add to queue" (queueAddEndpoint): the target's songs after
 * the current one or at the end; on an empty queue, the endpoint YTM gives
 * for that case plays instead
 */
export const addToQueue = async (add: Record<string, unknown>) => {
  const target = (add.queueTarget ?? {}) as {
    videoId?: string;
    playlistId?: string;
    onEmptyQueue?: Endpoint;
  };
  if (!queue().length) {
    if (target.onEmptyQueue) await playEndpoint(target.onEmptyQueue);
    return;
  }
  const response = await innertube<{
    queueDatas?: { content?: unknown }[];
  }>(
    '/music/get_queue',
    target.videoId
      ? { videoIds: [target.videoId] }
      : { playlistId: target.playlistId },
  );
  const added = (response.queueDatas ?? [])
    .map((data) => panelVideo(data.content))
    .filter((video): video is PanelVideo => !!video)
    .map(toEntry);
  if (!added.length) return;
  const entries = [...queue()];
  const at =
    add.queueInsertPosition === 'INSERT_AFTER_CURRENT_VIDEO'
      ? indexOfCurrent() + 1
      : entries.length;
  entries.splice(at, 0, ...added);
  setQueue(entries);
};

/** The queue entry a menu entry belongs to (its "remove from queue") */
export const entryOfMenu = (endpoint: unknown) =>
  untrack(queue).find((entry) =>
    entry.menu.some((item) => item.endpoint === endpoint),
  );

export { playerTime, seekTo };
