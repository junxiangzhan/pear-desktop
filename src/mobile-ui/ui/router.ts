// The new UI's navigation: a stack of pages under a bottom-nav tab, plus the
// layers over them (the player, a menu sheet). Every page or layer opened
// pushes one history entry, so the Android back button (GeckoSession.goBack)
// and the UI's own back/close buttons both go through popstate and pop
// exactly one layer.
import { createSignal } from 'solid-js';

import { installHistoryIsolation, pushBackEntry } from '../engine/history';

import type { SaveTarget } from '../engine';
import type { BrowseTarget, Endpoint, MenuEntry } from '../model/types';

export type Tab = 'home' | 'explore' | 'library';

export type Route =
  /** A tab's page; `params` picks one of its chips (home's moods) */
  | { name: Tab; params?: string }
  | { name: 'search'; query?: string; params?: string }
  | { name: 'browse'; target: BrowseTarget }
  /** YTM's settings (the account menu's) */
  | { name: 'settings' };

export interface MenuSheet {
  title: string;
  subtitle?: string;
  thumbnail?: string;
  entries: MenuEntry[];
  /** The page item it belongs to, hidden if an entry removes it */
  itemKey?: string;
  /** What it is about, for plugins' entries (the downloader) */
  target?: { videoId?: string; playlistId?: string };
  /** The playing song's menu: plugins add speed and quality there */
  playing?: boolean;
}

const [removedItems, setRemovedItems] = createSignal<ReadonlySet<string>>(
  new Set(),
);
export { removedItems };

/** Takes an item off its page (removed from its playlist), in place */
export const hideItem = (key: string) =>
  setRemovedItems((keys) => new Set([...keys, key]));

/** "Save to playlist" for one song or playlist */
export interface SaveSheet {
  title: string;
  thumbnail?: string;
  target: SaveTarget;
  /** YTM's own menu item, if the UI cannot list the playlists */
  fallback?: Endpoint;
}

/** A question before an action (sign out, clear the history) */
export interface Dialog {
  title: string;
  message?: string;
  confirm: string;
  cancel?: string;
  onConfirm: () => void;
}

type Layer = 'page' | 'player' | 'menu' | 'save' | 'account' | 'dialog';

const [pages, setPages] = createSignal<Route[]>([{ name: 'home' }]);
const [playerOpen, setPlayerOpen] = createSignal(false);
const [menu, setMenu] = createSignal<MenuSheet | null>(null);
const [saveSheet, setSaveSheet] = createSignal<SaveSheet | null>(null);
const [accountOpen, setAccountOpen] = createSignal(false);
const [dialog, setDialog] = createSignal<Dialog | null>(null);

export { accountOpen, dialog, menu, pages, playerOpen, saveSheet };

export const currentTab = (): Tab => {
  const root = pages()[0];
  return root.name === 'explore' || root.name === 'library'
    ? root.name
    : 'home';
};

const layers: Layer[] = [];
/** Runs once the pending back has popped its layer */
const afterBack: (() => void)[] = [];
let resetTo: Route | null = null;
/** Backs asked for whose popstate has not come yet */
let popping = 0;

const popLayer = () => {
  if (popping) popping--;
  if (resetTo) {
    popping = 0;
    // history.go(-n) from switchTab: one popstate for all the layers
    layers.length = 0;
    setMenu(null);
    setSaveSheet(null);
    setAccountOpen(false);
    setDialog(null);
    setPlayerOpen(false);
    setPages([resetTo]);
    resetTo = null;
  } else {
    switch (layers.pop()) {
      case 'menu':
        setMenu(null);
        break;
      case 'save':
        setSaveSheet(null);
        break;
      case 'account':
        setAccountOpen(false);
        break;
      case 'dialog':
        setDialog(null);
        break;
      case 'player':
        setPlayerOpen(false);
        break;
      case 'page':
        setPages((stack) => (stack.length > 1 ? stack.slice(0, -1) : stack));
        break;
      default:
        break;
    }
  }
  afterBack.splice(0).forEach((run) => run());
};

export const installRouter = () => installHistoryIsolation(popLayer);

const pushLayer = (layer: Layer) => {
  layers.push(layer);
  pushBackEntry();
};

/** Pops the top layer; `then` runs after it is gone */
export const back = (then?: () => void) => {
  if (!layers.length) {
    then?.();
    return;
  }
  if (then) afterBack.push(then);
  popping++;
  history.back();
};

/**
 * Pops `layer` if it is still the top one (once the backs on their way are
 * done): what an answer that comes later closes, which may be gone already
 * (the user closed it, or another answer did). `then` runs either way.
 */
export const backFrom = (layer: Layer, then?: () => void) => {
  if (layers[layers.length - 1 - popping] === layer) back(then);
  else then?.();
};

export const go = (route: Route) => {
  const open = () => {
    setPages((stack) => [...stack, route]);
    pushLayer('page');
  };
  // Opening a page from the player or a sheet closes them first
  if (layers.length && layers[layers.length - 1] !== 'page') {
    back(() => go(route));
    return;
  }
  open();
};

export const switchTab = (tab: Tab) => {
  if (!layers.length) {
    setPages([{ name: tab }]);
    return;
  }
  resetTo = { name: tab };
  history.go(-layers.length);
};

export const openPlayer = () => {
  if (playerOpen()) return;
  // The entry first: whatever reacts to the open (closing it again) finds
  // its layer
  pushLayer('player');
  setPlayerOpen(true);
};

export const openMenu = (sheet: MenuSheet) => {
  if (menu()) setMenu(sheet);
  else {
    setMenu(sheet);
    pushLayer('menu');
  }
};

/** Closes the player (and any sheet over it), e.g. when the queue is gone */
export const closePlayer = () => {
  if (!playerOpen()) return;
  if (!layers.includes('player')) setPlayerOpen(false);
  else if (layers[layers.length - 1] === 'player') back();
  else back(closePlayer);
};

/** The account and app menu sheet */
export const openAccount = () => {
  if (accountOpen()) return;
  setAccountOpen(true);
  pushLayer('account');
};

/** Asks first; the answer closes it (confirm runs once it is gone) */
export const openDialog = (next: Dialog) => {
  setDialog(next);
  pushLayer('dialog');
};

export const openSave = (sheet: SaveSheet) => {
  setSaveSheet(sheet);
  pushLayer('save');
};

/** The page on top of the stack */
export const topPage = () => pages()[pages().length - 1];

/**
 * Loads the top page again (after an edit changed what it shows); with
 * `route`, only if that page is still the top one (the user may have moved
 * on while the edit ran)
 */
export const reloadTop = (route?: Route) =>
  setPages((stack) => {
    const top = stack[stack.length - 1];
    if (route && top !== route) return stack;
    return [...stack.slice(0, -1), { ...top }];
  });

/** Replaces the top page (e.g. a new search on the search page) */
export const replacePage = (route: Route) =>
  setPages((stack) => [...stack.slice(0, -1), route]);
