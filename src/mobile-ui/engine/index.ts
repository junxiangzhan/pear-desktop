// The UI's engine: YouTube Music's API called directly (innertube.ts),
// YouTube's own player alone (boot.ts, player.ts), the queue kept here
// (queue.ts). YTM's interface is not loaded (docs/OWN-ENGINE.md); the UI
// only sees what this module exports.
import { createEffect, createRoot, on } from 'solid-js';

import { toggleDislike, toggleLike } from './commands';
import { innertube } from './innertube';
import { readAccount } from './library';
import { adShowing, setVideoMode, songVideoShown, startPlayer } from './player';
import {
  cycleRepeat,
  likeStatus,
  next,
  nowPlaying,
  previous,
  queue,
  repeatMode,
  shuffle,
} from './queue';
import { startSession } from './session';

import type { Cover, PlayerControls } from '@/types/player-controls';

export {
  accountAction,
  addAccount,
  openUrl,
  readAccounts,
  readSettings,
  runSetting,
  signOut,
  switchAccount,
  UPLOAD_TYPES,
  uploadSong,
  type AccountAction,
} from './account';
export { blockYtmInterface } from './boot';
export {
  rateTarget,
  runCommand,
  runEndpoint,
  runMenuItem,
  toggleDislike,
  toggleLike,
  type CommandOutcome,
} from './commands';
export { innertube, signedIn, signIn } from './innertube';
export {
  account,
  appMenuVersion,
  avatarUrl,
  clickAppMenu,
  createPlaylist,
  editPlaylist,
  playlistOptions,
  rate,
  readAppMenu,
  isSubscribed,
  setSubscription,
  YtmError,
  type AppMenuItem,
  type SaveTarget,
} from './library';
export {
  adInfo,
  adShowing,
  adSkippable,
  currentTime,
  duration,
  isPlaying,
  loading,
  pause,
  play,
  PARKED_SLOT,
  PLAYER_HOST_ID,
  PLAYER_CSS,
  qualityLevels,
  seekTo,
  setQuality,
  showVideo,
  SKIP_AD_TARGET,
  songVideoShown,
  togglePlay,
  videoRatio,
} from './player';
export {
  currentEntry,
  cycleRepeat,
  likeStatus,
  loadMoreQueue,
  moveQueueEntry,
  next,
  nowPlaying,
  playQueueEntry,
  previous,
  queue,
  queueHasMore,
  repeatMode,
  shuffle,
  type NowPlaying,
  type QueueEntry,
} from './queue';

/** Listeners of a signal's changes, for code outside Solid */
const watch = <T>(read: () => T) => {
  const listeners = new Set<(value: T) => void>();
  createRoot(() =>
    createEffect(
      on(
        read,
        (value) => {
          for (const listener of listeners) listener(value);
        },
        { defer: true },
      ),
    ),
  );
  return (listener: (value: T) => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  };
};

/** What the player page's cover shows (its picture, else a video) */
const cover = (): Cover => ({
  image: nowPlaying()?.thumbnail,
  video: adShowing() || songVideoShown(),
});

/**
 * What Pear's renderer runs for the commands Rust sends (the media
 * notification's buttons, shortcuts) and what the song-info provider reads:
 * they used to press and read YTM's player bar and queue
 */
const exposeControls = () => {
  const controls: PlayerControls = {
    next: () => {
      next().catch(console.error);
    },
    previous,
    shuffle,
    cycleRepeat,
    setLike: (status) => {
      if (likeStatus() === status) return;
      if (status === 'LIKE') toggleLike();
      else toggleDislike();
    },
    repeatMode,
    likeStatus,
    onRepeatChange: watch(repeatMode),
    onLikeChange: watch(likeStatus),
    innertube,
    songDetails: (videoId) => {
      const entry = queue().find((candidate) => candidate.videoId === videoId);
      return entry && { title: entry.title, album: entry.album };
    },
    cover,
    onCoverChange: watch(cover),
    setVideoMode: (on) => setVideoMode(on),
  };
  Object.assign(window, { __pearPlayerControls: controls });
  document.dispatchEvent(new Event('pear:player-controls'));
};

/** Starts the engine once the UI is in the page (`uiHost`) */
export const startEngine = async (uiHost: HTMLElement) => {
  exposeControls();
  readAccount();
  // The queue the previous page played, before the player is ready: a song
  // it was playing waits for it (see session.ts)
  startSession();
  await startPlayer(uiHost);
};
