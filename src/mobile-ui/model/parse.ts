// Turns innertube responses (browse, search) into the mobile UI's model.
//
// Only the renderers the pages need are read; anything else is skipped, so a
// new YTM renderer leaves a gap instead of breaking the page. Shapes checked
// against music.youtube.com on 2026-11-12.
import type {
  AccountItem,
  AccountMenu,
  AccountSwitcher,
  BrowseTarget,
  Chip,
  Confirm,
  Header,
  Item,
  ItemKind,
  Lyrics,
  MenuAction,
  MenuEntry,
  Page,
  PageType,
  Section,
  SettingCategory,
  SettingItem,
  Subscription,
  SwitchAccount,
  WatchTabs,
} from './types';

// A loose view of innertube JSON: every property may be another object
interface Obj {
  readonly [key: string]: Node;
}
type Node = Obj | undefined;

const str = (node: Node): string | undefined =>
  typeof (node as unknown) === 'string'
    ? (node as unknown as string)
    : undefined;

const arr = (node: Node): Obj[] =>
  Array.isArray(node) ? (node as unknown as Obj[]) : [];

/** The renderer inside a wrapper like `{ musicShelfRenderer: {...} }` */
const unwrap = (node: Node): [string, Obj] | undefined => {
  if (!node) return undefined;
  const key = Object.keys(node)[0];
  const value = key ? node[key] : undefined;
  return key && value ? [key, value] : undefined;
};

const text = (node: Node): string =>
  str(node?.simpleText) ??
  arr(node?.runs)
    .map((run) => str(run.text) ?? '')
    .join('');

const thumbnail = (node: Node): string | undefined => {
  const thumbs = arr(
    node?.musicThumbnailRenderer?.thumbnail?.thumbnails ??
      node?.croppedSquareThumbnailRenderer?.thumbnail?.thumbnails ??
      node?.thumbnails,
  );
  let best: Obj | undefined;
  for (const thumb of thumbs) {
    const width = Number(thumb.width ?? 0);
    if (!best || width > Number(best.width ?? 0)) best = thumb;
  }
  return str(best?.url);
};

const PAGE_TYPES: Record<string, PageType> = {
  MUSIC_PAGE_TYPE_ALBUM: 'album',
  MUSIC_PAGE_TYPE_AUDIOBOOK: 'album',
  MUSIC_PAGE_TYPE_PLAYLIST: 'playlist',
  MUSIC_PAGE_TYPE_ARTIST: 'artist',
  MUSIC_PAGE_TYPE_USER_CHANNEL: 'artist',
  MUSIC_PAGE_TYPE_LIBRARY_ARTIST: 'artist',
};

export const browseTarget = (
  endpoint: Node,
): BrowseTarget | undefined => {
  const browse = endpoint?.browseEndpoint;
  const browseId = str(browse?.browseId);
  if (!browseId) return undefined;
  const pageType = str(
    browse?.browseEndpointContextSupportedConfigs
      ?.browseEndpointContextMusicConfig?.pageType,
  );
  return {
    browseId,
    params: str(browse?.params),
    pageType: (pageType && PAGE_TYPES[pageType]) || 'other',
  };
};

const isPlayable = (endpoint: Node): endpoint is Obj =>
  !!(endpoint?.watchEndpoint || endpoint?.watchPlaylistEndpoint);

const kindOf = (open: BrowseTarget | undefined, play: Node): ItemKind => {
  if (open) {
    return open.pageType === 'other' ? 'other' : open.pageType;
  }
  const type = str(
    play?.watchEndpoint?.watchEndpointMusicSupportedConfigs
      ?.watchEndpointMusicConfig?.musicVideoType,
  );
  if (type === 'MUSIC_VIDEO_TYPE_ATV') return 'song';
  return play ? 'video' : 'other';
};

/**
 * What the new UI does itself instead of handing an item to YTM: liking
 * (YTM's toggle item does nothing when run outside its menu) and saving to a
 * playlist (the UI's own sheet instead of YTM's popup)
 */
const menuAction = (endpoint: Node): MenuAction | undefined => {
  const like = endpoint?.likeEndpoint;
  const status = str(like?.status);
  if (like && status) {
    return {
      kind: 'like',
      status,
      target: (like.target ?? {}) as unknown as Record<string, string>,
    };
  }
  // Playlist edits (remove from this playlist) run as API calls, so the page
  // can reload and show the result
  const edit = endpoint?.playlistEditEndpoint;
  if (edit) return { kind: 'edit', edit };
  const save = endpoint?.addToPlaylistEndpoint;
  if (save) {
    return {
      kind: 'save',
      videoId: str(save.videoId),
      playlistId: str(save.playlistId),
    };
  }
  return undefined;
};

const menu = (node: Node): MenuEntry[] => {
  const entries: MenuEntry[] = [];
  for (const wrapped of arr(node?.menuRenderer?.items)) {
    const [kind, item] = unwrap(wrapped) ?? [];
    if (!item) continue;
    // Downloads are the official app's
    if (kind === 'menuServiceItemDownloadRenderer') continue;
    // A toggle (like) shows and runs its current state
    const toggled = (item.isToggled as unknown) === true;
    const endpoint = toggled
      ? item.toggledServiceEndpoint
      : (item.navigationEndpoint ??
        item.serviceEndpoint ??
        item.defaultServiceEndpoint);
    const label = text(
      toggled ? item.toggledText : (item.text ?? item.defaultText),
    );
    if (!endpoint || !label) continue;
    const open = browseTarget(endpoint);
    entries.push({
      label,
      icon: str(
        (toggled ? item.toggledIcon : (item.icon ?? item.defaultIcon))
          ?.iconType,
      ),
      open,
      endpoint: open ? undefined : endpoint,
      action: open ? undefined : menuAction(endpoint),
      ytm: open ? undefined : wrapped,
    });
  }
  return entries;
};

/** A menu outside the responses parsed here (e.g. the queue's items) */
export const parseMenu = (node: unknown): MenuEntry[] => menu(node as Node);

const overlayPlay = (node: Node): Obj | undefined => {
  const endpoint =
    node?.musicItemThumbnailOverlayRenderer?.content?.musicPlayButtonRenderer
      ?.playNavigationEndpoint;
  return isPlayable(endpoint) ? endpoint : undefined;
};

let keySeed = 0;
const nextKey = (prefix: string) => `${prefix}-${keySeed++}`;

const twoRowItem = (item: Obj): Item => {
  const nav = item.navigationEndpoint;
  const open = browseTarget(nav);
  const play = isPlayable(nav) ? nav : overlayPlay(item.thumbnailOverlay);
  const kind = kindOf(open, isPlayable(nav) ? nav : undefined);
  return {
    key: nextKey('two-row'),
    kind,
    title: text(item.title),
    subtitle: text(item.subtitle),
    thumbnail: thumbnail(item.thumbnailRenderer),
    round: kind === 'artist',
    // Albums and playlists open their page; the cover's play button plays
    play: open ? undefined : play,
    open,
    menu: menu(item.menu),
  };
};

const listItem = (item: Obj): Item => {
  const columns = arr(item.flexColumns).map(
    (column) => column.musicResponsiveListItemFlexColumnRenderer?.text,
  );
  const titleRun = arr(columns[0]?.runs)[0];
  const titleNav = titleRun?.navigationEndpoint;
  const open = browseTarget(item.navigationEndpoint);
  const play = isPlayable(titleNav) ? titleNav : overlayPlay(item.overlay);
  const kind = kindOf(open, play);
  const subtitle = columns
    .slice(1)
    .map((column) => text(column))
    .filter(Boolean)
    .join(' • ');
  const fixed = arr(item.fixedColumns)[0]
    ?.musicResponsiveListItemFixedColumnRenderer?.text;
  return {
    key: nextKey('list'),
    kind,
    title: text(columns[0]),
    subtitle,
    thumbnail: thumbnail(item.thumbnail),
    round: kind === 'artist',
    index: text(item.index) || undefined,
    duration: text(fixed) || undefined,
    play: open ? undefined : play,
    open,
    menu: menu(item.menu),
  };
};

const anyItem = (wrapped: Obj): Item | undefined => {
  const [kind, item] = unwrap(wrapped) ?? [];
  if (!item) return undefined;
  if (kind === 'musicTwoRowItemRenderer') return twoRowItem(item);
  if (kind === 'musicResponsiveListItemRenderer') return listItem(item);
  return undefined;
};

const items = (node: Node) =>
  arr(node).flatMap((wrapped) => {
    const item = anyItem(wrapped);
    return item ? [item] : [];
  });

/** The search's top result card, as a one-item list */
const cardShelf = (shelf: Obj): Section => {
  const header = shelf.header?.musicCardShelfHeaderBasicRenderer;
  const titleNav = arr(shelf.title?.runs)[0]?.navigationEndpoint;
  const open = browseTarget(titleNav);
  const play = isPlayable(titleNav)
    ? titleNav
    : overlayPlay(shelf.thumbnailOverlay);
  const kind = kindOf(open, play);
  return {
    key: nextKey('card'),
    title: text(header?.title) || undefined,
    layout: 'list',
    items: [
      {
        key: nextKey('card-item'),
        kind,
        title: text(shelf.title),
        subtitle: text(shelf.subtitle),
        thumbnail: thumbnail(shelf.thumbnail),
        round: kind === 'artist',
        play: open ? undefined : play,
        open,
        menu: menu(shelf.menu),
      },
      ...items(shelf.contents),
    ],
  };
};

/**
 * A long playlist's next songs: a continuation item at the end of the list
 * (current), or the shelf's continuations (older responses)
 */
const shelfContinuation = (shelf: Obj): string | undefined => {
  const endpoint = arr(shelf.contents).at(-1)?.continuationItemRenderer
    ?.continuationEndpoint;
  // The user's own playlists run it among other commands
  const commands = [
    endpoint,
    ...arr(endpoint?.commandExecutorCommand?.commands),
  ];
  return (
    commands
      .map((command) => str(command?.continuationCommand?.token))
      .find(Boolean) ??
    str(arr(shelf.continuations)[0]?.nextContinuationData?.continuation)
  );
};

const sections = (contents: Node): Section[] => {
  const result: Section[] = [];
  // Search returns each result in its own itemSectionRenderer: merge runs
  let loose: Section | undefined;
  for (const wrapped of arr(contents)) {
    const [kind, shelf] = unwrap(wrapped) ?? [];
    if (!shelf) continue;
    if (kind !== 'itemSectionRenderer') loose = undefined;
    switch (kind) {
      case 'musicCarouselShelfRenderer': {
        const header = shelf.header?.musicCarouselShelfBasicHeaderRenderer;
        const list = items(shelf.contents);
        if (!list.length) break;
        result.push({
          key: nextKey('carousel'),
          title: text(header?.title) || undefined,
          // Song rows in a carousel (quick picks) read better as a list
          // Rows (quick picks) come as list items, cards as two-row items:
          // the renderer decides, not the first item's kind (a "listen
          // again" shelf can start with a song)
          layout:
            unwrap(arr(shelf.contents)[0])?.[0] ===
            'musicResponsiveListItemRenderer'
              ? 'list'
              : 'carousel',
          // Quick picks: columns of 4 songs, swiped through
          rows: Number(shelf.numItemsPerColumn ?? 0) || undefined,
          items: list,
          more: browseTarget(
            header?.moreContentButton?.buttonRenderer?.navigationEndpoint,
          ),
        });
        break;
      }
      case 'musicShelfRenderer':
      case 'musicPlaylistShelfRenderer': {
        const list = items(shelf.contents);
        if (!list.length) break;
        result.push({
          key: nextKey('shelf'),
          title: text(shelf.title) || undefined,
          layout: 'list',
          items: list,
          more: browseTarget(shelf.bottomEndpoint),
          continuation: shelfContinuation(shelf),
        });
        break;
      }
      case 'gridRenderer': {
        const list = items(shelf.items);
        if (!list.length) break;
        result.push({
          key: nextKey('grid'),
          title: text(shelf.header?.gridHeaderRenderer?.title) || undefined,
          layout: 'grid',
          items: list,
        });
        break;
      }
      case 'musicCardShelfRenderer':
        result.push(cardShelf(shelf));
        break;
      case 'musicDescriptionShelfRenderer': {
        const description = text(shelf.description);
        if (!description) break;
        result.push({
          key: nextKey('text'),
          title: text(shelf.header) || undefined,
          layout: 'text',
          items: [],
          text: description,
        });
        break;
      }
      case 'itemSectionRenderer': {
        const list = items(shelf.contents);
        if (!list.length) {
          // e.g. an itemSectionRenderer wrapping a gridRenderer
          result.push(...sections(shelf.contents));
          break;
        }
        if (!loose) {
          loose = { key: nextKey('items'), layout: 'list', items: [] };
          result.push(loose);
        }
        loose.items.push(...list);
        break;
      }
      default:
        break;
    }
  }
  return result;
};

const chips = (header: Node): Chip[] =>
  arr(header?.chipCloudRenderer?.chips).flatMap((wrapped) => {
    const chip = wrapped.chipCloudChipRenderer;
    if (!chip) return [];
    const nav = chip.navigationEndpoint;
    const search = nav?.searchEndpoint;
    const query = str(search?.query);
    return [
      {
        label: text(chip.text),
        selected: (chip.isSelected as unknown) === true,
        search:
          query !== undefined
            ? { query, params: str(search?.params) }
            : undefined,
        open: browseTarget(nav),
      },
    ];
  });

const buttonEndpoint = (node: Node) =>
  node?.buttonRenderer?.navigationEndpoint ?? undefined;

/** The first endpoint of `kind` anywhere under `node` */
const findEndpoint = (node: unknown, kind: string): Obj | undefined => {
  let found: Obj | undefined;
  JSON.stringify(node ?? null, (key, value: unknown) => {
    if (!found && key === kind) found = { [kind]: value as Obj };
    return value;
  });
  return found;
};

/** An artist header's subscribe button */
const subscription = (node: Node): Subscription | undefined => {
  const button = node?.subscribeButtonRenderer;
  const channelId = str(button?.channelId);
  if (!button || !channelId) return undefined;
  return {
    channelId,
    subscribed: (button.subscribed as unknown) === true,
    count: text(button.shortSubscriberCountText ?? button.subscriberCountText) || undefined,
    subscribeText: text(button.unsubscribedButtonText),
    subscribedText: text(button.subscribedButtonText),
    subscribe: findEndpoint(button.serviceEndpoints, 'subscribeEndpoint'),
    unsubscribe: findEndpoint(button.serviceEndpoints, 'unsubscribeEndpoint'),
  };
};

const header = (response: Obj, firstTab: Node): Header | undefined => {
  const immersive =
    response.header?.musicImmersiveHeaderRenderer ??
    response.header?.musicVisualHeaderRenderer;
  if (immersive) {
    return {
      title: text(immersive.title),
      subtitle: text(immersive.monthlyListenerCount) || undefined,
      thumbnail: thumbnail(
        immersive.thumbnail ?? immersive.foregroundThumbnail,
      ),
      wide: true,
      shuffle: buttonEndpoint(immersive.playButton),
      radio: buttonEndpoint(immersive.startRadioButton),
      subscription: subscription(immersive.subscriptionButton),
      menu: menu(immersive.menu),
    };
  }
  const simple = response.header?.musicHeaderRenderer;
  if (simple) {
    return { title: text(simple.title), wide: false, menu: [] };
  }
  // Albums and playlists: the header is the first section of the first tab;
  // the user's own playlists wrap it in an editable header
  const first = arr(firstTab?.contents)[0];
  const responsive =
    first?.musicResponsiveHeaderRenderer ??
    first?.musicEditablePlaylistDetailHeaderRenderer?.header
      ?.musicResponsiveHeaderRenderer;
  if (!responsive) return undefined;
  const buttons = arr(responsive.buttons);
  const playButton = buttons.find((b) => b.musicPlayButtonRenderer)
    ?.musicPlayButtonRenderer?.playNavigationEndpoint;
  const menuButton = buttons.find((b) => b.menuRenderer);
  const entries = menu(menuButton);
  const shuffle = entries.find((entry) => entry.icon === 'MUSIC_SHUFFLE');
  return {
    title: text(responsive.title),
    subtitle:
      [text(responsive.straplineTextOne), text(responsive.subtitle)]
        .filter(Boolean)
        .join(' • ') || undefined,
    detail: text(responsive.secondSubtitle) || undefined,
    thumbnail: thumbnail(responsive.thumbnail),
    wide: false,
    play: isPlayable(playButton) ? playButton : undefined,
    shuffle: shuffle?.endpoint,
    menu: entries,
  };
};

const continuationOf = (sectionList: Node) =>
  str(arr(sectionList?.continuations)[0]?.nextContinuationData?.continuation);

/** A /browse response: home, explore, library, album, playlist, artist */
export const parseBrowse = (data: unknown): Page => {
  const response = (data ?? {}) as Obj;
  const contents = response.contents;
  const single = contents?.singleColumnBrowseResultsRenderer;
  const two = contents?.twoColumnBrowseResultsRenderer;
  const firstTab = arr((single ?? two)?.tabs)[0]?.tabRenderer?.content
    ?.sectionListRenderer;
  const list = two ? two.secondaryContents?.sectionListRenderer : firstTab;
  return {
    header: header(response, two ? firstTab : undefined),
    chips: chips(list?.header),
    sections: sections(list?.contents),
    continuation: continuationOf(list),
  };
};

/** A /browse continuation (more shelves at the bottom of home) */
export const parseContinuation = (
  data: unknown,
): Pick<Page, 'sections' | 'continuation'> => {
  const response = (data ?? {}) as Obj;
  const list = response.continuationContents?.sectionListContinuation;
  return {
    sections: sections(list?.contents),
    continuation: continuationOf(list),
  };
};

/** A /search response */
export const parseSearch = (data: unknown): Page => {
  const response = (data ?? {}) as Obj;
  const tabs = arr(response.contents?.tabbedSearchResultsRenderer?.tabs);
  const list = (tabs.find((tab) => tab.tabRenderer?.selected) ?? tabs[0])
    ?.tabRenderer?.content?.sectionListRenderer;
  return {
    chips: chips(list?.header),
    // A filtered search's list ("Songs") continues in /search, not /browse
    sections: sections(list?.contents).map((section) =>
      section.continuation
        ? { ...section, continuationPath: '/search' as const }
        : section,
    ),
  };
};

/** A /next response: where the player's lyrics and related tabs load from */
export const parseWatchTabs = (data: unknown): WatchTabs => {
  const response = (data ?? {}) as Obj;
  const tabs = arr(
    response.contents?.singleColumnMusicWatchNextResultsRenderer?.tabbedRenderer
      ?.watchNextTabbedResultsRenderer?.tabs,
  );
  const result: WatchTabs = {};
  for (const tab of tabs) {
    const renderer = tab.tabRenderer;
    if ((renderer?.unselectable as unknown) === true) continue;
    const browseId = str(renderer?.endpoint?.browseEndpoint?.browseId);
    // MPLY: lyrics, MPTR: related (the tab order differs between clients)
    if (browseId?.startsWith('MPLY')) result.lyrics = browseId;
    else if (browseId?.startsWith('MPTR')) result.related = browseId;
  }
  return result;
};

/** A lyrics tab's /browse response; undefined when there are none */
export const parseLyrics = (data: unknown): Lyrics | undefined => {
  const response = (data ?? {}) as Obj;
  const shelf = arr(response.contents?.sectionListRenderer?.contents)[0]
    ?.musicDescriptionShelfRenderer;
  const lyrics = text(shelf?.description);
  if (!lyrics) return undefined;
  return { text: lyrics, source: text(shelf?.footer) || undefined };
};

/** A related tab's /browse response: shelves without a page around them */
export const parseSectionList = (data: unknown): Section[] => {
  const response = (data ?? {}) as Obj;
  return sections(response.contents?.sectionListRenderer?.contents);
};

export interface PlaylistOption {
  playlistId: string;
  title: string;
  thumbnail?: string;
  /** The edit adding the song; YTM does not say which playlists have it */
  add?: unknown;
}

/** A /playlist/get_add_to_playlist response: the playlists one can save to */
export const parseAddToPlaylist = (data: unknown): PlaylistOption[] => {
  const response = (data ?? {}) as Obj;
  const renderer = arr(response.contents)
    .map((content) => content.addToPlaylistRenderer)
    .find(Boolean);
  return arr(renderer?.playlists).flatMap((wrapped) => {
    const option = wrapped.playlistAddToOptionRenderer;
    const playlistId = str(option?.playlistId);
    if (!option || !playlistId) return [];
    return [
      {
        playlistId,
        title: text(option.title),
        thumbnail: thumbnail(unwrap(option.thumbnailRenderer)?.[1]?.thumbnail),
        add: option.addToPlaylistServiceEndpoint?.playlistEditEndpoint,
      },
    ];
  });
};

/** The next songs of a long list, from its continuation */
export const parseShelfContinuation = (
  data: unknown,
): { items: Item[]; continuation?: string } => {
  const response = (data ?? {}) as Obj;
  const appended = arr(response.onResponseReceivedActions)
    .map((action) => action.appendContinuationItemsAction?.continuationItems)
    .find(Boolean);
  if (appended) {
    return {
      items: items(appended),
      continuation: shelfContinuation({ contents: appended }),
    };
  }
  // A playlist's shelf, or a plain one (a library list, search results)
  // with the older `continuations`
  const shelf =
    response.continuationContents?.musicPlaylistShelfContinuation ??
    response.continuationContents?.musicShelfContinuation;
  return {
    items: items(shelf?.contents),
    continuation: shelf ? shelfContinuation(shelf) : undefined,
  };
};

/** A button YTM offers with its message ("Add anyway") */
export interface ResponseAction {
  label: string;
  edit: unknown;
}

/** The "Add anyway" kind of button in an action's response, if any */
export const responseAction = (data: unknown): ResponseAction | undefined => {
  let found: ResponseAction | undefined;
  const walk = (node: unknown, depth: number) => {
    if (found || depth > 12 || !node || typeof node !== 'object') return;
    const button = (node as Obj).notificationActionRenderer?.actionButton
      ?.buttonRenderer;
    const edit = button?.command?.playlistEditEndpoint;
    if (button && edit) {
      found = { label: text(button.text), edit };
      return;
    }
    for (const value of Object.values(node as Obj)) walk(value, depth + 1);
  };
  walk(data, 0);
  return found;
};

/** A message YTM attached to an action's response ("Saved to …") */
export const responseMessage = (data: unknown): string | undefined => {
  let found: string | undefined;
  const walk = (node: unknown, depth: number) => {
    if (found || depth > 12 || !node || typeof node !== 'object') return;
    const obj = node as Obj;
    const message =
      text(obj.responseText) ||
      text(arr(obj.confirmDialogRenderer?.dialogMessages)[0]) ||
      text(obj.confirmDialogRenderer?.title);
    if (message) {
      found = message;
      return;
    }
    for (const value of Object.values(obj)) walk(value, depth + 1);
  };
  walk(data, 0);
  return found;
};

// Premium upsell: a store page the new UI cannot show
const SKIPPED_ACCOUNT_ICONS = new Set(['UNLIMITED']);

/** YTM's account menu (/account/account_menu): the header and its items */
export const parseAccountMenu = (data: unknown): AccountMenu | null => {
  let header: Obj | undefined;
  const sections: AccountItem[][] = [];
  const walk = (node: unknown, depth: number) => {
    if (depth > 16 || !node || typeof node !== 'object') return;
    const obj = node as Obj;
    if (obj.activeAccountHeaderRenderer && !header) {
      header = obj.activeAccountHeaderRenderer;
      return;
    }
    const section = obj.multiPageMenuSectionRenderer;
    if (section) {
      const items: AccountItem[] = [];
      for (const entry of arr(section.items)) {
        const link = entry.compactLinkRenderer;
        const icon = str(link?.icon?.iconType);
        if (!link || (icon && SKIPPED_ACCOUNT_ICONS.has(icon))) continue;
        items.push({
          label: text(link.title),
          icon,
          open: browseTarget(link.navigationEndpoint),
          ytm: { compactLinkRenderer: link },
        });
      }
      if (items.length) sections.push(items);
      return;
    }
    for (const value of Object.values(obj)) walk(value, depth + 1);
  };
  walk(data, 0);
  if (!header) return null;
  const channel = sections
    .flat()
    .find((item) => item.icon === 'ACCOUNT_BOX')?.open;
  return {
    name: text(header.accountName),
    handle: text(header.channelHandle) || undefined,
    photo: thumbnail(header.accountPhoto),
    channel,
    sections,
  };
};

// ---- Settings and the account switcher ---------------------------------

/** A confirmDialogRenderer: its title, message and buttons */
const parseConfirm = (node: Node): Confirm | undefined => {
  const dialog = node?.confirmDialogRenderer;
  if (!dialog) return undefined;
  return {
    title: text(dialog.title),
    message:
      arr(dialog.dialogMessages)
        .map((message) => text(message))
        .join('\n')
        .trim() || undefined,
    confirm: text(dialog.confirmButton?.buttonRenderer?.text),
    cancel: text(dialog.cancelButton?.buttonRenderer?.text) || undefined,
  };
};

const parseSetting = (node: Obj): SettingItem | undefined => {
  const entry = unwrap(node);
  if (!entry) return undefined;
  const [kind, data] = entry;
  const title = text(data.title);
  const summary = text(data.summary) || undefined;
  switch (kind) {
    case 'settingBooleanRenderer': {
      const enable = data.enableServiceEndpoint;
      const disable = data.disableServiceEndpoint;
      if (!enable || !disable) return undefined;
      return {
        kind: 'toggle',
        id: str(data.itemId),
        title,
        summary,
        on: (data.enabled as unknown) === true,
        enable,
        disable,
        confirmOn: parseConfirm(data.confirmEnableDialog),
        confirmOff: parseConfirm(data.confirmDisableDialog),
      };
    }
    case 'settingSingleOptionMenuRenderer':
      return {
        kind: 'choice',
        id: str(data.itemId),
        title,
        options: arr(data.items).flatMap((item) => {
          const option = item.settingMenuItemRenderer;
          const endpoint = option?.updateServiceEndpoint;
          if (!option || !endpoint) return [];
          return [
            {
              label: str(option.name) ?? '',
              value: str(option.value),
              description: str(option.description),
              selected: (option.selected as unknown) === true,
              endpoint,
            },
          ];
        }),
      };
    case 'settingReadOnlyItemRenderer':
      return { kind: 'link', title, summary, endpoint: data.navigationEndpoint };
    case 'settingActionRenderer': {
      const endpoint = data.serviceEndpoint;
      if (!endpoint) return undefined;
      return {
        kind: 'action',
        title,
        summary,
        endpoint,
        confirm: parseConfirm(data.confirmDialog),
      };
    }
    default:
      return undefined;
  }
};

/** YTM's settings (/account/get_setting), by category */
export const parseSettings = (data: unknown): SettingCategory[] =>
  arr((data as Obj | undefined)?.items).flatMap((node) => {
    const category = node.settingCategoryCollectionRenderer;
    if (!category) return [];
    const items = arr(category.items).flatMap(
      (item) => parseSetting(item) ?? [],
    );
    return items.length ? [{ title: text(category.title), items }] : [];
  });

/** The account switcher (/getAccountSwitcherEndpoint, its JSON) */
export const parseAccountSwitcher = (data: unknown): AccountSwitcher => {
  const accounts: SwitchAccount[] = [];
  let googleAccount: string | undefined;
  let addAccountUrl: string | undefined;
  const walk = (node: unknown, depth: number) => {
    if (depth > 24 || !node || typeof node !== 'object') return;
    const obj = node as Obj;
    const account = obj.accountItem;
    if (account) {
      const tokens = arr(
        account.serviceEndpoint?.selectActiveIdentityEndpoint?.supportedTokens,
      );
      accounts.push({
        name: text(account.accountName),
        byline: text(account.accountByline) || undefined,
        handle: text(account.channelHandle) || undefined,
        photo: thumbnail(account.accountPhoto),
        selected: (account.isSelected as unknown) === true,
        signinUrl: tokens
          .map((token) => str(token.accountSigninToken?.signinUrl))
          .find(Boolean),
      });
      return;
    }
    if (obj.googleAccountHeaderRenderer && !googleAccount) {
      googleAccount = text(obj.googleAccountHeaderRenderer.name) || undefined;
    }
    const link = obj.compactLinkRenderer;
    if (link && str(link.icon?.iconType) === 'PERSON_ADD') {
      addAccountUrl = str(link.navigationEndpoint?.urlEndpoint?.url);
      return;
    }
    for (const value of Object.values(obj)) walk(value, depth + 1);
  };
  walk(data, 0);
  return { googleAccount, accounts, addAccountUrl };
};
