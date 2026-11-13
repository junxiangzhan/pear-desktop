// What the mobile UI renders: plain data normalized from innertube responses
// (see parse.ts). Endpoints stay opaque and go back to the engine untouched.

export type Endpoint = Record<string, unknown>;

export type PageType = 'album' | 'playlist' | 'artist' | 'other';

export interface BrowseTarget {
  browseId: string;
  params?: string;
  pageType: PageType;
}

export type ItemKind =
  | 'song'
  | 'video'
  | 'album'
  | 'playlist'
  | 'artist'
  | 'other';

export interface MenuEntry {
  label: string;
  icon?: string;
  /** A page of the new UI, or YTM's own menu item for the engine to run */
  open?: BrowseTarget;
  endpoint?: Endpoint;
  /** Handled by the new UI itself */
  action?: MenuAction;
  /** The item as YTM sent it (`{ menuServiceItemRenderer: {...} }`) */
  ytm?: Endpoint;
}

export type MenuAction =
  | { kind: 'like'; status: string; target: Record<string, string> }
  | { kind: 'save'; videoId?: string; playlistId?: string }
  | { kind: 'edit'; edit: unknown };

export interface Item {
  key: string;
  kind: ItemKind;
  title: string;
  subtitle: string;
  thumbnail?: string;
  /** Artists are shown round */
  round: boolean;
  index?: string;
  duration?: string;
  /** Tap target: playback first, otherwise a page */
  play?: Endpoint;
  open?: BrowseTarget;
  menu: MenuEntry[];
  /** An entry of the queue, not of a page */
  queued?: boolean;
}

/** An entry of YTM's account menu (your channel, history, sign out…) */
export interface AccountItem {
  label: string;
  /** YTM's iconType (ACCOUNT_BOX, SETTINGS…) */
  icon?: string;
  /** A page the new UI shows itself (your channel, history) */
  open?: BrowseTarget;
  /** The item as YTM sent it (`{ compactLinkRenderer: {...} }`) */
  ytm: Endpoint;
}

/** The signed-in account and YTM's account menu, in its sections */
export interface AccountMenu {
  name: string;
  handle?: string;
  photo?: string;
  /** The account's own channel, opened from the header */
  channel?: BrowseTarget;
  sections: AccountItem[][];
}

export interface Section {
  key: string;
  title?: string;
  /** `text`: a paragraph (an artist's description), no items */
  layout: 'carousel' | 'list' | 'grid' | 'text';
  items: Item[];
  text?: string;
  more?: BrowseTarget;
  /** More items of a long list (a playlist's songs) */
  continuation?: string;
  /** A list laid out in columns of this many rows, scrolled sideways */
  rows?: number;
}

export interface Chip {
  label: string;
  selected: boolean;
  /** A search with its filter params */
  search?: { query: string; params?: string };
  open?: BrowseTarget;
}

export interface Header {
  title: string;
  subtitle?: string;
  detail?: string;
  thumbnail?: string;
  /** Artist pages: a wide banner instead of a square cover */
  wide: boolean;
  play?: Endpoint;
  shuffle?: Endpoint;
  radio?: Endpoint;
  menu: MenuEntry[];
}

export interface Page {
  header?: Header;
  chips: Chip[];
  sections: Section[];
  continuation?: string;
}

/** The tabs under the player, from a /next response */
export interface WatchTabs {
  lyrics?: string;
  related?: string;
}

export interface Lyrics {
  text: string;
  source?: string;
}
