// YouTube's player (see boot.ts), owned by the UI: created in a box that is a
// child of the UI's host, so it stays in the page's tree (its stylesheet,
// `#movie_player` for the renderer and plugins, the skip-ad labels) and shows
// in whichever of the UI's slots wants it, or off screen when none does.
import { createSignal } from 'solid-js';

import { createPlayer } from './boot';

/** The player element's API, as YTM's `#movie_player` has it */
interface PlayerMethods {
  loadVideoByPlayerVars(vars: Record<string, unknown>): void;
  playVideo(): void;
  pauseVideo(): void;
  stopVideo(): void;
  seekTo(seconds: number, allowSeekAhead?: boolean): void;
  getCurrentTime(): number;
  getDuration(): number;
  getPlayerState(): number;
  getVideoData(): { video_id?: string; title?: string; author?: string };
  getPlayerResponse?(): {
    videoDetails?: { musicVideoType?: string };
  } | null;
}
export type PlayerApi = HTMLElement & PlayerMethods;

/** The id of the player's box (a child of the UI's host) */
export const PLAYER_HOST_ID = 'pear-player-host';

/** The player's own events (`onStateChange`), on the same element */
const onPlayerEvent = <T>(
  element: HTMLElement,
  type: string,
  listener: (value: T) => void,
) =>
  (
    element as unknown as {
      addEventListener(name: string, callback: (value: T) => void): void;
    }
  ).addEventListener(type, listener);

const [isPlaying, setIsPlaying] = createSignal(false);
const [currentTime, setCurrentTime] = createSignal(0);
const [duration, setDuration] = createSignal(0);
const [adShowing, setAdShowing] = createSignal(false);
const [adSkippable, setAdSkippable] = createSignal(false);
/** The ad's own title and its advertiser (the "visit" link's text) */
const [adInfo, setAdInfo] = createSignal<{
  title?: string;
  advertiser?: string;
}>({});
/** The playing video's width / height: ads are not always 16:9 */
const [videoRatio, setVideoRatio] = createSignal(16 / 9);
/**
 * A song on its way: from the tap (the queue's request) until the player
 * plays it, or while it buffers. The first song after a start waits for the
 * player's BotGuard (up to half a minute).
 */
const [loading, setLoading] = createSignal(false);
/**
 * `asked`: the queue is still fetching what to play (the previous song may
 * play on meanwhile); `loading`: the player has been given the new song
 */
let loadPending: false | 'asked' | 'loading' = false;

/** A song was asked for: loading until it plays (or gives up) */
export const markLoading = (pending = true) => {
  loadPending = pending ? 'asked' : false;
  setLoading(pending);
};

/**
 * The asked-for song (or its ad) is moving: the player reports "playing"
 * well before that (the time sits at 0:00 meanwhile), so its time is what
 * ends the load. A song's start, not the previous song's last updates: the
 * previous song's early seconds, still playing while the queue asks, do not
 * count.
 */
const loadMoved = (time: number) => {
  if (loadPending !== 'loading' || time < 0.05 || time > 5) return;
  loadPending = false;
  setLoading(false);
};

/** Whether the song has a video (not YTM's audio track, `ATV`) */
const [hasVideo, setHasVideo] = createSignal(false);
/** Song or video (video-toggle): the video shows when the song has one */
const [videoMode, setVideoMode] = createSignal(false);

/** Whether the player page shows the song's video instead of its picture */
export const songVideoShown = () => videoMode() && hasVideo();

export {
  adInfo,
  adShowing,
  adSkippable,
  currentTime,
  duration,
  isPlaying,
  loading,
  setVideoMode,
  videoRatio,
};

/** The slot the player waits in when no part of the UI shows it */
export const PARKED_SLOT = 'player-parked';
const HOST_ID = PLAYER_HOST_ID;

/** For the document: the player's box, and its own chrome hidden */
export const PLAYER_CSS = (host: string) => `
${host} > #${HOST_ID} {
  display: block;
  width: 100%;
  height: 100%;
  overflow: hidden;
  background: #000;
  border-radius: var(--pear-video-radius, 0);
  clip-path: inset(0 round var(--pear-video-radius, 0));
}
${host} > #${HOST_ID}[slot='video-player'] { --pear-video-radius: 8px; }
${host} > #${HOST_ID}[slot='video-mini'] { --pear-video-radius: 4px; }
/* The bar's thumbnail is the ad's picture only: its interface (the skip
   button, the advertiser's link) is the bar's own. Transparent, not
   removed: the skip button must stay laid out for the bar's "Skip ad"
   label to find and reach it. */
${host} > #${HOST_ID}[slot='video-mini'] :is(.video-ads,
  .ytp-ad-player-overlay-layout, .ytp-ad-player-overlay) {
  opacity: 0 !important;
  pointer-events: none !important;
}
#${HOST_ID} #movie_player,
#${HOST_ID} .html5-video-container {
  width: 100% !important;
  height: 100% !important;
}
#${HOST_ID} video {
  position: static !important;
  width: 100% !important;
  height: 100% !important;
  left: auto !important;
  top: auto !important;
  object-fit: contain !important;
}
/* The UI has its own controls; the player keeps only its ad interface
   (its skip button included: on the player page it is tapped directly) */
#${HOST_ID} :is(.ytp-chrome-top, .ytp-chrome-bottom, .ytp-gradient-top,
  .ytp-gradient-bottom, .ytp-large-play-button, .ytp-pause-overlay,
  .ytp-watermark, .ytp-ce-element, .ytp-cards-teaser, .ytp-cards-button,
  .html5-endscreen, .ytp-autonav-endscreen-countdown-overlay,
  .ytp-paid-content-overlay, .ytp-spinner, .ytp-bezel,
  .ytp-bezel-text-wrapper, .ytp-tooltip, .ytp-contextmenu) {
  display: none !important;
}
`;

const SKIP_AD =
  '.ytp-skip-ad-button, .ytp-ad-skip-button-modern, .ytp-ad-skip-button';
// "You can skip the ad in 3 seconds": shown with the skip button's slot
const AD_COUNTDOWN = '.ytp-preview-ad, .ytp-ad-preview-container';

/**
 * The id the UI's "Skip ad" <label>s point at (index.tsx). YouTube's skip
 * button ignores clicks a script makes, but a tap on a label is forwarded to
 * its control as the user's own click, so the skip is a real one.
 */
export const SKIP_AD_TARGET = 'pear-skip-ad';

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

let player: PlayerApi | null = null;
let host: HTMLElement | null = null;
let video: HTMLVideoElement | null = null;
const endedListeners: (() => void)[] = [];

/** Where the song (not an ad) was last: an ad after its end ends it */
let songTime = 0;
let songDuration = 0;
let adWasShowing = false;
/** The song's end is told once per load */
let endTold = false;

const songEnded = () => {
  if (endTold) return;
  endTold = true;
  for (const listener of endedListeners) listener();
};

const shown = (selector: string) => {
  const element = player?.querySelector<HTMLElement>(selector);
  return element && element.offsetParent !== null ? element : null;
};

const watchAds = () => {
  if (!player) return;
  const showing = player.classList.contains('ad-showing');
  setAdShowing(showing);
  // An ad after the song's end (played out or skipped): the player reports
  // no end of the song after it, so the song ends here
  if (adWasShowing && !showing) {
    const atEnd = songDuration > 0 && songTime >= songDuration - 1.5;
    if (atEnd || player.getPlayerState() === 0) songEnded();
  }
  adWasShowing = showing;
  const skip = showing ? shown(SKIP_AD) : null;
  setAdSkippable(!!skip && !shown(AD_COUNTDOWN));
  if (skip && skip.id !== SKIP_AD_TARGET) {
    document.getElementById(SKIP_AD_TARGET)?.removeAttribute('id');
    skip.id = SKIP_AD_TARGET;
  }
  if (!showing) return;
  // The player's (hidden) title bar shows the ad's title during an ad
  const title = cleanAdTitle(
    player.querySelector('.ytp-title-link')?.textContent ?? undefined,
  );
  const advertiser = readable(
    shown('.ytp-ad-button-text')?.textContent?.trim(),
  );
  const info = adInfo();
  if (info.title !== title || info.advertiser !== advertiser) {
    setAdInfo({ title, advertiser });
  }
};

const bindVideo = (next: HTMLVideoElement) => {
  video = next;
  const readRatio = () => {
    if (next.videoWidth && next.videoHeight) {
      setVideoRatio(next.videoWidth / next.videoHeight);
    }
  };
  next.addEventListener('timeupdate', () => {
    setCurrentTime(next.currentTime);
    loadMoved(next.currentTime);
    if (!player?.classList.contains('ad-showing')) {
      songTime = next.currentTime;
      if (Number.isFinite(next.duration)) songDuration = next.duration;
    }
  });
  next.addEventListener('durationchange', () =>
    setDuration(Number.isFinite(next.duration) ? next.duration : 0),
  );
  next.addEventListener('loadedmetadata', readRatio);
  next.addEventListener('resize', readRatio);
  next.addEventListener('playing', () => setIsPlaying(true));
  next.addEventListener('pause', () => setIsPlaying(false));
  next.addEventListener('ended', () => setIsPlaying(false));
};

/**
 * Creates the player in the UI's `uiHost`. Resolves once it can play; the
 * first song still waits for the player's BotGuard to start (seen: up to
 * half a minute on a cold start), later ones start at once.
 */
export const startPlayer = async (uiHost: HTMLElement) => {
  host = document.createElement('div');
  host.id = HOST_ID;
  host.slot = PARKED_SLOT;
  uiHost.append(host);
  const element = (await createPlayer(host)) as PlayerApi;
  player = element;
  onPlayerEvent<number>(element, 'onStateChange', (state) => {
    setDuration(element.getDuration() || 0);
    // Paused, ended or cued: the load is over; buffering shows too
    if (state === 0 || state === 2 || state === 5) loadPending = false;
    setLoading(!!loadPending || state === 3);
    // ENDED of the song (an ad's end does not report it). After the load's
    // end above, so the next song it starts shows as loading.
    if (state === 0 && !element.classList.contains('ad-showing')) songEnded();
    // The song's own response (an ad's state changes leave it as it was)
    if (!element.classList.contains('ad-showing')) {
      const details = element.getPlayerResponse?.()?.videoDetails;
      setHasVideo(
        !!details && details.musicVideoType !== 'MUSIC_VIDEO_TYPE_ATV',
      );
    }
  });
  new MutationObserver(() => {
    const next = element.querySelector('video');
    if (next && next !== video) bindVideo(next);
    watchAds();
  }).observe(element, {
    attributes: true,
    attributeFilter: ['class', 'style'],
    characterData: true,
    childList: true,
    subtree: true,
  });
  const first = element.querySelector('video');
  if (first) bindVideo(first);
  watchAds();
  return element;
};

/** Runs when a song (not an ad) plays to its end */
export const onEnded = (listener: () => void) => {
  endedListeners.push(listener);
};

/** Plays a song: the player fetches it (and its ads) itself */
export const loadVideo = (videoId: string, playerParams?: string) => {
  if (!player) return;
  setCurrentTime(0);
  songTime = 0;
  songDuration = 0;
  endTold = false;
  markLoading();
  loadPending = 'loading';
  player.loadVideoByPlayerVars(
    playerParams ? { videoId, playerParams } : { videoId },
  );
  player.playVideo();
};

export const play = () => player?.playVideo();
export const pause = () => player?.pauseVideo();
export const togglePlay = () => (isPlaying() ? pause() : play());
export const stop = () => {
  markLoading(false);
  player?.stopVideo();
  setIsPlaying(false);
  setCurrentTime(0);
  setDuration(0);
};
export const seekTo = (seconds: number) => player?.seekTo(seconds, true);

interface QualityApi {
  getAvailableQualityLevels?(): string[];
  getAvailableQualityLabels?(): string[];
  getPlaybackQuality?(): string;
  setPlaybackQualityRange?(level: string): void;
  setPlaybackQuality?(level: string): void;
}

/** The stream's qualities (quality-changer): codes, their labels, the current */
export const qualityLevels = () => {
  const api = player as (PlayerApi & QualityApi) | null;
  return {
    levels: api?.getAvailableQualityLevels?.() ?? [],
    labels: api?.getAvailableQualityLabels?.() ?? [],
    current: api?.getPlaybackQuality?.(),
  };
};

/** Fixes the quality, as quality-changer does */
export const setQuality = (level: string) => {
  const api = player as (PlayerApi & QualityApi) | null;
  api?.setPlaybackQualityRange?.(level);
  api?.setPlaybackQuality?.(level);
};
export const playerTime = () => player?.getCurrentTime() ?? 0;

/** Shows the player in the UI's `slot`, or parks it off screen (null) */
export const showVideo = (slot: string | null) => {
  if (host) host.slot = slot ?? PARKED_SLOT;
};
