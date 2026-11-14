// The account's library and the account itself: ratings, playlists, the
// account menu, and Pear's app menu (which replaces in-app-menu's title bar).
import { createSignal } from 'solid-js';

import { innertube, readSignedIn } from './innertube';

import {
  parseAccountMenu,
  parseAddToPlaylist,
  responseAction,
  responseMessage,
  type ResponseAction,
} from '../model/parse';

import type { AccountMenu, Subscription } from '../model/types';

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

/**
 * Subscriptions changed in this session. YouTube's artist page reports the
 * old state for a while after a change (seen: still "not subscribed" right
 * after subscribing, though the change had gone through), so a page opened
 * again shows what the user did, not that.
 */
const changedSubscriptions = new Map<string, boolean>();

/** Whether the user is subscribed: this session's change, else the page's */
export const isSubscribed = (subscription: Subscription) =>
  changedSubscriptions.get(subscription.channelId) ?? subscription.subscribed;

/** A subscription changed elsewhere (a menu's subscribe item) */
export const noteSubscriptions = (
  channelIds: string[] | undefined,
  on: boolean,
) => {
  for (const id of channelIds ?? []) changedSubscriptions.set(id, on);
};

/** Subscribes to an artist (`on`) or unsubscribes */
export const setSubscription = async (
  subscription: Subscription,
  on: boolean,
) => {
  const endpoint = on ? subscription.subscribe : subscription.unsubscribe;
  const body = (Object.values(endpoint ?? {})[0] as
    | Record<string, unknown>
    | undefined) ?? { channelIds: [subscription.channelId] };
  await innertube(
    on ? '/subscription/subscribe' : '/subscription/unsubscribe',
    body,
  );
  changedSubscriptions.set(subscription.channelId, on);
};

// ---- Account ---------------------------------------------------------

// The signed-in account (name, photo, its menu), read once the page is up
const [account, setAccount] = createSignal<AccountMenu | null>(null);
export { account };

/** The account's photo for the top bar */
export const avatarUrl = () => account()?.photo;

/** The signed-in account and YTM's account menu */
export const readAccountInfo = async () =>
  parseAccountMenu(await innertube('/account/account_menu', {}));

/** Reads the sign-in state and, signed in, the account */
export const readAccount = () => {
  if (!readSignedIn()) return;
  readAccountInfo()
    .then(setAccount)
    .catch(() => setAccount(null));
};

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
        on(channel: string, listener: () => void): void;
      };
    }
  ).ipcRenderer;

/**
 * Counts the app menu's rebuilds: Rust rebuilds it after a switch (a plugin
 * switched on gets a submenu of its options) and says so once it is done,
 * later than the click's own reply
 */
const [appMenuVersion, setAppMenuVersion] = createSignal(0);
ipc()?.on('refresh-in-app-menu', () => setAppMenuVersion((count) => count + 1));
export { appMenuVersion };

export const readAppMenu = async () =>
  ((await ipc()?.invoke('get-menu')) as { items: AppMenuItem[] } | null)
    ?.items ?? [];

/** Runs a menu item (checkboxes and radios switch), as the title bar does */
export const clickAppMenu = async (commandId: number) => {
  await ipc()?.invoke('peard:menu-event', commandId);
};
