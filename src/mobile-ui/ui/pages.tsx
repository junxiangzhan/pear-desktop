import {
  createResource,
  createSignal,
  For,
  Match,
  onCleanup,
  onMount,
  Show,
  Switch,
  untrack,
} from 'solid-js';

import { AvatarButton } from './account';
import { Icon } from './icons';
import { PendingLists, SectionView, showMenu } from './items';
import {
  back,
  go,
  openAccount,
  pages,
  replacePage,
  type Route,
  type Tab,
} from './router';
import { s } from './strings';

import { innertube, runEndpoint, signedIn, signIn } from '../engine/ytm';
import { parseBrowse, parseContinuation, parseSearch } from '../model/parse';

import type { BrowseTarget, Chip, Header, Page, Section } from '../model/types';

const browse = async (browseId: string, params?: string) =>
  parseBrowse(await innertube('/browse', { browseId, params }));

const TopBar = (props: { title?: string; canGoBack: boolean }) => (
  <header class="top-bar">
    <Show when={props.canGoBack}>
      <button aria-label={s('back')} class="icon-button" onClick={() => back()}>
        <Icon name="back" />
      </button>
    </Show>
    <h1 class="top-title">{props.title}</h1>
    <button
      aria-label={s('search')}
      class="icon-button"
      onClick={() => go({ name: 'search' })}
    >
      <Icon name="search" />
    </button>
    <Show when={!signedIn()}>
      <button class="text-button sign-in" onClick={signIn}>
        {s('signIn')}
      </button>
    </Show>
    <AvatarButton onClick={openAccount} />
  </header>
);

const SignInPrompt = () => (
  <div class="message">
    <p>{s('signInForLibrary')}</p>
    <button class="filled-button" onClick={signIn}>
      {s('signIn')}
    </button>
  </div>
);

const PageHeader = (props: { header: Header }) => (
  <div class="page-header" classList={{ wide: props.header.wide }}>
    <Show when={props.header.thumbnail}>
      <img alt="" class="page-header-art" src={props.header.thumbnail} />
    </Show>
    <div class="page-header-text">
      <h1>{props.header.title}</h1>
      <Show when={props.header.subtitle}>
        <div class="subtitle">{props.header.subtitle}</div>
      </Show>
      <Show when={props.header.detail}>
        <div class="subtitle">{props.header.detail}</div>
      </Show>
    </div>
    <div class="page-header-actions">
      <Show when={props.header.play}>
        {(play) => (
          <button class="filled-button" onClick={() => runEndpoint(play())}>
            <Icon name="play" />
            {s('play')}
          </button>
        )}
      </Show>
      <Show when={props.header.shuffle}>
        {(shuffle) => (
          <button class="tonal-button" onClick={() => runEndpoint(shuffle())}>
            <Icon name="shuffle" />
            {s('shuffle')}
          </button>
        )}
      </Show>
      <Show when={props.header.radio}>
        {(radio) => (
          <button class="tonal-button" onClick={() => runEndpoint(radio())}>
            <Icon name="radio" />
            {s('radio')}
          </button>
        )}
      </Show>
      <Show when={props.header.menu.length}>
        <button
          aria-label={s('more')}
          class="icon-button"
          onClick={() =>
            showMenu({
              key: 'header',
              kind: 'other',
              title: props.header.title,
              subtitle: props.header.subtitle ?? '',
              thumbnail: props.header.thumbnail,
              round: false,
              menu: props.header.menu,
            })
          }
        >
          <Icon name="more" />
        </button>
      </Show>
    </div>
  </div>
);

/** Loads more shelves when the end of the page scrolls into view */
const Continuation = (props: {
  token: string;
  onLoaded: (sections: Section[], next?: string) => void;
}) => {
  const [sentinel, setSentinel] = createSignal<HTMLDivElement>();
  let loading = false;
  onMount(() => {
    const element = sentinel();
    if (!element) return;
    const observer = new IntersectionObserver(async (entries) => {
      if (loading || !entries.some((entry) => entry.isIntersecting)) return;
      loading = true;
      const more = parseContinuation(
        await innertube('/browse', { continuation: props.token }),
      );
      props.onLoaded(more.sections, more.continuation);
      loading = false;
    });
    observer.observe(element);
    onCleanup(() => observer.disconnect());
  });
  return <div class="sentinel" ref={setSentinel} />;
};

/** A page's filter chips (home's moods and genres) */
const Chips = (props: { chips: Chip[]; onChip: (chip: Chip) => void }) => (
  <div class="chips">
    <For each={props.chips}>
      {(chip) => (
        <button
          class="chip"
          classList={{ selected: chip.selected }}
          onClick={() => props.onChip(chip)}
        >
          {chip.label}
        </button>
      )}
    </For>
  </div>
);

const PageBody = (props: { page: Page; onChip?: (chip: Chip) => void }) => {
  const [extra, setExtra] = createSignal<Section[]>([]);
  // Each page object is rendered once (the stack keys by route)
  const [token, setToken] = createSignal(
    untrack(() => props.page.continuation),
  );
  const [pendingLists, setPendingLists] = createSignal(0);
  const pending = {
    add: () => setPendingLists((count) => count + 1),
    done: () => setPendingLists((count) => count - 1),
  };
  return (
    <PendingLists.Provider value={pending}>
      <Show when={props.page.header}>
        {(header) => <PageHeader header={header()} />}
      </Show>
      <Show when={props.onChip && props.page.chips.length}>
        <Chips
          chips={props.page.chips}
          onChip={(chip) => props.onChip?.(chip)}
        />
      </Show>
      <Show
        fallback={<p class="message">{s('empty')}</p>}
        when={props.page.sections.length || props.page.header}
      >
        <For each={[...props.page.sections, ...extra()]}>
          {(section) => <SectionView section={section} />}
        </For>
      </Show>
      <Show keyed when={pendingLists() === 0 && token()}>
        {(current) => (
          <Continuation
            onLoaded={(sections, next) => {
              setExtra((list) => [...list, ...sections]);
              setToken(next);
            }}
            token={current}
          />
        )}
      </Show>
    </PendingLists.Provider>
  );
};

const Loaded = (props: {
  load: () => Promise<Page>;
  onChip?: (chip: Chip) => void;
}) => {
  const [page, { refetch }] = createResource(() => props.load());
  return (
    <Switch>
      <Match when={page.error as unknown}>
        <div class="message">
          <p>{s('error')}</p>
          <button class="tonal-button" onClick={() => refetch()}>
            {s('retry')}
          </button>
        </div>
      </Match>
      <Match when={page()}>
        {(loaded) => <PageBody onChip={props.onChip} page={loaded()} />}
      </Match>
      <Match when={page.loading}>
        <p class="message">{s('loading')}</p>
      </Match>
    </Switch>
  );
};

const SearchPage = (props: { query?: string; params?: string }) => {
  const [input, setInput] = createSignal<HTMLInputElement>();
  const [results] = createResource(
    () => (props.query ? { query: props.query, params: props.params } : null),
    async (search) => parseSearch(await innertube('/search', search)),
  );
  onMount(() => {
    if (!props.query) input()?.focus();
  });
  const submit = (query: string, params?: string) => {
    if (query.trim()) replacePage({ name: 'search', query, params });
  };
  return (
    <>
      <header class="top-bar">
        <button
          aria-label={s('back')}
          class="icon-button"
          onClick={() => back()}
        >
          <Icon name="back" />
        </button>
        <form
          class="search-form"
          onSubmit={(event) => {
            event.preventDefault();
            input()?.blur();
            submit(input()?.value ?? '');
          }}
        >
          <input
            class="search-input"
            enterkeyhint="search"
            placeholder={s('searchPlaceholder')}
            ref={setInput}
            type="search"
            value={props.query ?? ''}
          />
        </form>
      </header>
      <Show when={results()}>
        {(page) => (
          <>
            <div class="chips">
              <For each={page().chips}>
                {(chip) => (
                  <button
                    class="chip"
                    classList={{ selected: chip.selected }}
                    onClick={() => {
                      if (chip.search) {
                        submit(
                          chip.search.query || props.query || '',
                          chip.selected ? undefined : chip.search.params,
                        );
                      } else if (chip.open) {
                        go({ name: 'browse', target: chip.open });
                      }
                    }}
                  >
                    {chip.label}
                  </button>
                )}
              </For>
            </div>
            <For each={page().sections}>
              {(section) => <SectionView section={section} />}
            </For>
          </>
        )}
      </Show>
      <Show when={results.loading}>
        <p class="message">{s('loading')}</p>
      </Show>
    </>
  );
};

const TAB_IDS = {
  home: 'FEmusic_home',
  explore: 'FEmusic_explore',
  library: 'FEmusic_library_landing',
} as const;

const BrowsePage = (props: { target: BrowseTarget }) => (
  <>
    <TopBar canGoBack title="" />
    <Loaded load={() => browse(props.target.browseId, props.target.params)} />
  </>
);

export const RouteView = (props: { route: Route; depth: number }) => (
  <Switch>
    <Match
      when={
        props.route.name === 'home' ||
        props.route.name === 'explore' ||
        props.route.name === 'library'
      }
    >
      <TopBar
        canGoBack={props.depth > 0}
        title={s(props.route.name as keyof typeof TAB_IDS)}
      />
      <Show
        fallback={<SignInPrompt />}
        when={props.route.name !== 'library' || signedIn()}
      >
        <Loaded
          load={() =>
            browse(
              TAB_IDS[props.route.name as keyof typeof TAB_IDS],
              (props.route as { params?: string }).params,
            )
          }
          onChip={(chip) => {
            const tab = props.route.name as Tab;
            const target = chip.open;
            // A chip of the tab's own page filters it in place; tapping the
            // selected one clears the filter
            if (target?.browseId === TAB_IDS[tab]) {
              replacePage({
                name: tab,
                params: chip.selected ? undefined : target.params,
              });
            } else if (target) {
              go({ name: 'browse', target });
            }
          }}
        />
      </Show>
    </Match>
    <Match keyed when={props.route.name === 'search' && props.route}>
      {(route) =>
        route.name === 'search' && (
          <SearchPage params={route.params} query={route.query} />
        )
      }
    </Match>
    <Match keyed when={props.route.name === 'browse' && props.route}>
      {(route) =>
        route.name === 'browse' && <BrowsePage target={route.target} />
      }
    </Match>
  </Switch>
);

/**
 * The page stack: every page stays mounted (hidden under the top one), so
 * going back keeps its scroll position and data.
 */
export const PageStack = () => (
  <For each={pages()}>
    {(route, index) => (
      <div class="page" classList={{ hidden: index() !== pages().length - 1 }}>
        <RouteView depth={index()} route={route} />
      </div>
    )}
  </For>
);
