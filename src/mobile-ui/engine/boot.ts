// The page without YouTube Music's interface: YTM's scripts are stopped as
// the page loads, and YouTube's own player (base.js, the same one YTM, the
// YouTube site and embeds use) is loaded alone. The player fetches each song
// itself, runs its own BotGuard for the PO tokens its requests need, streams
// (SABR), plays ads and reports playback, so the UI only tells it what to
// play. Checked on a GeckoView tablet: docs/OWN-ENGINE.md.

/** YTM's interface: its Polymer app and the polyfills only it needs */
const YTM_INTERFACE =
  /\/music_polymer_inlined_html\.js|\/scheduler\.js|\/webcomponents-[\w-]+\.js|\/custom-elements-es5-adapter\.js/;

/**
 * Stops YTM's interface. Must run at document start (the injected bundle
 * does): parser-inserted scripts are retyped and detached as they are added,
 * before they run. Should one still run, its elements never upgrade.
 */
export const blockYtmInterface = () => {
  new MutationObserver((records) => {
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (
          node instanceof HTMLScriptElement &&
          node.src &&
          YTM_INTERFACE.test(node.src)
        ) {
          node.type = 'javascript/blocked';
          node.remove();
        }
      }
    }
  }).observe(document.documentElement, { childList: true, subtree: true });
  const define = customElements.define.bind(customElements);
  customElements.define = (name, constructor, options) => {
    if (/^(ytmusic|ytd|yt|tp-yt)-/.test(name)) return;
    define(name, constructor, options);
  };
  defineStandIns(define);
};

type Runs = { runs?: { text: string }[]; simpleText?: string } | undefined;

/**
 * `<yt-formatted-string>`, as far as plugins use it outside YTM's own
 * components: its `text` (runs or simpleText) shown as plain text. Plugins
 * whose views the UI hosts (synced-lyrics' lines) draw their text with it;
 * without YTM's interface it would be an unknown element showing nothing.
 */
class FormattedString extends HTMLElement {
  #text: Runs;

  connectedCallback() {
    // A value set before the element was upgraded
    if (Object.hasOwn(this, 'text')) {
      const value = (this as { text?: Runs }).text;
      delete (this as { text?: Runs }).text;
      this.text = value;
    }
  }

  get text() {
    return this.#text;
  }

  set text(value: Runs) {
    this.#text = value;
    this.textContent =
      value?.simpleText ?? value?.runs?.map((run) => run.text).join('') ?? '';
  }
}

/** YTM elements plugins rely on, defined past the block above */
const defineStandIns = (define: CustomElementRegistry['define']) => {
  if (!customElements.get('yt-formatted-string')) {
    define('yt-formatted-string', FormattedString);
  }
};

interface Ytcfg {
  get(key: string): unknown;
}

/** A value of the page's ytcfg (set by inline scripts in YTM's HTML) */
export const config = <T>(key: string) =>
  (window as { ytcfg?: Ytcfg }).ytcfg?.get(key) as T | undefined;

const domReady = () =>
  new Promise<void>((resolve) => {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', () => resolve(), {
        once: true,
      });
    } else {
      resolve();
    }
  });

/** The player's context: script URLs, API key, client version, flags */
const playerContext = () =>
  config<Record<string, Record<string, unknown>>>('WEB_PLAYER_CONTEXT_CONFIGS')
    ?.WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH;

/**
 * The client flag (an experiment, on for some visitors) that has the player
 * take its PO tokens from YTM's shared BotGuard instead of running its own.
 * YTM's interface starts that BotGuard (from `/att/get`'s challenge) and
 * hands it over on `window['havuokmhhs-…'].bevasrs`; without it the player
 * waits for it forever, and the stream stops after the part it gets without
 * a token (about 50 seconds).
 */
const SHARED_BOTGUARD_FLAG = '45760017';

/** The context, with the player running its own BotGuard */
const ownBotguardContext = (context: Record<string, unknown> | undefined) => {
  const flags = context?.serializedClientExperimentFlags;
  if (typeof flags !== 'string') return context;
  const pattern = new RegExp(`(^|&)${SHARED_BOTGUARD_FLAG}=[^&]*`);
  if (!pattern.test(flags)) return context;
  return {
    ...context,
    serializedClientExperimentFlags: flags.replace(
      pattern,
      `$1${SHARED_BOTGUARD_FLAG}=false`,
    ),
  };
};

interface PlayerApplication {
  create(...args: unknown[]): unknown;
  createAlternate?(...args: unknown[]): unknown;
}

const playerApplication = () =>
  (window as { yt?: { player?: { Application?: PlayerApplication } } }).yt
    ?.player?.Application;

let scriptPolicy: Pick<TrustedTypePolicy, 'createScriptURL'> | null | undefined;

/** The page enforces Trusted Types: script URLs go through a policy */
const scriptUrl = (url: string) => {
  scriptPolicy ??=
    window.trustedTypes?.createPolicy('pear-mobile-ui-player', {
      createScriptURL: (value: string) => value,
    }) ?? null;
  return (scriptPolicy?.createScriptURL(url) ?? url) as string;
};

let loading: Promise<PlayerApplication> | undefined;

/** Loads base.js (YTM's interface used to) and its stylesheet, once */
const loadPlayerScript = () =>
  (loading ??= (async () => {
    await domReady();
    const existing = playerApplication();
    if (existing) return existing;
    const context = playerContext();
    const js = context?.jsUrl;
    if (typeof js !== 'string') throw new Error('no player script in ytcfg');
    if (typeof context?.cssUrl === 'string') {
      const link = document.createElement('link');
      link.rel = 'stylesheet';
      link.href = context.cssUrl;
      document.head.append(link);
    }
    await new Promise<void>((resolve, reject) => {
      const script = document.createElement('script');
      script.onload = () => resolve();
      script.onerror = () => reject(new Error('player script failed'));
      script.src = scriptUrl(js);
      document.head.append(script);
    });
    const application = playerApplication();
    if (!application) throw new Error('player script has no Application');
    return application;
  })());

/**
 * Creates YouTube's player in `host`, as YTM does: no app OAuth token (the
 * player then uses the page's cookies), the music watch context, but its own
 * BotGuard (`ownBotguardContext`). The player element gets YTM's id,
 * `movie_player`, so the renderer and plugins, which look for it, find it.
 */
export const createPlayer = async (host: HTMLElement) => {
  const application = await loadPlayerScript();
  const args = [
    host,
    { args: {}, attrs: { id: 'movie_player' } },
    ownBotguardContext(playerContext()),
  ];
  if (application.createAlternate) application.createAlternate(...args);
  else application.create(...args);
  const element = host.querySelector<HTMLElement>('#movie_player');
  if (!element) throw new Error('player not created');
  return element;
};
