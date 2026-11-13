// Rows, cards and sections. Every item has a visible ⋮ button for its menu,
// so nothing depends on right-click or long-press (long-press is only a
// shortcut to the same sheet).
import {
  createContext,
  createEffect,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  untrack,
  useContext,
} from 'solid-js';

import { Icon } from './icons';
import { go, openMenu, removedItems } from './router';
import { s } from './strings';

import { innertube, runEndpoint } from '../engine/ytm';
import { parseShelfContinuation } from '../model/parse';

import type { Item, Section } from '../model/types';

export const activate = (item: Item) => {
  if (item.open) go({ name: 'browse', target: item.open });
  else if (item.play) runEndpoint(item.play);
};

export const showMenu = (item: Item) =>
  openMenu({
    title: item.title,
    subtitle: item.subtitle,
    thumbnail: item.thumbnail,
    entries: item.menu,
    itemKey: item.queued ? undefined : item.key,
  });

const LONG_PRESS_MS = 500;

/** Long-press opens the menu too, without the browser's context menu */
const longPress = (item: Item) => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let fired = false;
  const cancel = () => clearTimeout(timer);
  return {
    onPointerDown: () => {
      fired = false;
      timer = setTimeout(() => {
        fired = true;
        showMenu(item);
      }, LONG_PRESS_MS);
    },
    onPointerUp: cancel,
    onPointerLeave: cancel,
    onPointerCancel: cancel,
    onContextMenu: (event: MouseEvent) => event.preventDefault(),
    onClick: () => {
      if (!fired) activate(item);
    },
  };
};

const MoreButton = (props: { item: Item }) => (
  <Show when={props.item.menu.length}>
    <button
      aria-label={s('more')}
      class="icon-button"
      onClick={(event) => {
        event.stopPropagation();
        showMenu(props.item);
      }}
    >
      <Icon name="more" />
    </button>
  </Show>
);

export const Row = (props: {
  item: Item;
  /** The playing song in the queue */
  current?: boolean;
  /** Durations show next to thumbnails too (the queue) */
  showDuration?: boolean;
}) => (
  <div
    class="row"
    classList={{ current: props.current }}
    role="button"
    {...longPress(props.item)}
  >
    <Show
      fallback={<span class="row-index">{props.item.index}</span>}
      when={props.item.thumbnail}
    >
      <img
        alt=""
        class="row-thumb"
        classList={{ round: props.item.round }}
        loading="lazy"
        src={props.item.thumbnail}
      />
    </Show>
    <div class="row-text">
      <div class="title">{props.item.title}</div>
      <div class="subtitle">{props.item.subtitle}</div>
    </div>
    <Show
      when={
        props.item.duration && (props.showDuration || !props.item.thumbnail)
      }
    >
      <span class="row-duration">{props.item.duration}</span>
    </Show>
    <MoreButton item={props.item} />
  </div>
);

export const Card = (props: { item: Item }) => (
  <div class="card" role="button" {...longPress(props.item)}>
    <img
      alt=""
      class="card-thumb"
      classList={{ round: props.item.round }}
      loading="lazy"
      src={props.item.thumbnail}
    />
    <div class="card-text">
      <div class="card-lines">
        <div class="title two-lines">{props.item.title}</div>
        <div class="subtitle">{props.item.subtitle}</div>
      </div>
      <MoreButton item={props.item} />
    </div>
  </div>
);

/** Calls `onVisible` when the end of a list scrolls into view */
export const Sentinel = (props: { onVisible: () => Promise<void> }) => {
  const [element, setElement] = createSignal<HTMLDivElement>();
  let busy = false;
  onMount(() => {
    const target = element();
    if (!target) return;
    const observer = new IntersectionObserver(async (entries) => {
      if (busy || !entries.some((entry) => entry.isIntersecting)) return;
      busy = true;
      try {
        await props.onVisible();
      } finally {
        busy = false;
      }
    });
    observer.observe(target);
    onCleanup(() => observer.disconnect());
  });
  return <div class="sentinel" ref={setElement} />;
};

/**
 * Lists still loading their items, so the page holds its own continuation
 * (recommendations after a playlist) until they are done, as YTM does
 */
export const PendingLists = createContext<{
  add(): void;
  done(): void;
}>();

export const SectionView = (props: { section: Section }) => {
  // A long playlist's further songs, loaded as its end comes into view
  const [more, setMore] = createSignal<Item[]>([]);
  const [token, setToken] = createSignal(
    untrack(() => props.section.continuation),
  );
  const pending = useContext(PendingLists);
  if (untrack(token)) {
    pending?.add();
    let counted = true;
    createEffect(() => {
      if (!token() && counted) {
        counted = false;
        pending?.done();
      }
    });
    onCleanup(() => counted && pending?.done());
  }
  const listItems = () =>
    [...props.section.items, ...more()].filter(
      (item) => !removedItems().has(item.key),
    );
  const loadMore = async (continuation: string) => {
    const next = parseShelfContinuation(
      await innertube('/browse', { continuation }),
    );
    setMore((list) => [...list, ...next.items]);
    setToken(next.continuation);
  };
  return (
    <section class="section">
      <Show when={props.section.title}>
        <div class="section-header">
          <h2>{props.section.title}</h2>
          <Show when={props.section.more}>
            {(more) => (
              <button
                class="text-button"
                onClick={() => go({ name: 'browse', target: more() })}
              >
                {s('more')}
              </button>
            )}
          </Show>
        </div>
      </Show>
      <Show when={props.section.layout === 'text'}>
        <p class="section-text">{props.section.text}</p>
      </Show>
      <Show when={props.section.layout === 'list' && props.section.rows}>
        {(rows) => (
          <div class="row-pages" style={{ '--rows': rows() }}>
            <For each={listItems()}>{(item) => <Row item={item} />}</For>
          </div>
        )}
      </Show>
      <Show when={props.section.layout === 'list' && !props.section.rows}>
        <div class="list">
          <For each={listItems()}>{(item) => <Row item={item} />}</For>
        </div>
        <Show keyed when={token()}>
          {(current) => <Sentinel onVisible={() => loadMore(current)} />}
        </Show>
      </Show>
      <Show
        when={
          props.section.layout === 'carousel' || props.section.layout === 'grid'
        }
      >
        <div
          classList={{
            carousel: props.section.layout === 'carousel',
            grid: props.section.layout === 'grid',
          }}
        >
          <For each={props.section.items}>{(item) => <Card item={item} />}</For>
        </div>
      </Show>
    </section>
  );
};
