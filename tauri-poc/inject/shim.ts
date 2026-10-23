// Stand-ins for the globals that `src/preload.ts` exposes through Electron's
// contextBridge, so the unmodified renderer bundle can run inside a Tauri
// webview. Only what the renderer side touches is implemented.
//
// This module must not import `virtual:plugins`: that pulls in every plugin,
// and their top-level code would run before the globals below exist.
// `plugin-stubs.ts` hands the plugin list over once the globals are in place.
import { deepmergeCustom } from 'deepmerge-ts';

import { defaultConfig } from '@/config/defaults';
import musicPlayerCss from '@/music-player.css?inline';

import type { PluginConfig } from '@/types/plugins';

type Listener = (event: unknown, ...args: unknown[]) => void;
type Store = Record<string, unknown>;

declare global {
  interface Window {
    __PEAR_STORED_CONFIG__?: Store;
    __pearIpcEmit?: (channel: string, ...args: unknown[]) => void;
    __TAURI_INTERNALS__?: {
      invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
    };
  }
}

const LOG_PREFIX = '[pear-tauri-poc]';

// Plugins with a backend half (Electron main process) that is not ported yet.
// They are forced off so their renderer half does not wait on missing IPC.
const UNPORTED_BACKEND_PLUGINS = new Set([
  'amuse',
  'api-server',
  'auth-proxy-adapter',
  'captions-selector',
  'crossfade',
  'discord',
  'do-not-track',
  'downloader',
  'in-app-menu',
  'lumiastream',
  'music-together',
  'notifications',
  'picture-in-picture',
  'precise-volume',
  'quality-changer',
  'scrobbler',
  'shortcuts',
  'sponsorblock',
  'synced-lyrics',
  'taskbar-mediacontrol',
  'touchbar',
  'transparent-player',
  'tuna-obs',
]);

// PoC defaults that differ from the plugins' own. Unlike the list above these
// are only defaults: an explicit entry in config.json still wins.
// performance-improvement: its cpu-tamer wakes the main thread every frame via
// a 1ms CSS animation and dropped 550 frames in 20s of scrolling (0 when off).
const POC_DEFAULT_OVERRIDES: Record<string, Partial<PluginConfig>> = {
  'performance-improvement': { enabled: false },
};

const deepmerge = deepmergeCustom({ mergeArrays: false });

let store = deepmerge(
  defaultConfig,
  window.__PEAR_STORED_CONFIG__ ?? {},
) as Store;

const getPath = (key: string): unknown =>
  key
    .split('.')
    .reduce<unknown>(
      (node, part) => (node as Store | undefined)?.[part],
      store,
    );

const setPath = (key: string, value: unknown) => {
  const parts = key.split('.');
  const last = parts.pop()!;
  let node = store;
  for (const part of parts) {
    node[part] ??= {};
    node = node[part] as Store;
  }
  node[last] = value;
  persist();
};

const persist = () => {
  window.__TAURI_INTERNALS__?.invoke('save_config', { config: store }).catch(
    (error: unknown) => console.error(LOG_PREFIX, 'save_config failed', error),
  );
};

const storedPlugins = () => (store.plugins ?? {}) as Record<string, Store>;

type PluginStubs = Record<string, { config?: PluginConfig } | undefined>;
const { promise: pluginStubs, resolve: resolvePluginStubs } =
  Promise.withResolvers<PluginStubs>();
export const providePluginStubs = (allPlugins: () => Promise<PluginStubs>) =>
  allPlugins().then(resolvePluginStubs);

const pluginConfig = async (id: string) => {
  const stub = (await pluginStubs)[id];
  const config = deepmerge(
    stub?.config ?? { enabled: false },
    POC_DEFAULT_OVERRIDES[id] ?? {},
    storedPlugins()[id] ?? {},
  ) as PluginConfig;
  if (UNPORTED_BACKEND_PLUGINS.has(id)) config.enabled = false;
  return config;
};

const setPluginConfig = async (id: string, value: object) => {
  const before = await pluginConfig(id);
  setPath(`plugins.${id}`, deepmerge(storedPlugins()[id] ?? {}, value));
  const after = await pluginConfig(id);

  // Mirrors the config watcher in `src/index.ts#initHook`
  if (before.enabled !== after.enabled) {
    emit(after.enabled ? 'plugin:enable' : 'plugin:unload', id);
  }
  emit('config-changed', id, after);
};

// --- ipcRenderer ---------------------------------------------------------

const listeners = new Map<string, Set<Listener>>();

const emit = (channel: string, ...args: unknown[]) => {
  for (const listener of [...(listeners.get(channel) ?? [])]) {
    listener({ sender: null }, ...args);
  }
};

const invokeHandlers: Record<string, (...args: never[]) => unknown> = {
  'peard:get-config': (id: string) => pluginConfig(id),
  'peard:set-config': (id: string, value: object) =>
    setPluginConfig(id, value),
};

const ipcRenderer = {
  on(channel: string, listener: Listener) {
    if (!listeners.has(channel)) listeners.set(channel, new Set());
    listeners.get(channel)!.add(listener);
  },
  once(channel: string, listener: Listener) {
    const wrapped: Listener = (...args) => {
      ipcRenderer.off(channel, wrapped);
      listener(...args);
    };
    ipcRenderer.on(channel, wrapped);
  },
  off(channel: string, listener: Listener) {
    listeners.get(channel)?.delete(listener);
  },
  removeListener(channel: string, listener: Listener) {
    ipcRenderer.off(channel, listener);
  },
  removeAllListeners(channel: string) {
    listeners.delete(channel);
  },
  send(channel: string, ...args: unknown[]) {
    console.debug(LOG_PREFIX, 'send (no backend):', channel, ...args);
  },
  async invoke(channel: string, ...args: unknown[]) {
    const handler = invokeHandlers[channel];
    if (handler) return await handler(...(args as never[]));
    console.debug(LOG_PREFIX, 'invoke (no backend):', channel, ...args);
    return undefined;
  },
  sendSync(channel: string, ...args: unknown[]) {
    console.debug(LOG_PREFIX, 'sendSync (no backend):', channel, ...args);
    return undefined;
  },
  sendToHost() {},
};

// --- electron-is ---------------------------------------------------------

const ua = navigator.userAgent;
const platform = {
  windows: /Windows/.test(ua),
  macOS: /Mac OS X/.test(ua) && !/Mobile/.test(ua),
  linux: /Linux|X11/.test(ua),
};
const electronIs = {
  windows: () => platform.windows,
  macOS: () => platform.macOS,
  osx: () => platform.macOS,
  linux: () => platform.linux,
  freebsd: () => false,
  dev: () => false,
  production: () => true,
  renderer: () => true,
  main: () => false,
};

// --- mainConfig ----------------------------------------------------------

const mainConfig = {
  get: getPath,
  set: setPath,
  setPartial(key: string, value: object, defaultValue?: object) {
    setPath(
      key,
      deepmerge(defaultValue ?? {}, (getPath(key) as object) ?? {}, value),
    );
  },
  setMenuOption: setPath,
  plugins: {
    getPlugins() {
      const plugins = { ...storedPlugins() };
      for (const [id, override] of Object.entries(POC_DEFAULT_OVERRIDES)) {
        plugins[id] = { ...override, ...plugins[id] };
      }
      for (const id of UNPORTED_BACKEND_PLUGINS) {
        plugins[id] = { ...plugins[id], enabled: false };
      }
      return plugins;
    },
    isEnabled: async (id: string) => (await pluginConfig(id)).enabled,
    setOptions(id: string, options: Store, exclude = ['enabled']) {
      const filtered = { ...options };
      for (const key of exclude) delete filtered[key];
      void setPluginConfig(id, filtered);
    },
  },
};

Object.assign(window, {
  ipcRenderer,
  electronIs,
  mainConfig,
  reload: () => location.reload(),
  ELECTRON_RENDERER_URL: undefined,
  // Lets the Rust side push events: webview.eval("__pearIpcEmit(...)")
  __pearIpcEmit: emit,
});

// Electron injects this from the main process (`initTheme` in src/index.ts)
const sheet = new CSSStyleSheet();
sheet.replaceSync(musicPlayerCss);
document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];

console.log(LOG_PREFIX, 'shim installed');
