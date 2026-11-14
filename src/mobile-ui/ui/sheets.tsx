// Bottom sheets over everything: an item's menu, and "save to playlist".
import {
  createResource,
  createSignal,
  For,
  Match,
  Show,
  Switch,
} from 'solid-js';

import { Icon, type IconName } from './icons';
import { playAndOpen } from './items';
import { pluginMenuEntries } from './plugins';
import { Presence } from './presence';
import {
  back,
  backFrom,
  go,
  hideItem,
  menu,
  type MenuSheet,
  openSave,
  reloadTop,
  topPage,
  saveSheet,
  type SaveSheet,
  openMenu,
} from './router';
import { s } from './strings';
import { showToast } from './toast';

import {
  createPlaylist,
  editPlaylist,
  playlistOptions,
  rateTarget,
  runMenuItem,
  YtmError,
} from '../engine';

import type { PlaylistOption } from '../model/parse';
import type { MenuAction, MenuEntry } from '../model/types';

/** Swipe down on a sheet to close it */
export const swipeDown = (
  onSwipe: () => void,
  atTop: () => boolean = () => true,
) => {
  let startY: number | null = null;
  return {
    onTouchStart: (event: TouchEvent) => {
      startY = atTop() ? (event.touches[0]?.clientY ?? null) : null;
    },
    onTouchEnd: (event: TouchEvent) => {
      const endY = event.changedTouches[0]?.clientY;
      if (startY !== null && endY !== undefined && endY - startY > 80) {
        onSwipe();
      }
      startY = null;
    },
  };
};

const failed = (error: unknown) => {
  console.error('[pear-mobile-ui]', error);
  if (!(error instanceof YtmError)) {
    showToast(s('actionFailed'));
    return;
  }
  // YTM's own reason ("Already in this playlist"), with what it offers
  // instead ("Add anyway")
  const action = error.action;
  showToast(
    error.message,
    action && {
      label: action.label,
      run: () =>
        editPlaylist(action.edit)
          .then((message) => showToast(message ?? s('playlistUpdated')))
          .catch(failed),
    },
  );
};

const RATED: Record<string, 'liked' | 'unliked' | 'disliked'> = {
  LIKE: 'liked',
  INDIFFERENT: 'unliked',
  DISLIKE: 'disliked',
};

const runRating = (action: Extract<MenuAction, { kind: 'like' }>) =>
  rateTarget(action.status, action.target)
    .then(() =>
      showToast(
        // A playlist target is an album or playlist saved to the library
        action.target.playlistId
          ? s('libraryUpdated')
          : s(RATED[action.status] ?? 'liked'),
      ),
    )
    .catch(failed);

/** YTM changes the queue without a word: say what happened */
const queueToast = (endpoint?: Record<string, unknown>) => {
  if (!endpoint) return undefined;
  if ('removeFromQueueEndpoint' in endpoint) return s('removedFromQueue');
  const add = endpoint.queueAddEndpoint as
    | { queueInsertPosition?: string }
    | undefined;
  if (!add) return undefined;
  return add.queueInsertPosition === 'INSERT_AFTER_CURRENT_VIDEO'
    ? s('playingNext')
    : s('addedToQueue');
};

/** A playlist edit that takes a song out (ACTION_REMOVE_VIDEO) */
const removesVideo = (edit: unknown) =>
  ((edit as { actions?: { action?: string }[] }).actions ?? []).some(
    (change) => change.action === 'ACTION_REMOVE_VIDEO',
  );

const runEntry = (entry: MenuEntry, sheet: MenuSheet) => {
  const action = entry.action;
  if (entry.open) {
    const target = entry.open;
    go({ name: 'browse', target });
  } else if (action?.kind === 'like') {
    back(() => runRating(action));
  } else if (action?.kind === 'edit') {
    const itemKey = sheet.itemKey;
    const page = topPage();
    back(() =>
      editPlaylist(action.edit)
        .then((message) => {
          showToast(message ?? s('playlistUpdated'));
          // A song taken off the playlist on screen goes, in place; other
          // edits (or one from the queue) show on the page reloaded
          if (itemKey && removesVideo(action.edit)) hideItem(itemKey);
          else reloadTop(page);
        })
        .catch(failed),
    );
  } else if (action?.kind === 'local') {
    // A sub-menu replaces this one; anything else runs once it is gone
    if (action.keepOpen) action.run();
    else back(action.run);
  } else if (action?.kind === 'save') {
    back(() =>
      openSave({
        title: sheet.title,
        thumbnail: sheet.thumbnail,
        target: { videoId: action.videoId, playlistId: action.playlistId },
        fallback: entry.ytm,
      }),
    );
  } else if (
    entry.endpoint?.watchEndpoint ||
    entry.endpoint?.watchPlaylistEndpoint
  ) {
    // Shuffle, radio: played as the page's own buttons play, the player
    // page opening
    const endpoint = entry.endpoint;
    back(() => playAndOpen(endpoint));
  } else if (entry.ytm) {
    const item = entry.ytm;
    const done = queueToast(entry.endpoint);
    // After the sheet is gone: YTM may open a popup
    back(() => {
      runMenuItem(item)
        .then((outcome) => {
          if (outcome === 'unsupported') showToast(s('notSupported'));
          else if (outcome === 'copied') showToast(s('linkCopied'));
          else if (done) showToast(done);
        })
        .catch(failed);
    });
  }
};

/** YTM's icon names (iconType) to the UI's icons */
const MENU_ICONS: Record<string, IconName> = {
  MIX: 'mix',
  BROADCAST: 'mix',
  QUEUE_PLAY_NEXT: 'play-next',
  ADD_TO_REMOTE_QUEUE: 'queue',
  FAVORITE: 'like',
  UNFAVORITE: 'liked',
  ADD_TO_PLAYLIST: 'playlist-add',
  REMOVE_FROM_PLAYLIST: 'playlist-remove',
  ALBUM: 'album',
  ARTIST: 'person',
  SHARE: 'share',
  MUSIC_SHUFFLE: 'shuffle',
  SUBSCRIBE: 'subscribe',
  BOOKMARK_BORDER: 'bookmark',
  BOOKMARK: 'bookmarked',
  LIBRARY_ADD: 'bookmark',
  LIBRARY_SAVED: 'bookmarked',
  PEOPLE_GROUP: 'people',
  NOT_INTERESTED: 'not-interested',
  REMOVE_CIRCLE: 'remove-circle',
  REMOVE: 'delete',
  DELETE: 'delete',
  FLAG: 'flag',
  KEEP: 'pin',
  KEEP_OFF: 'pin',
  // The UI's own entries (plugins')
  PEAR_DOWNLOAD: 'download',
  PEAR_SPEED: 'speed',
  PEAR_QUALITY: 'quality',
  PEAR_CHECK: 'check',
};

/** Shown as the big buttons on top, in this order, as YTM's app does */
const QUICK = ['QUEUE_PLAY_NEXT', 'ADD_TO_PLAYLIST', 'SHARE'];

const MenuSheetBody = (props: { sheet: MenuSheet }) => {
  const quick = () =>
    QUICK.flatMap((icon) =>
      props.sheet.entries.filter((entry) => entry.icon === icon),
    );
  const rest = () => [
    ...props.sheet.entries.filter((entry) => !QUICK.includes(entry.icon ?? '')),
    // Plugins' features, where YTM's interface had them
    ...pluginMenuEntries(
      props.sheet.target,
      !!props.sheet.playing,
      (title, entries) => openMenu({ title, entries }),
    ),
  ];
  let scroller: HTMLDivElement | undefined;
  return (
    <>
      <div class="scrim" onClick={() => back()} />
      {/* A swipe that scrolls a long menu back up is not a close */}
      <div
        class="sheet"
        {...swipeDown(
          () => back(),
          () => !scroller || scroller.scrollTop <= 0,
        )}
      >
        <div class="sheet-handle" />
        <div class="row sheet-head">
          <Show when={props.sheet.thumbnail}>
            <img alt="" class="row-thumb" src={props.sheet.thumbnail} />
          </Show>
          <div class="row-text">
            <div class="title">{props.sheet.title}</div>
            <div class="subtitle">{props.sheet.subtitle}</div>
          </div>
        </div>
        <div
          class="sheet-scroll"
          ref={(element) => {
            scroller = element;
          }}
        >
          <Show when={quick().length}>
            <div class="menu-quick">
              <For each={quick()}>
                {(entry) => (
                  <button
                    class="menu-quick-item"
                    onClick={() => runEntry(entry, props.sheet)}
                  >
                    <span class="menu-quick-icon">
                      <Icon name={MENU_ICONS[entry.icon ?? ''] ?? 'more'} />
                    </span>
                    <span>{entry.label}</span>
                  </button>
                )}
              </For>
            </div>
          </Show>
          <For each={rest()}>
            {(entry) => (
              <button
                class="sheet-item with-icon"
                onClick={() => runEntry(entry, props.sheet)}
              >
                <span class="menu-icon">
                  <Show when={MENU_ICONS[entry.icon ?? '']}>
                    {(name) => <Icon name={name()} />}
                  </Show>
                </span>
                <span>{entry.label}</span>
              </button>
            )}
          </For>
        </div>
      </div>
    </>
  );
};

export const MenuSheetView = () => (
  <Presence when={menu()}>
    {(sheet) => <MenuSheetBody sheet={sheet} />}
  </Presence>
);

const NewPlaylist = (props: { sheet: SaveSheet }) => {
  const [open, setOpen] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  let input: HTMLInputElement | undefined;
  const create = async () => {
    const name = input?.value.trim();
    if (!name || busy()) return;
    setBusy(true);
    try {
      await createPlaylist(name, props.sheet.target);
      showToast(s('createdPlaylist', name));
      // The sheet may have been closed while it saved
      backFrom('save');
    } catch (error) {
      failed(error);
      setBusy(false);
    }
  };
  return (
    <Show
      fallback={
        <button class="row new-playlist" onClick={() => setOpen(true)}>
          <span class="row-thumb icon-thumb">
            <Icon name="add" />
          </span>
          <span class="title">{s('newPlaylist')}</span>
        </button>
      }
      when={open()}
    >
      <form
        class="new-playlist-form"
        onSubmit={(event) => {
          event.preventDefault();
          create().catch(failed);
        }}
      >
        <input
          class="search-input"
          enterkeyhint="done"
          maxLength={150}
          placeholder={s('playlistName')}
          ref={(element) => {
            input = element;
            requestAnimationFrame(() => element.focus());
          }}
        />
        <button class="filled-button" disabled={busy()} type="submit">
          {s('create')}
        </button>
      </form>
    </Show>
  );
};

const PlaylistRow = (props: { option: PlaylistOption }) => {
  const [busy, setBusy] = createSignal(false);
  const add = async () => {
    const option = props.option;
    if (!option.add || busy()) return;
    setBusy(true);
    try {
      const message = await editPlaylist(option.add);
      showToast(message ?? s('savedTo', option.title));
      // The sheet may have been closed while it saved, or by another row
      backFrom('save');
    } catch (error) {
      // Already there: YTM's message and its "Add anyway"
      backFrom('save', () => failed(error));
    }
  };
  return (
    <button class="row" disabled={busy()} onClick={() => add().catch(failed)}>
      <Show
        fallback={
          <span class="row-thumb icon-thumb">
            <Icon name="queue" />
          </span>
        }
        when={props.option.thumbnail}
      >
        <img
          alt=""
          class="row-thumb"
          loading="lazy"
          src={props.option.thumbnail}
        />
      </Show>
      <span class="row-text title">{props.option.title}</span>
    </button>
  );
};

/** Opens YTM's own "save to playlist" popup instead */
const YtmFallback = (props: { sheet: SaveSheet }) => (
  <Show when={props.sheet.fallback}>
    {(fallback) => (
      <button
        class="text-button"
        onClick={() =>
          back(() =>
            runMenuItem(fallback())
              .then(
                (outcome) =>
                  outcome === 'unsupported' && showToast(s('notSupported')),
              )
              .catch(failed),
          )
        }
      >
        {s('openInYtm')}
      </button>
    )}
  </Show>
);

export const SaveSheetView = () => (
  <Presence when={saveSheet()}>
    {(sheet) => {
      const [options, { refetch }] = createResource(() =>
        playlistOptions(sheet.target),
      );
      return (
        <>
          <div class="scrim" onClick={() => back()} />
          <div class="sheet tall">
            <div class="sheet-handle" {...swipeDown(() => back())} />
            <h2 class="sheet-title">{s('saveToPlaylist')}</h2>
            <div class="sheet-scroll">
              <NewPlaylist sheet={sheet} />
              <Switch>
                <Match when={options.loading}>
                  <p class="message">{s('loading')}</p>
                </Match>
                <Match when={options.error as unknown}>
                  <div class="message">
                    <p>{s('error')}</p>
                    <button class="tonal-button" onClick={() => refetch()}>
                      {s('retry')}
                    </button>
                    <YtmFallback sheet={sheet} />
                  </div>
                </Match>
                <Match when={options()}>
                  {(list) => (
                    <>
                      <For each={list()}>
                        {(option) => <PlaylistRow option={option} />}
                      </For>
                      {/* Nothing understood: YTM's own popup can still do it */}
                      <Show when={!list().length}>
                        <div class="message">
                          <YtmFallback sheet={sheet} />
                        </div>
                      </Show>
                    </>
                  )}
                </Match>
              </Switch>
            </div>
          </div>
        </>
      );
    }}
  </Presence>
);
