import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  Match,
  Show,
  Switch,
} from 'solid-js';

import { Icon } from './icons';
import { Row, SectionView, Sentinel, showMenu } from './items';
import { pluginEnabled } from './plugins';
import { Presence } from './presence';
import { back, closePlayer, go, openPlayer, playerOpen } from './router';
import { swipeDown } from './sheets';
import { s } from './strings';
import { AdVideoSlot } from './video';

import {
  adInfo,
  adShowing,
  adSkippable,
  currentEntry,
  currentTime,
  duration,
  innertube,
  isPlaying,
  loading,
  next,
  PLAYER_HOST_ID,
  nowPlaying,
  previous,
  queue,
  cycleRepeat,
  likeStatus,
  loadMoreQueue,
  moveQueueEntry,
  type QueueEntry,
  songVideoShown,
  videoRatio,
  queueHasMore,
  repeatMode,
  seekTo,
  shuffle,
  toggleDislike,
  toggleLike,
  togglePlay,
} from '../engine';
import { parseLyrics, parseSectionList, parseWatchTabs } from '../model/parse';

import type { Item } from '../model/types';

const formatTime = (seconds: number) => {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00';
  const total = Math.floor(seconds);
  const minutes = Math.floor(total / 60);
  const rest = String(total % 60).padStart(2, '0');
  if (minutes < 60) return `${minutes}:${rest}`;
  return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}:${rest}`;
};

const PlayPause = (props: { size?: number }) => (
  <button
    aria-label={isPlaying() ? s('pause') : s('play')}
    class="icon-button"
    onClick={(event) => {
      event.stopPropagation();
      togglePlay();
    }}
  >
    <Show
      fallback={<Icon name={isPlaying() ? 'pause' : 'play'} size={props.size} />}
      when={loading()}
    >
      {/* The song is on its way (the first one can take half a minute) */}
      <span
        aria-hidden="true"
        class="spinner"
        style={{ '--size': `${props.size ?? 24}px` }}
      />
    </Show>
  </button>
);

/**
 * A click from the player inside the bar (YouTube's skip button, reached
 * through the "Skip ad" label, or its ad link): it bubbles out of the slot
 * into the bar, which must not open the player for it
 */
const fromPlayer = (event: MouseEvent) =>
  event
    .composedPath()
    .some(
      (node) =>
        node instanceof HTMLElement &&
        (node.id === PLAYER_HOST_ID || node.classList.contains('skip-ad')),
    );

/**
 * "Skip ad": a <label> of the page (index.tsx) shown here through a slot, so
 * the user's tap reaches YouTube's skip button as their own click
 */
const SkipAd = (props: { where: 'mini' }) => (
  <Show when={adSkippable()}>
    {/* The box draws the backdrop: styling the slotted label itself did not
        hold on the tablet */}
    <span class={`skip-ad-box ${props.where}`}>
      <slot name={`skip-ad-${props.where}`} />
    </span>
  </Show>
);

const Rating = () => (
  <div class="rating">
    <button
      aria-label={s('like')}
      aria-pressed={likeStatus() === 'LIKE'}
      class="icon-button"
      onClick={toggleLike}
    >
      <Icon name={likeStatus() === 'LIKE' ? 'liked' : 'like'} />
    </button>
    <button
      aria-label={s('dislike')}
      aria-pressed={likeStatus() === 'DISLIKE'}
      class="icon-button"
      onClick={toggleDislike}
    >
      <Icon name={likeStatus() === 'DISLIKE' ? 'disliked' : 'dislike'} />
    </button>
  </div>
);

/**
 * What the player shows: during an ad, the ad's title and advertiser in
 * place of the song's (YTM's data still names the song that follows)
 */
const shown = (song: { title: string; artist: string }) =>
  adShowing()
    ? { title: adInfo().title || s('ad'), artist: adInfo().advertiser ?? '' }
    : song;

const AdBadge = () => (
  <Show when={adShowing()}>
    <span class="ad-badge">{s('ad')}</span>
  </Show>
);

/** The playing song as a queue item, for its menu */
const playingItem = (): Item | undefined => {
  const entry = currentEntry();
  if (!entry) return undefined;
  return {
    key: entry.key,
    kind: 'song',
    title: entry.title,
    subtitle: entry.artist,
    thumbnail: entry.thumbnail,
    round: false,
    duration: entry.duration,
    play: entry.endpoint,
    menu: entry.menu,
    queued: true,
  };
};

/**
 * The player bar, in three parts: the song (tap to open the player), the
 * playback controls in the middle, and the song's menu on the right
 */
export const MiniPlayer = () => (
  <Show when={nowPlaying()}>
    {(song) => (
      <div
        class="mini-player"
        classList={{ ad: adShowing() }}
        onClick={(event) => {
          if (!fromPlayer(event)) openPlayer();
        }}
        role="button"
      >
        <div
          class="mini-progress"
          style={{
            width: `${duration() ? (currentTime() / duration()) * 100 : 0}%`,
          }}
        />
        <div class="mini-info">
          <Show
            fallback={<img alt="" class="mini-art" src={song().thumbnail} />}
            when={adShowing()}
          >
            <AdVideoSlot class="mini-art" layer={0} name="video-mini" />
          </Show>
          <div class="row-text">
            <div class="title">
              <AdBadge />
              {shown(song()).title}
            </div>
            <div class="subtitle">{shown(song()).artist}</div>
          </div>
        </div>
        <div class="mini-controls">
          {/* Kept during an ad, disabled, so the controls do not shift */}
          <button
            aria-label={s('previous')}
            class="icon-button mini-previous"
            disabled={adShowing()}
            onClick={(event) => {
              event.stopPropagation();
              previous();
            }}
          >
            <Icon name="previous" />
          </button>
          <PlayPause />
          <button
            aria-label={s('next')}
            class="icon-button"
            onClick={(event) => {
              event.stopPropagation();
              next();
            }}
          >
            <Icon name="next" />
          </button>
        </div>
        <div class="mini-actions">
          {/* During an ad: skip, in the menu's place */}
          <Show when={adShowing()}>
            <SkipAd where="mini" />
          </Show>
          <Show when={!adShowing() && playingItem()}>
            {(item) => (
              <button
                aria-label={s('more')}
                class="icon-button"
                onClick={(event) => {
                  event.stopPropagation();
                  showMenu(item());
                }}
              >
                <Icon name="more" />
              </button>
            )}
          </Show>
        </div>
      </div>
    )}
  </Show>
);

const Seek = () => {
  const [seeking, setSeeking] = createSignal<number | null>(null);
  const position = () => seeking() ?? currentTime();
  const percent = () =>
    duration() ? Math.min(100, (position() / duration()) * 100) : 0;
  return (
    <div class="seek-area">
      <input
        aria-label="seek"
        class="seek"
        disabled={adShowing()}
        max={duration() || 0}
        min={0}
        onChange={(event) => {
          seekTo(Number(event.currentTarget.value));
          setSeeking(null);
        }}
        onInput={(event) => setSeeking(Number(event.currentTarget.value))}
        step={1}
        style={{ '--played': `${percent()}%` }}
        type="range"
        value={position()}
      />
      <div class="times">
        <span>{formatTime(position())}</span>
        <span>{formatTime(duration())}</span>
      </div>
    </div>
  );
};

type PanelTab = 'queue' | 'lyrics' | 'related';

/**
 * The queue, scrolled to the playing song whenever it is shown or the song
 * changes (as YTM's own queue does); its end loads the next songs
 */
const QueueList = () => {
  const [list, setList] = createSignal<HTMLDivElement>();
  // The song, not each update of its data (the queue changes it often)
  const playing = createMemo(() => nowPlaying()?.videoId);
  createEffect(() => {
    const element = list();
    const playingId = playing();
    if (!element || !playingId) return;
    requestAnimationFrame(() => {
      // Hidden behind another tab: that tab owns the scroller now
      if (element.offsetParent === null) return;
      const row = element.querySelector<HTMLElement>('.row.current');
      const scroller = element.closest<HTMLElement>('.panel-content');
      // The narrow layout scrolls the whole player, not the panel: leave it
      if (!row || !scroller || scroller.scrollHeight <= scroller.clientHeight) {
        return;
      }
      scroller.scrollTop =
        row.offsetTop - scroller.offsetTop - scroller.clientHeight / 3;
    });
  });
  // Dragging a row by its handle: where it was, where it would land, and
  // how far the finger moved (the rows in between slide to make room)
  const [drag, setDrag] = createSignal<{
    entry: QueueEntry;
    from: number;
    to: number;
    dy: number;
    height: number;
  } | null>(null);
  const offset = (index: number) => {
    const current = drag();
    if (!current) return 0;
    if (index === current.from) return current.dy;
    if (
      current.from < current.to &&
      index > current.from &&
      index <= current.to
    ) {
      return -current.height;
    }
    if (
      current.from > current.to &&
      index >= current.to &&
      index < current.from
    ) {
      return current.height;
    }
    return 0;
  };
  const startDrag = (event: PointerEvent, entry: QueueEntry, index: number) => {
    event.preventDefault();
    event.stopPropagation();
    const handle = event.currentTarget as HTMLElement;
    const row = handle.closest<HTMLElement>('.queue-item');
    if (!row) return;
    // The panel scrolls on wide screens, the whole player on narrow ones
    let scroller: HTMLElement | null = row.parentElement;
    while (scroller && scroller.scrollHeight <= scroller.clientHeight) {
      scroller = scroller.parentElement;
    }
    handle.setPointerCapture(event.pointerId);
    const startY = event.clientY;
    const startScroll = scroller?.scrollTop ?? 0;
    const height = row.offsetHeight;
    const count = queue().length;
    let lastY = startY;
    let frame = 0;
    const update = () => {
      const dy = lastY - startY + ((scroller?.scrollTop ?? 0) - startScroll);
      const to = Math.max(
        0,
        Math.min(count - 1, index + Math.round(dy / height)),
      );
      setDrag({ entry, from: index, to, dy, height });
    };
    // Near the scroller's edges, keep scrolling while held
    const edgeScroll = () => {
      if (scroller) {
        const box = scroller.getBoundingClientRect();
        if (lastY < box.top + 48) scroller.scrollTop -= 10;
        else if (lastY > box.bottom - 48) scroller.scrollTop += 10;
        update();
      }
      frame = requestAnimationFrame(edgeScroll);
    };
    const move = (moved: PointerEvent) => {
      lastY = moved.clientY;
      update();
    };
    const end = () => {
      cancelAnimationFrame(frame);
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', end);
      handle.removeEventListener('pointercancel', end);
      const done = drag();
      setDrag(null);
      const target = done && queue()[done.to];
      if (done && target && done.to !== done.from) {
        moveQueueEntry(done.entry, target);
      }
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', end);
    handle.addEventListener('pointercancel', end);
    update();
    frame = requestAnimationFrame(edgeScroll);
  };
  return (
    <div class="queue-list" ref={setList}>
      <For each={queue()}>
        {(entry, index) => (
          <div
            class="queue-item"
            classList={{ dragging: drag()?.entry === entry }}
            style={{ transform: `translateY(${offset(index())}px)` }}
          >
            <button
              aria-label={s('reorder')}
              class="drag-handle"
              on:pointerdown={(event) => startDrag(event, entry, index())}
            >
              <Icon name="drag" />
            </button>
            <Row
              current={entry === currentEntry()}
              item={{
                key: entry.key,
                kind: 'song',
                title: entry.title,
                subtitle: entry.artist,
                thumbnail: entry.thumbnail,
                round: false,
                duration: entry.duration,
                play: entry.endpoint,
                menu: entry.menu,
                queued: true,
              }}
              showDuration
            />
          </div>
        )}
      </For>
      <Show when={queueHasMore()}>
        <Sentinel onVisible={loadMoreQueue} />
      </Show>
    </div>
  );
};

/** synced-lyrics renders here (its own components and styles) while on */
const PluginLyrics = () => (
  <div class="plugin-lyrics">
    <slot name="synced-lyrics" />
  </div>
);

const LyricsView = (props: { browseId?: string }) => (
  <Show
    fallback={<YtmLyrics browseId={props.browseId} />}
    when={pluginEnabled('synced-lyrics')}
  >
    <PluginLyrics />
  </Show>
);

/** YouTube Music's own (plain) lyrics */
const YtmLyrics = (props: { browseId?: string }) => {
  const [lyrics] = createResource(
    () => props.browseId,
    async (browseId) => parseLyrics(await innertube('/browse', { browseId })),
  );
  return (
    <Switch fallback={<p class="message">{s('noLyrics')}</p>}>
      <Match when={lyrics.loading}>
        <p class="message">{s('loading')}</p>
      </Match>
      <Match when={lyrics()}>
        {(loaded) => (
          <div class="lyrics">
            <p>{loaded().text}</p>
            <Show when={loaded().source}>
              <p class="lyrics-source">{loaded().source}</p>
            </Show>
          </div>
        )}
      </Match>
    </Switch>
  );
};

const RelatedView = (props: { browseId?: string }) => {
  const [sections] = createResource(
    () => props.browseId,
    async (browseId) =>
      parseSectionList(await innertube('/browse', { browseId })),
  );
  return (
    <Switch fallback={<p class="message">{s('empty')}</p>}>
      <Match when={sections.loading}>
        <p class="message">{s('loading')}</p>
      </Match>
      <Match when={sections()?.length}>
        <For each={sections()}>
          {(section) => <SectionView section={section} />}
        </For>
      </Match>
    </Switch>
  );
};

const PANEL_TABS: { tab: PanelTab; label: () => string }[] = [
  { tab: 'queue', label: () => s('queue') },
  { tab: 'lyrics', label: () => s('lyrics') },
  { tab: 'related', label: () => s('related') },
];

const Panel = (props: { onTab: () => void }) => {
  const [tab, setTab] = createSignal<PanelTab>('queue');
  const [content, setContent] = createSignal<HTMLDivElement>();
  // Each tab keeps where it was scrolled to (the tabs share one scroller)
  const scrolled = new Map<PanelTab, number>();
  const [visited, setVisited] = createSignal(false);
  // Lyrics and related are tabs of YTM's own watch page: found once one of
  // them is opened, again for each new song
  const [tabs] = createResource(
    () => (visited() ? nowPlaying()?.videoId : undefined),
    async (videoId) => parseWatchTabs(await innertube('/next', { videoId })),
  );
  const show = (next: PanelTab) => {
    const scroller = content();
    if (scroller) scrolled.set(tab(), scroller.scrollTop);
    if (next !== 'queue') setVisited(true);
    setTab(next);
    requestAnimationFrame(() => {
      const target = content();
      if (target) target.scrollTop = scrolled.get(next) ?? 0;
    });
  };
  return (
    <aside class="player-panel">
      <div class="panel-tabs" role="tablist">
        <For each={PANEL_TABS}>
          {(entry) => (
            <button
              aria-selected={tab() === entry.tab}
              class="panel-tab"
              classList={{ active: tab() === entry.tab }}
              onClick={() => {
                if (entry.tab !== tab()) show(entry.tab);
                props.onTab();
              }}
              role="tab"
            >
              {entry.label()}
            </button>
          )}
        </For>
      </div>
      <div class="panel-content" ref={setContent}>
        {/* Kept while hidden: rebuilding it lost its rows and position */}
        <div hidden={tab() !== 'queue'}>
          <QueueList />
        </div>
        <Switch>
          <Match when={tab() === 'queue'}>{null}</Match>
          <Match when={tabs.loading}>
            <p class="message">{s('loading')}</p>
          </Match>
          <Match when={tab() === 'lyrics'}>
            <LyricsView browseId={tabs()?.lyrics} />
          </Match>
          <Match when={tab() === 'related'}>
            <RelatedView browseId={tabs()?.related} />
          </Match>
        </Switch>
      </div>
    </aside>
  );
};

/**
 * The full player. Wide screens show the panel (up next, lyrics, related)
 * beside the controls; narrow ones put it below them, a scroll away, with
 * the queue button as a shortcut.
 */
/**
 * The player page's cover: the song's picture, or the player's video (an ad,
 * the song's video in video mode). Plugins draw on it as they do on YTM's
 * player: ambient-mode's glow behind it, the visualizer over it,
 * video-toggle's switch on top (their mounts, slotted in).
 */
const Cover = (props: { image?: string; atTop: () => boolean }) => {
  const video = () => adShowing() || songVideoShown();
  return (
    <div
      class="player-cover"
      style={{ '--ratio': video() ? String(videoRatio()) : '1' }}
    >
      <Show when={pluginEnabled('ambient-mode')}>
        <div class="cover-layer">
          <slot name="ambient-mode" />
        </div>
      </Show>
      <Show
        fallback={
          <img
            alt=""
            class="player-art"
            src={props.image}
            {...swipeDown(() => back(), () => props.atTop())}
          />
        }
        when={video()}
      >
        <AdVideoSlot class="player-video" layer={1} name="video-player" />
      </Show>
      {/* Not over an ad: its own interface (the skip button) is tapped */}
      <Show when={!adShowing()}>
        <Show when={pluginEnabled('visualizer')}>
          <div class="cover-layer cover-clip">
            <slot name="visualizer" />
          </div>
        </Show>
        <Show when={pluginEnabled('video-toggle')}>
          <div class="cover-layer cover-switch">
            <slot name="video-toggle" />
          </div>
        </Show>
      </Show>
    </div>
  );
};

export const PlayerSheet = () => {
  const [layout, setLayout] = createSignal<HTMLDivElement>();
  const [panel, setPanel] = createSignal<HTMLDivElement>();
  const atTop = () => (layout()?.scrollTop ?? 0) <= 0;
  const showPanel = () =>
    panel()?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  // The queue cleared (or the first song failed to load): the UI closes
  // its player. Not while the first song loads: a tap opens the player
  // before its queue is there.
  createEffect(() => {
    if (!nowPlaying() && !loading()) closePlayer();
  });
  return (
    <Presence when={playerOpen()}>
      {() => (
      <div class="player-sheet" classList={{ ad: adShowing() }}>
        <header class="top-bar" {...swipeDown(() => back(), atTop)}>
          <button
            aria-label={s('collapse')}
            class="icon-button"
            onClick={() => back()}
          >
            <Icon name="collapse" />
          </button>
          <span class="top-title" />
          <button
            aria-label={s('queue')}
            class="icon-button panel-shortcut"
            onClick={showPanel}
          >
            <Icon name="queue" />
          </button>
        </header>
        <Show
          fallback={
            <Show
              fallback={<p class="message">{s('nothingPlaying')}</p>}
              when={loading()}
            >
              <div class="message">
                <span class="spinner" style={{ '--size': '40px' }} />
              </div>
            </Show>
          }
          when={nowPlaying()}
        >
          {(song) => (
            <div class="player-layout" ref={setLayout}>
              <div class="player-main">
                <Cover atTop={atTop} image={song().thumbnail} />
                <div class="player-heading">
                  <div class="player-titles">
                    <div class="player-title">
                      <AdBadge />
                      {shown(song()).title}
                    </div>
                    <Show
                      fallback={
                        <div class="subtitle">{shown(song()).artist}</div>
                      }
                      when={!adShowing() && song().artistOpen}
                    >
                      {(target) => (
                        <button
                          class="subtitle artist-link"
                          onClick={() => go({ name: 'browse', target: target() })}
                        >
                          {song().artist}
                        </button>
                      )}
                    </Show>
                  </div>
                  <Show when={!adShowing()}>
                    <Rating />
                  </Show>
                </div>
                <Seek />
                <div class="controls">
                  <button
                    aria-label={s('shuffle')}
                    class="icon-button"
                    onClick={shuffle}
                  >
                    <Icon name="shuffle" />
                  </button>
                  <button
                    aria-label={s('previous')}
                    class="icon-button large"
                    onClick={() => previous()}
                  >
                    <Icon name="previous" size={36} />
                  </button>
                  <div class="play-circle">
                    <PlayPause size={40} />
                  </div>
                  <button
                    aria-label={s('next')}
                    class="icon-button large"
                    onClick={() => next()}
                  >
                    <Icon name="next" size={36} />
                  </button>
                  <button
                    aria-label={s('repeat')}
                    aria-pressed={repeatMode() !== 'NONE'}
                    class="icon-button repeat"
                    classList={{ on: repeatMode() !== 'NONE' }}
                    onClick={cycleRepeat}
                  >
                    <Icon
                      name={repeatMode() === 'ONE' ? 'repeat-one' : 'repeat'}
                    />
                  </button>
                </div>
              </div>
              <div class="player-panel-wrap" ref={setPanel}>
                <Panel onTab={showPanel} />
              </div>
            </div>
          )}
        </Show>
      </div>
      )}
    </Presence>
  );
};
