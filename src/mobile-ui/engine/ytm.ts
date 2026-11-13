// The only place the mobile UI touches YouTube Music's internals: the
// <ytmusic-app> element (innertube requests, commands), the player API
// (#movie_player) and the queue's store. The UI only sees the signals and
// functions exported here, so a YTM change breaks this module, not the pages.
import { createSignal, untrack } from 'solid-js';

import {
  parseAccountMenu,
  parseAddToPlaylist,
  parseMenu,
  responseAction,
  responseMessage,
  type ResponseAction,
} from '../model/parse';

import type {
  AccountItem,
  AccountMenu,
  Endpoint,
  MenuEntry,
} from '../model/types';
import type { MusicPlayer } from '@/types/music-player';
import type { MusicPlayerAppElement } from '@/types/music-player-app-element';
import type { QueueElement } from '@/types/queue';

type YtmApp = MusicPlayerAppElement;

const app = () => document.querySelector<YtmApp>('ytmusic-app');

const waitFor = <T>(get: () => T | null | undefined): Promise<T> =>
  new Promise((resolve) => {
    const found = get();
    if (found) return resolve(found);
    const observer = new MutationObserver(() => {
      const value = get();
      if (value) {
        observer.disconnect();
        resolve(value);
      }
    });
    observer.observe(document.documentElement, {
      childList: true,
      subtree: true,
    });
  });

export const whenAppReady = () =>
  waitFor(() => {
    const element = app();
    return element?.networkManager ? element : null;
  });

/**
 * Clicks one of YTM's elements for the UI. After a touch, YTM's gesture code
 * (Polymer's ghost-click guard) cancels, for 2.5 s, every click whose path
 * misses the touched element: on a touch screen it cancelled each click the
 * UI passed on. It lets through mouse events whose sourceCapabilities say no
 * touch fired them; Chrome sets that, Gecko has no such property, so the
 * click carries it.
 */
const press = (element: Element | null | undefined) => {
  if (!element) return;
  const click = new MouseEvent('click', {
    bubbles: true,
    cancelable: true,
    composed: true,
    view: window,
  });
  Object.defineProperty(click, 'sourceCapabilities', {
    value: { firesTouchEvents: false },
  });
  element.dispatchEvent(click);
};

/** An innertube request through YTM's own client (auth, context, cookies) */
export const innertube = async <T = unknown>(
  path: string,
  body: Record<string, unknown>,
): Promise<T> => {
  const element = await whenAppReady();
  return element.networkManager.fetch<T, Record<string, unknown>>(path, body);
};

/**
 * Starts playback from a watch endpoint the way YTM's own links do. Browse
 * endpoints are the new UI's job and never get here.
 */
export const runEndpoint = (endpoint: Endpoint) => {
  app()?.dispatchEvent(
    new CustomEvent('yt-navigate', {
      detail: { endpoint },
      bubbles: true,
      composed: true,
    }),
  );
};

/** `menuServiceItemRenderer` -> `ytmusic-menu-service-item-renderer`;
 * the account menu's links are YouTube's shared (ytd-) elements */
const menuItemTag = (kind: string) =>
  `${kind === 'compactLinkRenderer' ? 'ytd' : 'ytmusic'}-${kind.replace(
    /[A-Z]/g,
    (c) => `-${c.toLowerCase()}`,
  )}`;

/**
 * Runs an item of a response's menu through YTM's own menu item element, so
 * it does exactly what tapping it in YTM's menu does: "add to queue", "save
 * to playlist" (YTM's popup, shown above the new UI), like toggles, share,
 * sign-in prompts. Sending these endpoints as yt-navigate or
 * yt-service-request does nothing for popups and only toasts for the queue.
 */
export const runMenuItem = (wrapped: Endpoint) => {
  const [kind, data] = Object.entries(wrapped)[0] ?? [];
  const host =
    document.querySelector('ytmusic-popup-container') ?? app() ?? undefined;
  if (!kind || !data || !host) return;
  const element = document.createElement(menuItemTag(kind)) as HTMLElement & {
    data?: unknown;
  };
  element.hidden = true;
  host.append(element);
  element.data = data;
  // Rendered on the next frame; YTM listens on the item's inner link
  requestAnimationFrame(() => {
    press(
      element.querySelector<HTMLElement>(
        'a, #navigation-endpoint, tp-yt-paper-item',
      ) ?? element,
    );
    setTimeout(() => element.remove(), 1000);
  });
};

// ---- Library actions -------------------------------------------------

const RATE_PATHS: Record<string, string> = {
  LIKE: '/like/like',
  DISLIKE: '/like/dislike',
  INDIFFERENT: '/like/removelike',
};

/** Like, dislike or un-rate a song (or save an album: a playlist target) */
export const rate = async (status: string, target: Record<string, string>) => {
  const path = RATE_PATHS[status];
  if (!path) throw new Error(`unknown rating ${status}`);
  await innertube(path, { target });
};

export interface SaveTarget {
  videoId?: string;
  playlistId?: string;
}

const saveBody = (target: SaveTarget) =>
  target.videoId
    ? { videoIds: [target.videoId] }
    : { playlistId: target.playlistId };

/** The playlists the user can save the target to */
export const playlistOptions = async (target: SaveTarget) =>
  parseAddToPlaylist(
    await innertube('/playlist/get_add_to_playlist', saveBody(target)),
  );

/** An action YTM refused, with its message ("Already in the playlist") */
export class YtmError extends Error {
  /** What YTM offers to do instead ("Add anyway") */
  action?: ResponseAction;
}

/**
 * Runs a playlist edit YTM prepared (add to / remove from a playlist).
 * Resolves to YTM's message, if any; rejects with it when it refused.
 */
export const editPlaylist = async (edit: unknown) => {
  const result = await innertube<{ status?: string }>(
    '/browse/edit_playlist',
    edit as Record<string, unknown>,
  );
  const message = responseMessage(result);
  if (result?.status && result.status !== 'STATUS_SUCCEEDED') {
    const error = new YtmError(message ?? result.status);
    error.action = responseAction(result);
    throw error;
  }
  return message;
};

/** A new private playlist holding the target */
export const createPlaylist = async (title: string, target: SaveTarget) => {
  const result = await innertube<{ playlistId?: string }>('/playlist/create', {
    title,
    privacyStatus: 'PRIVATE',
    ...(target.videoId
      ? { videoIds: [target.videoId] }
      : { sourcePlaylistId: target.playlistId }),
  });
  if (!result?.playlistId) throw new Error('no playlist created');
  return result.playlistId;
};

// ---- Account ---------------------------------------------------------

// Assumed signed in until YTM's config says otherwise, so a signed-in user
// does not see a sign-in button flash by
const [signedIn, setSignedIn] = createSignal(true);
export { signedIn };

/** Reads YTM's sign-in state; it does not change without a reload */
export const readAccount = () => {
  const yes =
    (window as { ytcfg?: { get?: (key: string) => unknown } }).ytcfg?.get?.(
      'LOGGED_IN',
    ) === true;
  setSignedIn(yes);
  if (yes) {
    readAccountInfo()
      .then(setAccount)
      .catch(() => setAccount(null));
  }
};

/** The signed-in account and YTM's account menu */
export const readAccountInfo = async () =>
  parseAccountMenu(await innertube('/account/account_menu', {}));

/**
 * Resolves to what `get` finds, or undefined after `ms`. Polls: YTM's menu
 * pages are kept and shown again by attribute, which a childList observer
 * would not see.
 */
const pollFor = <T>(get: () => T | null | undefined, ms: number) =>
  new Promise<T | undefined>((resolve) => {
    const until = performance.now() + ms;
    const check = () => {
      const found = get();
      if (found) resolve(found);
      else if (performance.now() > until) resolve(undefined);
      else setTimeout(check, 100);
    };
    check();
  });

/** A shown element of YTM's popups (menus keep their hidden pages) */
const shownInPopup = <E extends HTMLElement>(
  selector: string,
  test: (element: E) => boolean = () => true,
) =>
  Array.from(
    document.querySelectorAll<E>(`ytmusic-popup-container ${selector}`),
  ).find((element) => element.offsetParent !== null && test(element));

/** An item of YTM's account menu while it shows, by its icon */
const shownAccountItem = (icon: string) =>
  shownInPopup<HTMLElement & { data?: unknown }>(
    'ytd-compact-link-renderer',
    (element) =>
      (element.data as { icon?: { iconType?: string } } | undefined)?.icon
        ?.iconType === icon,
  );

let switcherWatch: MutationObserver | null = null;

/**
 * Switch account is a page of YTM's account menu (the menu loads the account
 * list and shows it in place), so a lone item element does nothing: open
 * YTM's own menu and go to that page, as tapping the item there does.
 * The page's ← goes back to YTM's account menu, which the UI replaces: once
 * that menu shows again, it is closed and `onBack` (the UI's own account
 * sheet) runs instead. Watching for the menu, not the ← button: YTM may act
 * on its tap gesture before any click a listener could stop.
 */
const openAccountSwitcher = async (onBack?: () => void) => {
  if (!shownAccountItem('SWITCH_ACCOUNTS')) {
    press(
      document.querySelector('ytmusic-settings-button button') ??
        document.querySelector(
          'ytmusic-settings-button tp-yt-paper-icon-button',
        ),
    );
  }
  const item = await pollFor(() => shownAccountItem('SWITCH_ACCOUNTS'), 3000);
  press(item?.querySelector('a, tp-yt-paper-item') ?? item);
  // Armed once the menu has moved on: during the slide both pages show
  const switcher = await pollFor(
    () =>
      shownAccountItem('SWITCH_ACCOUNTS')
        ? undefined
        : shownInPopup('ytd-account-item-renderer'),
    5000,
  );
  const dropdown = switcher?.closest<
    HTMLElement & { opened?: boolean; close?: () => void }
  >('tp-yt-iron-dropdown');
  const container = document.querySelector('ytmusic-popup-container');
  if (!dropdown || !container) return;
  switcherWatch?.disconnect();
  const watch = new MutationObserver(() => {
    if (shownAccountItem('SWITCH_ACCOUNTS')) {
      watch.disconnect();
      dropdown.close?.();
      onBack?.();
    } else if (!dropdown.opened) {
      // Closed some other way (a tap outside, an account picked)
      watch.disconnect();
    }
  });
  watch.observe(container, { attributes: true, childList: true, subtree: true });
  switcherWatch = watch;
};

/**
 * Runs an account menu item YTM handles itself (sign out, settings, upload,
 * help) through its own link element, as runMenuItem does. `onBack`: where
 * the account switcher's ← leads.
 */
export const runAccountItem = (item: AccountItem, onBack?: () => void) => {
  if (item.icon === 'SWITCH_ACCOUNTS') {
    openAccountSwitcher(onBack).catch(console.error);
  } else runMenuItem(item.ytm);
};

// The signed-in account (name, photo), read once YTM is ready
const [account, setAccount] = createSignal<AccountMenu | null>(null);
export { account };

/** The account's photo for the top bar; YTM's own (hidden) avatar is only
 * a placeholder until its nav bar shows, which it never does here */
export const avatarUrl = () => account()?.photo;

// ---- Pear's app menu --------------------------------------------------
//
// The menu in-app-menu's title bar draws (Rust builds it; on Android the
// plugin is always on): the same tree, clicks and refreshes.

export interface AppMenuItem {
  commandId: number;
  label: string;
  type: 'normal' | 'separator' | 'submenu' | 'checkbox' | 'radio';
  checked: boolean;
  enabled: boolean;
  visible: boolean;
  submenu?: { items: AppMenuItem[] };
}

const ipc = () =>
  (
    window as {
      ipcRenderer?: {
        invoke(channel: string, ...args: unknown[]): Promise<unknown>;
      };
    }
  ).ipcRenderer;

export const readAppMenu = async () =>
  ((await ipc()?.invoke('get-menu')) as { items: AppMenuItem[] } | null)
    ?.items ?? [];

/** Runs a menu item (checkboxes and radios switch), as the title bar does */
export const clickAppMenu = async (commandId: number) => {
  await ipc()?.invoke('peard:menu-event', commandId);
};

/** YTM's own sign-in link (hidden with its nav bar): the same Google flow */
export const signIn = () => press(document.querySelector('a.sign-in-link'));

// ---- Player ----------------------------------------------------------

export interface NowPlaying {
  videoId: string;
  title: string;
  artist: string;
  thumbnail?: string;
}

export interface QueueEntry {
  key: string;
  videoId: string;
  title: string;
  artist: string;
  thumbnail?: string;
  duration?: string;
  selected: boolean;
  endpoint?: Endpoint;
  menu: MenuEntry[];
}

const [song, setNowPlaying] = createSignal<NowPlaying | null>(null);
/**
 * YTM's own "is there a player" (`player-ui-state` on its layout): INACTIVE
 * once the queue is cleared, when YTM hides its player bar too. The video's
 * data stays loaded then, so the song alone would keep the bar up.
 */
const [playerActive, setPlayerActive] = createSignal(true);
/** The song in the player bar; none while YTM has no player */
const nowPlaying = () => (playerActive() ? song() : null);
const [isPlaying, setIsPlaying] = createSignal(false);
const [currentTime, setCurrentTime] = createSignal(0);
const [duration, setDuration] = createSignal(0);
const [queue, setQueue] = createSignal<QueueEntry[]>([]);
const [adShowing, setAdShowing] = createSignal(false);
/** The playing video's width / height: ads are not always 16:9 */
const [videoRatio, setVideoRatio] = createSignal(16 / 9);
const [likeStatus, setLikeStatus] = createSignal('INDIFFERENT');
const [repeatMode, setRepeatMode] = createSignal('NONE');
const [adSkippable, setAdSkippable] = createSignal(false);
/** The ad's own title and its advertiser (the "visit" link's text) */
const [adInfo, setAdInfo] = createSignal<{
  title?: string;
  advertiser?: string;
}>({});

export {
  videoRatio,
  likeStatus,
  repeatMode,
  adInfo,
  adShowing,
  adSkippable,
  currentTime,
  duration,
  isPlaying,
  nowPlaying,
  queue,
};

const SKIP_AD =
  '.ytp-skip-ad-button, .ytp-ad-skip-button-modern, .ytp-ad-skip-button';
// "You can skip the ad in 3 seconds": shown with the skip button's slot
const AD_COUNTDOWN = '.ytp-preview-ad, .ytp-ad-preview-container';
const visible = (selector: string) => {
  const element = document.querySelector<HTMLElement>(selector);
  return element && element.offsetParent !== null ? element : null;
};
const canSkipAd = () => !!visible(SKIP_AD) && !visible(AD_COUNTDOWN);

/**
 * The id the UI's "Skip ad" <label>s point at (index.tsx). YouTube's skip
 * button ignores clicks a script makes, but a tap on a label is forwarded to
 * its control as the user's own click, so the skip is a real one (and ends
 * the rest of the ad break, as skipping does in YTM).
 */
export const SKIP_AD_TARGET = 'pear-skip-ad';

// ---- The video in the UI ---------------------------------------------
//
// YTM's <video> (an ad's picture; later, music videos) is moved into the UI
// while the UI shows it: made a child of the UI's host and assigned to one of
// its slots, so it stays in the document (document.querySelector('video')
// still finds it, plugins included). A placeholder keeps its place in YTM's
// player for the way back. Moving a playing element does not pause it, YTM
// keeps using its reference and does not move it back (checked on a
// GeckoView tablet across song changes and ads). Showing the element itself
// is the only way on Android: GeckoView cannot draw hardware-decoded frames
// onto a canvas (they come out black).

/** For the document: YTM keeps writing the video's size and position */
export const VIDEO_CSS = (host: string) => `
${host} > video {
  position: static !important;
  display: block !important;
  width: 100% !important;
  height: 100% !important;
  left: auto !important;
  top: auto !important;
  transform: none !important;
  object-fit: contain !important;
  background: #000;
  /* Rounded by the video itself: a video is composited in its own layer,
     which a parent's rounded overflow clip does not always reach (Gecko) */
  border-radius: var(--pear-video-radius, 0) !important;
  clip-path: inset(0 round var(--pear-video-radius, 0)) !important;
}
${host} > video[slot='video-player'] {
  --pear-video-radius: 8px;
}
${host} > video[slot='video-mini'] {
  --pear-video-radius: 4px;
}
`;

let shown: { video: HTMLVideoElement; placeholder: Element } | null = null;

const returnVideo = () => {
  if (!shown) return;
  const { video, placeholder } = shown;
  shown = null;
  video.removeAttribute('slot');
  if (placeholder.isConnected) {
    placeholder.replaceWith(video);
  } else if (player && !player.querySelector('video')) {
    player.querySelector('.html5-video-container')?.append(video);
  } else {
    // YTM made a new one meanwhile: this one is done
    video.remove();
  }
};

/** Where the UI wants the video; applied whenever there is one to move */
let wanted: { host: Element; slot: string } | null = null;

const placeVideo = () => {
  const current = video;
  if (!wanted || !current) {
    returnVideo();
    return;
  }
  if (shown?.video !== current) {
    returnVideo();
    const placeholder = document.createElement('div');
    placeholder.className = 'pear-video-placeholder';
    current.before(placeholder);
    wanted.host.append(current);
    shown = { video: current, placeholder };
  }
  current.slot = wanted.slot;
};

/**
 * Shows YTM's video in the `slot` of the UI's `host`, or puts it back
 * (null). Kept as a wish: the video may not exist yet, or YTM may replace
 * it, and it follows.
 */
export const showVideo = (host: Element | null, slot?: string) => {
  wanted = host && slot ? { host, slot } : null;
  placeVideo();
};

/** "Ad 1 of 2 · 0:15" bits that can end up in the ad's title */
const AD_NOISE = [
  /第\s*\d+\s*則.*?\(共\s*\d+\s*則\)/g,
  /\bAd \d+ of \d+\b/gi,
  // A countdown at either end ("0:15 · …", "… · 0:15")
  /^\s*\d{1,2}:\d{2}\s*[·•]?|[·•]?\s*\d{1,2}:\d{2}\s*$/g,
  /^[\s·•|]+|[\s·•|]+$/g,
];
const cleanAdTitle = (title?: string) => {
  let clean = title ?? '';
  for (const pattern of AD_NOISE) clean = clean.replace(pattern, '');
  return clean.trim() || undefined;
};

/** The advertiser's link text can come percent-encoded (non-ASCII paths) */
const readable = (value?: string) => {
  if (!value) return value;
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

const markSkipButton = () => {
  const button = visible(SKIP_AD);
  if (button && button.id !== SKIP_AD_TARGET) {
    document.getElementById(SKIP_AD_TARGET)?.removeAttribute('id');
    button.id = SKIP_AD_TARGET;
  }
};

let player: (Element & MusicPlayer) | null = null;

export const play = () => player?.playVideo();
export const pause = () => player?.pauseVideo();
export const togglePlay = () => (isPlaying() ? pause() : play());
/**
 * YTM's player bar: `ytmusic-player-bar`, or (rolled out to some sessions on
 * 2026-11-13, seen on the tablet) the `ytmusic-miniplayer` that replaces it,
 * built from view models with other class names
 */
const playerBar = () =>
  document.querySelector('ytmusic-player-bar, ytmusic-miniplayer');

/** A bar button's class in each bar */
const BAR_BUTTONS = {
  next: ['next-button', 'ytmusicPlayerControlsNextButton'],
  previous: ['previous-button', 'ytmusicPlayerControlsPreviousButton'],
  shuffle: ['shuffle', 'ytmusicPlayerControlsShuffleButton'],
  repeat: ['repeat', 'ytmusicPlayerControlsRepeatButton'],
} as const;

/**
 * A button of YTM's player bar, by its class. Its native <button> is clicked
 * (the yt-icon-button host does not react to a click on itself everywhere).
 */
const playerBarButton = (name: keyof typeof BAR_BUTTONS) => {
  const bar = playerBar();
  const selector = BAR_BUTTONS[name].map((found) => `.${found}`).join(', ');
  const host =
    bar?.querySelector<HTMLElement>(selector) ??
    bar?.shadowRoot?.querySelector<HTMLElement>(selector);
  return host?.querySelector<HTMLElement>('button') ?? host ?? null;
};

// The player bar's buttons follow YTM's queue (and its "previous restarts the
// song first" rule); the player API's nextVideo() does not know the queue, so
// it is only the fallback
export const next = () => {
  const button = playerBarButton('next');
  if (button) press(button);
  else player?.nextVideo();
};
export const previous = () => {
  const button = playerBarButton('previous');
  if (button) press(button);
  else player?.previousVideo();
};
/** Shuffles the queue (an action in YTM, not a mode) */
export const shuffle = () => press(playerBarButton('shuffle'));
/** NONE -> ALL -> ONE, as YTM's repeat button cycles */
export const cycleRepeat = () => press(playerBarButton('repeat'));

/** The bar's like and dislike buttons, in either bar */
const likeButtons = () => {
  const bar = playerBar();
  const old = bar?.querySelectorAll<HTMLElement>('#like-button-renderer button');
  if (old?.length) return [old[0], old[1]];
  return [
    bar?.querySelector<HTMLElement>('like-button-view-model button'),
    bar?.querySelector<HTMLElement>('dislike-button-view-model button'),
  ];
};
/** YTM's own like buttons: they keep its state and sign-in prompt */
export const toggleLike = () => press(likeButtons()[0]);
export const toggleDislike = () => press(likeButtons()[1]);
export const seekTo = (seconds: number) => player?.seekTo(seconds);

type Runs = { runs?: { text: string }[] } | undefined;
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

/**
 * Entries already made, by YTM's item object: YTM replaces only the items
 * that change, so keeping their entries lets the queue list keep its rows
 * (rebuilding all of them threw its scroll position back to the top)
 */
let madeEntries = new WeakMap<object, QueueEntry>();
/** Each entry's item in YTM's queue, to find its place when it moves */
const itemOf = new WeakMap<QueueEntry, object>();

const readQueue = (element: QueueElement) => {
  const items = element.queue.store.store.getState().queue.items as unknown[];
  const entries: QueueEntry[] = [];
  const made = new WeakMap<object, QueueEntry>();
  items.forEach((item, index) => {
    const video = panelVideo(item);
    if (!video) return;
    const known = madeEntries.get(item as object);
    if (known) {
      entries.push(known);
      made.set(item as object, known);
      return;
    }
    entries.push({
      key: `${index}:${video.videoId}`,
      videoId: video.videoId,
      title: text(video.title),
      artist: text(video.shortBylineText ?? video.longBylineText),
      thumbnail: largest(video.thumbnail?.thumbnails),
      duration: text(video.lengthText),
      selected: !!video.selected,
      endpoint: video.navigationEndpoint,
      menu: parseMenu(video.menu),
    });
    made.set(item as object, entries[entries.length - 1]);
    itemOf.set(entries[entries.length - 1], item as object);
  });
  madeEntries = made;
  setQueue(entries);
  const current = entries.find((entry) => entry.selected);
  const shown = song();
  // Unchanged: a new object would wake everything following the song
  if (
    current &&
    (current.videoId !== shown?.videoId ||
      current.title !== shown.title ||
      current.artist !== shown.artist ||
      current.thumbnail !== shown.thumbnail)
  ) {
    setNowPlaying({
      videoId: current.videoId,
      title: current.title,
      artist: current.artist,
      thumbnail: current.thumbnail,
    });
  }
};

const readVideoData = () => {
  if (!player) return;
  const data = player.getVideoData() as {
    video_id?: string;
    title?: string;
    author?: string;
  };
  if (!data.video_id) return;
  // The queue has the album art; fall back to the video's own thumbnail
  // Read without tracking: this runs from YTM's events, not in an effect
  const fromQueue = untrack(queue).find(
    (entry) => entry.videoId === data.video_id,
  );
  setNowPlaying({
    videoId: data.video_id,
    // The queue's title is localized; the video's can be in another language
    title: fromQueue?.title || data.title || '',
    artist: fromQueue?.artist || data.author || '',
    thumbnail:
      fromQueue?.thumbnail ??
      `https://i.ytimg.com/vi/${data.video_id}/hqdefault.jpg`,
  });
  setDuration(player.getDuration() || 0);
};

// ---- Queue ----------------------------------------------------------

interface QueueService {
  continuation?: string | null;
  store: {
    store: {
      getState(): { queue: { items: unknown[]; nextQueueItemId: number } };
    };
  };
}

let queueService: QueueService | null = null;
let queueElementRef: Element | null = null;
let queueDispatch: ((action: unknown) => void) | null = null;
let loadingQueue = false;
const [queueHasMore, setQueueHasMore] = createSignal(false);
export { queueHasMore };

/**
 * The queue's next songs. YTM loads them when its own (hidden) queue list
 * scrolls to its end, which never happens under the new UI, so the UI asks
 * with YTM's continuation and appends them to YTM's queue the way YTM does.
 */
export const loadMoreQueue = async () => {
  const token = queueService?.continuation;
  if (!queueService || !queueDispatch || !token || loadingQueue) return;
  loadingQueue = true;
  try {
    const result = await innertube<{
      continuationContents?: {
        playlistPanelContinuation?: {
          contents?: unknown[];
          continuations?: Record<string, { continuation?: string }>[];
        };
      };
    }>('/next', { continuation: token });
    const panel = result?.continuationContents?.playlistPanelContinuation;
    const next = panel?.continuations
      ?.map((entry) => Object.values(entry)[0]?.continuation)
      .find(Boolean);
    const state = queueService.store.store.getState().queue;
    // Songs only: the page also carries a "more songs" message and automix
    // previews, which YTM's queue code throws on
    const songs = (panel?.contents ?? []).filter(
      (item) => panelVideo(item) !== undefined,
    );
    if (songs.length) {
      queueDispatch({
        type: 'ADD_ITEMS',
        payload: {
          nextQueueItemId: state.nextQueueItemId,
          index: state.items.length,
          items: songs,
          shuffleEnabled: false,
          shouldAssignIds: true,
        },
      });
    }
    // YTM would otherwise load the same songs again
    queueService.continuation = next ?? null;
  } finally {
    loadingQueue = false;
    setQueueHasMore(!!queueService?.continuation);
  }
};

let video: HTMLVideoElement | null = null;
let unbindPlayer: (() => void) | null = null;
let unbindVideo: (() => void) | null = null;

/** The player marks ads with a class; the skip button shows after a delay */
const watchAds = () => {
  if (!player) return;
  const showing = player.classList.contains('ad-showing');
  setAdShowing(showing);
  setAdSkippable(showing && canSkipAd());
  if (!showing) return;
  markSkipButton();
  // The player's (hidden) title bar shows the ad's title during an ad
  const title = cleanAdTitle(
    player.querySelector('.ytp-title-link')?.textContent ?? undefined,
  );
  const advertiser = readable(
    visible('.ytp-ad-button-text')?.textContent?.trim(),
  );
  const info = adInfo();
  if (info.title !== title || info.advertiser !== advertiser) {
    setAdInfo({ title, advertiser });
  }
};

type PlayerEvents = {
  addEventListener(type: string, listener: (value: unknown) => void): void;
  removeEventListener?(type: string, listener: (value: unknown) => void): void;
};

const bindPlayer = (next: Element & MusicPlayer) => {
  unbindPlayer?.();
  player = next;
  const api = next as unknown as PlayerEvents;
  const listeners: [string, (value: unknown) => void][] = [
    ['onStateChange', () => setDuration(player?.getDuration() || 0)],
    ['videodatachange', () => readVideoData()],
  ];
  for (const [type, listener] of listeners) {
    api.addEventListener(type, listener);
  }
  const ads = new MutationObserver(watchAds);
  ads.observe(next, {
    attributes: true,
    attributeFilter: ['class', 'style'],
    // The ad's title and advertiser change as text in place
    characterData: true,
    childList: true,
    subtree: true,
  });
  watchAds();
  unbindPlayer = () => {
    for (const [type, listener] of listeners) {
      api.removeEventListener?.(type, listener);
    }
    ads.disconnect();
  };
};

// Playing state from the <video> itself: reading video.paused once (or
// trusting a player state change from before) could leave the UI showing
// "paused"
const bindVideo = (next: HTMLVideoElement) => {
  unbindVideo?.();
  video = next;
  const abort = new AbortController();
  const options = { signal: abort.signal };
  next.addEventListener(
    'timeupdate',
    () => setCurrentTime(next.currentTime),
    options,
  );
  next.addEventListener(
    'durationchange',
    () => setDuration(Number.isFinite(next.duration) ? next.duration : 0),
    options,
  );
  const readRatio = () => {
    if (next.videoWidth && next.videoHeight) {
      setVideoRatio(next.videoWidth / next.videoHeight);
    }
  };
  next.addEventListener('loadedmetadata', readRatio, options);
  next.addEventListener('resize', readRatio, options);
  readRatio();
  next.addEventListener('playing', () => setIsPlaying(true), options);
  next.addEventListener('pause', () => setIsPlaying(false), options);
  next.addEventListener('ended', () => setIsPlaying(false), options);
  setIsPlaying(!next.paused);
  readVideoData();
  unbindVideo = () => abort.abort();
};

/**
 * Binds to YTM's current player and video, again whenever YTM replaces one
 * (seen after a cold start: the UI stayed bound to elements YTM had dropped,
 * so its play button and ad state stopped following)
 */
let bar: Element | null = null;
let unbindBar: (() => void) | null = null;

/** The like state, as the bar shows it */
const readLike = () => {
  const old = bar?.querySelector('#like-button-renderer');
  if (old) {
    setLikeStatus(old.getAttribute('like-status') ?? 'INDIFFERENT');
    return;
  }
  const [like, dislike] = likeButtons();
  const pressed = (button?: HTMLElement | null) =>
    button?.getAttribute('aria-pressed') === 'true';
  setLikeStatus(
    pressed(like) ? 'LIKE' : pressed(dislike) ? 'DISLIKE' : 'INDIFFERENT',
  );
};

const bindBar = (next: Element) => {
  unbindBar?.();
  bar = next;
  const observer = new MutationObserver(readLike);
  observer.observe(next, {
    attributes: true,
    attributeFilter: ['like-status', 'aria-pressed'],
    subtree: true,
  });
  readLike();
  unbindBar = () => observer.disconnect();
};

const follow = () => {
  const nextBar = playerBar();
  if (nextBar && nextBar !== bar) bindBar(nextBar);
  const nextPlayer = document.querySelector<Element & MusicPlayer>(
    '#movie_player',
  );
  if (nextPlayer && nextPlayer !== player) bindPlayer(nextPlayer);
  // In YTM's player, or in the UI while shown there
  const nextVideo = nextPlayer?.querySelector('video') ?? shown?.video ?? null;
  if (nextVideo && nextVideo !== video) {
    bindVideo(nextVideo);
    // A new (or first) video goes where the UI wants it
    placeVideo();
  }
};

/**
 * Moves a queue entry to `target`'s place, as dragging it in YTM's queue
 * does: through the queue's own drop handler (it also updates the server's
 * copy of the queue and a cast device), else the store's MOVE_ITEM.
 */
export const moveQueueEntry = (entry: QueueEntry, target: QueueEntry) => {
  const items = queueService?.store.store.getState().queue.items ?? [];
  const item = itemOf.get(entry);
  const targetItem = itemOf.get(target);
  const from = item ? items.indexOf(item) : -1;
  const to = targetItem ? items.indexOf(targetItem) : -1;
  if (from < 0 || to < 0 || from === to) return;
  const wrapped = item as {
    playlistPanelVideoRenderer?: unknown;
    playlistPanelVideoWrapperRenderer?: unknown;
  };
  // What YTM's drop handler expects: the dragged row, whose `data` is the
  // item's renderer, at its new place among its siblings
  const dragEl: { data: unknown; parentElement?: { children: unknown[] } } = {
    data:
      wrapped.playlistPanelVideoWrapperRenderer ??
      wrapped.playlistPanelVideoRenderer,
  };
  const children: unknown[] = Array.from({ length: items.length });
  children[to] = dragEl;
  dragEl.parentElement = { children };
  const drop = (queueElementRef as { onDrop?: (event: unknown) => void } | null)
    ?.onDrop;
  const before = items;
  try {
    drop?.call(queueElementRef, { detail: { dragEl } });
  } catch (error) {
    console.error('[pear-mobile-ui] queue drop', error);
  }
  // YTM's handler did not move it (its data did not match): move it here
  if (queueService?.store.store.getState().queue.items === before) {
    queueDispatch?.({
      type: 'MOVE_ITEM',
      payload: { fromIndex: from, toIndex: to },
    });
  }
};

/** The player and video, again whenever YTM replaces one */
const followPlayer = async () => {
  await waitFor(() => document.querySelector('#movie_player'));
  let pending: ReturnType<typeof setTimeout> | undefined;
  new MutationObserver(() => {
    pending ??= setTimeout(() => {
      pending = undefined;
      follow();
    }, 100);
  }).observe(document.body, { childList: true, subtree: true });
  follow();
};

/** Whether YTM has a player at all (none once the queue is dismissed) */
const followUiState = async () => {
  const layout = await waitFor(() =>
    document.querySelector('ytmusic-app-layout'),
  );
  const readUiState = () =>
    setPlayerActive(layout.getAttribute('player-ui-state') !== 'INACTIVE');
  new MutationObserver(readUiState).observe(layout, {
    attributes: true,
    attributeFilter: ['player-ui-state'],
  });
  readUiState();
};

/** The queue (and repeat mode) from YTM's queue store */
const followQueue = async () => {
  const queueElement = await waitFor(() =>
    document.querySelector<QueueElement>('#queue'),
  );
  const subscribe = () => {
    const store = queueElement.queue?.store?.store;
    if (!store) return false;
    queueService = queueElement.queue as unknown as QueueService;
    queueElementRef = queueElement;
    queueDispatch = (action) =>
      (queueElement as unknown as { dispatch(a: unknown): void }).dispatch(
        action,
      );
    const read = () => {
      readQueue(queueElement);
      setQueueHasMore(!!queueService?.continuation);
      // Both bars show it; the store has it whichever YTM draws
      const { repeatMode: mode } = (
        store.getState() as { queue: { repeatMode?: string } }
      ).queue;
      setRepeatMode(mode ?? 'NONE');
    };
    store.subscribe(read);
    read();
    return true;
  };
  if (!subscribe()) {
    const retry = setInterval(() => subscribe() && clearInterval(retry), 500);
  }
};

/**
 * Follows the player, the queue and YTM's player state, each once YTM has
 * created it: none waits for another (on a cold start to /watch, YTM was
 * seen to create the queue but not the player, or another player bar)
 */
export const connectPlayer = () =>
  Promise.all([followPlayer(), followUiState(), followQueue()]);
