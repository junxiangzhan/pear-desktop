// What menu entries and play buttons ask for (YouTube's endpoints), run by
// the UI itself: YTM's interface, which used to run them, is not loaded.
import { innertube, signedIn, signIn } from './innertube';
import { noteSubscriptions, rate } from './library';
import {
  addToQueue,
  clearQueue,
  currentEntry,
  entryOfMenu,
  likeStatus,
  next,
  playEndpoint,
  queue,
  removeFromQueue,
  setLikeStatus,
} from './queue';

import type { Endpoint } from '../model/types';

/** What a command did, for the UI to tell */
export type CommandOutcome = 'done' | 'copied' | 'unsupported';

/** Plays a watch endpoint (a song, a playlist, a radio, a shuffle) */
export const runEndpoint = (endpoint: Endpoint) => {
  playEndpoint(endpoint).catch(console.error);
};

/** A share link for a shared entity (song, playlist, album, artist) */
const shareUrl = async (entity: string) => {
  const panel = await innertube('/share/get_share_panel', {
    serializedSharedEntity: entity,
  });
  let found: string | undefined;
  JSON.stringify(panel, (_, value: unknown) => {
    if (
      !found &&
      typeof value === 'string' &&
      /^https:\/\/(music\.youtube\.com|youtu\.be|www\.youtube\.com)\//.test(
        value,
      )
    ) {
      found = value;
    }
    return value;
  });
  return found;
};

const share = async (entity: string): Promise<CommandOutcome> => {
  const url = await shareUrl(entity);
  if (!url) return 'unsupported';
  if (navigator.share) {
    try {
      await navigator.share({ url });
      return 'done';
    } catch {
      // Dismissed, or not allowed: copy instead
    }
  }
  await navigator.clipboard.writeText(url);
  return 'copied';
};

/** The current song's like or dislike, toggled (YTM's player bar buttons) */
const toggleRating = async (status: 'LIKE' | 'DISLIKE') => {
  const entry = currentEntry();
  if (!entry) return;
  if (!signedIn()) {
    signIn();
    return;
  }
  const next = likeStatus() === status ? 'INDIFFERENT' : status;
  const before = likeStatus();
  setLikeStatus(next);
  try {
    await rate(next, { videoId: entry.videoId });
  } catch (error) {
    setLikeStatus(before);
    throw error;
  }
};

export const toggleLike = () => {
  toggleRating('LIKE').catch(console.error);
};
/** As YTM's player: a disliked song gives way to the next (kept queued) */
export const toggleDislike = () => {
  const entry = currentEntry();
  toggleRating('DISLIKE')
    .then(() =>
      // skip-disliked-songs may have moved on already
      likeStatus() === 'DISLIKE' && currentEntry() === entry
        ? next()
        : undefined,
    )
    .catch(console.error);
};

/**
 * A rating from a menu (a song's, an album's): for the playing song, its
 * like state follows, as with the player's buttons, and a dislike gives way
 * to the next song
 */
export const rateTarget = async (
  status: string,
  target: Record<string, string>,
) => {
  const entry = currentEntry();
  await rate(status, target);
  if (!entry || target.videoId !== entry.videoId) return;
  setLikeStatus(status);
  // skip-disliked-songs may have moved on already
  if (status === 'DISLIKE' && currentEntry() === entry) await next();
};

/** Runs one endpoint; what it did, or 'unsupported' */
export const runCommand = async (
  endpoint: Endpoint,
): Promise<CommandOutcome> => {
  if (endpoint.watchEndpoint || endpoint.watchPlaylistEndpoint) {
    await playEndpoint(endpoint);
    return 'done';
  }
  const add = endpoint.queueAddEndpoint as Record<string, unknown> | undefined;
  if (add) {
    await addToQueue(add);
    return 'done';
  }
  const remove = endpoint.removeFromQueueEndpoint as
    | { videoId?: string }
    | undefined;
  if (remove) {
    const entry =
      entryOfMenu(endpoint) ??
      queue().find((candidate) => candidate.videoId === remove.videoId);
    if (entry) removeFromQueue(entry);
    return 'done';
  }
  // "Dismiss queue": YouTube sends it as deleting the queue's own (QP…)
  // playlist, with a dismissQueueCommand inside. Only the local queue goes:
  // the deletion is never sent.
  const deletion = endpoint.deletePlaylistEndpoint as
    | { command?: Record<string, unknown> }
    | undefined;
  if (
    deletion?.command?.dismissQueueCommand ||
    Object.keys(endpoint).some((key) => /dismissQueue/i.test(key))
  ) {
    clearQueue();
    return 'done';
  }
  const like = endpoint.likeEndpoint as
    | { status?: string; target?: Record<string, string> }
    | undefined;
  if (like?.status) {
    await rateTarget(like.status, like.target ?? {});
    return 'done';
  }
  const feedback = endpoint.feedbackEndpoint as
    | { feedbackToken?: string }
    | undefined;
  if (feedback?.feedbackToken) {
    await innertube('/feedback', { feedbackTokens: [feedback.feedbackToken] });
    return 'done';
  }
  const subscribe = endpoint.subscribeEndpoint as
    | { channelIds?: string[]; params?: string }
    | undefined;
  // Noted as the artist page's button does: the page reports the old state
  // for a while
  if (subscribe) {
    await innertube('/subscription/subscribe', subscribe);
    noteSubscriptions(subscribe.channelIds, true);
    return 'done';
  }
  const unsubscribe = endpoint.unsubscribeEndpoint as
    | { channelIds?: string[]; params?: string }
    | undefined;
  if (unsubscribe) {
    await innertube('/subscription/unsubscribe', unsubscribe);
    noteSubscriptions(unsubscribe.channelIds, false);
    return 'done';
  }
  const shared = endpoint.shareEntityEndpoint as
    | { serializedShareEntity?: string }
    | undefined;
  if (shared?.serializedShareEntity) {
    return await share(shared.serializedShareEntity);
  }
  // Signed out, YouTube answers "sign in first" with a modal
  if (endpoint.modalEndpoint && !signedIn()) {
    signIn();
    return 'done';
  }
  const url = (endpoint.urlEndpoint as { url?: string } | undefined)?.url;
  if (url) {
    window.open(url, '_blank');
    return 'done';
  }
  return 'unsupported';
};

/** The endpoint a menu item (as YouTube sends it) runs */
const itemEndpoint = (wrapped: Endpoint): Endpoint | undefined => {
  const item = Object.values(wrapped)[0] as Record<string, unknown> | undefined;
  if (!item) return undefined;
  const endpoint =
    item.isToggled === true
      ? item.toggledServiceEndpoint
      : (item.navigationEndpoint ??
        item.serviceEndpoint ??
        item.defaultServiceEndpoint);
  return endpoint as Endpoint | undefined;
};

/** Runs a menu item as YouTube sent it (`{ menuServiceItemRenderer: … }`) */
export const runMenuItem = async (
  wrapped: Endpoint,
): Promise<CommandOutcome> => {
  const endpoint = itemEndpoint(wrapped);
  return endpoint ? await runCommand(endpoint) : 'unsupported';
};
