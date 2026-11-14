// What plays, kept across page loads, so playback goes on after the page is
// loaded again: Android ends Gecko's content process when memory runs low,
// even while it plays in the background, and GeckoPage.recover loads the
// page again (docs/OWN-ENGINE.md, rounds 15 and 16). YTM's interface comes
// back on its watch page and plays on; this does the same for the queue
// kept here (queue.ts), from about where the song was.
//
// What a load does with it:
// - The page went away while a song played (no pause of the user's, no
//   unload: the process was ended, or the app with it) and it was saved in
//   the last RECOVERY_WINDOW: the queue comes back and the song plays on from
//   where it was. Whatever "Resume last song when app starts" says, as YTM's
//   interface plays on there (it reloads its watch page): this is the page
//   coming back, not the app starting.
// - Otherwise (paused, an unload such as a reload or an account switch, or
//   long ago) with that option on: the queue comes back with its song cued
//   where it was, paused, to play with a tap. The app never starts playing
//   by itself, and a pause stays a pause.
import { createEffect, createRoot, on, untrack } from 'solid-js';

import { loading, playIntended, songLength, songPosition } from './player';
import {
  currentEntry,
  queue,
  restoreQueue,
  savedQueue,
  type QueueEntry,
  type SavedQueue,
} from './queue';

const QUEUE_KEY = 'pear-mobile-ui:queue';
const POSITION_KEY = 'pear-mobile-ui:position';
/** Bumped when what is saved changes shape: older saves are ignored */
const VERSION = 1;
/**
 * Kept around the current song: a radio grows without end, and a song with
 * its menu is about 7 KB saved
 */
const KEEP_BEFORE = 25;
const KEEP = 100;
/**
 * YouTube's logging, in every endpoint: half of a queue's size, and not
 * read by the UI
 */
const UNSAVED = new Set(['clickTrackingParams', 'trackingParams']);
/**
 * A load this soon after the last save of a song playing is the page coming
 * back: the content process is ended and the page loaded again within
 * seconds (the page then starts the player, up to half a minute)
 */
const RECOVERY_WINDOW = 2 * 60_000;
/** How often the song's position is saved while it plays */
const SAVE_EVERY = 3000;

interface StoredQueue extends SavedQueue {
  v: number;
  /** The index in the whole queue of the first entry kept */
  first: number;
}

interface StoredPosition {
  v: number;
  /** The song's index in the whole queue, and its id to check it by */
  index: number;
  videoId: string;
  time: number;
  duration?: number;
  /** Playing, or meant to (loading, buffering), not paused */
  playing: boolean;
  /** The page was left (unloaded), not ended */
  unloaded?: boolean;
  /** Date.now() of the save */
  at: number;
}

const read = <T extends { v: number }>(key: string): T | null => {
  try {
    const raw = localStorage.getItem(key);
    const value = raw ? (JSON.parse(raw) as T) : null;
    return value?.v === VERSION ? value : null;
  } catch {
    return null;
  }
};

const write = (key: string, value: unknown) => {
  try {
    if (value === null) localStorage.removeItem(key);
    else {
      localStorage.setItem(
        key,
        JSON.stringify(value, (name, field: unknown) =>
          UNSAVED.has(name) ? undefined : field,
        ),
      );
    }
  } catch (error) {
    // Storage full or blocked: the next load starts without it
    console.warn('[pear-mobile-ui] saving the queue failed', error);
  }
};

/** Puts back what the previous page played (see the top of this file) */
const restore = () => {
  const position = read<StoredPosition>(POSITION_KEY);
  const saved = read<StoredQueue>(QUEUE_KEY);
  if (!position || !saved) return;
  const interrupted =
    position.playing &&
    !position.unloaded &&
    Date.now() - position.at < RECOVERY_WINDOW;
  const resumeOnStart =
    window.mainConfig?.get('options.resumeOnStart') !== false;
  if (!interrupted && !resumeOnStart) return;
  let index = position.index - saved.first;
  if (saved.entries[index]?.videoId !== position.videoId) {
    index = saved.entries.findIndex(
      (entry) => entry.videoId === position.videoId,
    );
  }
  if (index < 0) return;
  const restored = restoreQueue(saved, {
    index,
    start: position.time,
    duration: position.duration,
    play: interrupted,
  });
  if (restored) {
    console.info(
      `[pear-mobile-ui] restored the queue at ${position.videoId}, ${Math.round(position.time)} s, ${interrupted ? 'playing on' : 'paused'}`,
    );
  }
};

/** What is played now, or meant to be */
const playing = () => playIntended() || loading();

/** The position last saved: a seek while paused is saved too */
let savedTime = -1;

const savePosition = (unloaded = false) => {
  const entry = untrack(currentEntry);
  if (!entry) return;
  savedTime = songPosition();
  const position: StoredPosition = {
    v: VERSION,
    index: untrack(queue).indexOf(entry),
    videoId: entry.videoId,
    time: savedTime,
    duration: songLength() || undefined,
    playing: untrack(playing),
    at: Date.now(),
  };
  if (unloaded) position.unloaded = true;
  write(POSITION_KEY, position);
};

/** Keeps the queue and the song's position saved as they change */
const keepSaved = () => {
  let lastEntries: QueueEntry[] | undefined;
  let lastFirst = -1;
  let lastRepeat: string | undefined;
  createRoot(() => {
    createEffect(() => {
      const saved = savedQueue();
      const entry = currentEntry();
      const index = entry ? saved.entries.indexOf(entry) : -1;
      if (index < 0) {
        // Dismissed: nothing to come back to
        lastEntries = undefined;
        write(QUEUE_KEY, null);
        write(POSITION_KEY, null);
        return;
      }
      const first = Math.max(
        0,
        Math.min(index - KEEP_BEFORE, saved.entries.length - KEEP),
      );
      if (
        saved.entries !== lastEntries ||
        first !== lastFirst ||
        saved.repeat !== lastRepeat
      ) {
        lastEntries = saved.entries;
        lastFirst = first;
        lastRepeat = saved.repeat;
        const stored: StoredQueue = {
          ...saved,
          entries: saved.entries.slice(first, first + KEEP),
          v: VERSION,
          first,
        };
        write(QUEUE_KEY, stored);
      }
      savePosition();
    });
    // A pause, a play, a load: saved at once
    createEffect(on(playing, () => savePosition(), { defer: true }));
  });
  setInterval(() => {
    if (untrack(playing) || songPosition() !== savedTime) savePosition();
  }, SAVE_EVERY);
  // A reload, an account switch, leaving for a sign-in page: not an end of
  // the page's process, so no playing on by itself after it
  window.addEventListener('pagehide', () => savePosition(true));
};

/** At the engine's start: puts back the previous page's queue, then saves */
export const startSession = () => {
  restore();
  keepSaved();
};
