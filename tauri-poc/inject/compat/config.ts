// Page-side mirror of the config that Rust owns (`src-tauri/src/config.rs`).
// Same API as `src/config` so it can serve both as `window.mainConfig` and as
// the `@/config` alias for page-bundled modules such as `src/loader/preload.ts`.
//
// Reads are synchronous from the mirror (seeded from the payload Rust bakes
// into the page); writes go to Rust, which persists them, runs the plugin
// watcher and broadcasts the new store back as `pear:config`.
import { deepmergeCustom } from 'deepmerge-ts';

import { invoke, listen } from '../tauri';

import type { PluginConfig } from '@/types/plugins';

export { defaultConfig } from '@/config/defaults';

type Store = Record<string, unknown>;
type Watcher = (newValue: Store, oldValue: Store) => void;

declare global {
  interface Window {
    __PEAR_STORED_CONFIG__?: Store;
    /** Plugins whose backend is not available on Tauri yet; always off. */
    __PEAR_UNAVAILABLE_PLUGINS__?: string[];
  }
}

const LOG_PREFIX = '[pear-tauri]';
const deepmerge = deepmergeCustom({ mergeArrays: false });

let store: Store = structuredClone(window.__PEAR_STORED_CONFIG__ ?? {});
const unavailable = new Set(window.__PEAR_UNAVAILABLE_PLUGINS__ ?? []);
const watchers = new Set<Watcher>();

const replaceStore = (next: Store) => {
  const previous = store;
  store = next;
  for (const watcher of watchers) watcher(store, previous);
};

void listen<Store>('pear:config', replaceStore);

const getPath = (key: string): unknown =>
  key
    .split('.')
    .reduce<unknown>((node, part) => (node as Store | undefined)?.[part], store);

const setPathLocally = (key: string, value: unknown) => {
  const parts = key.split('.');
  const last = parts.pop()!;
  let node = store;
  for (const part of parts) {
    node[part] ??= {};
    node = node[part] as Store;
  }
  node[last] = value;
};

const report = (what: string) => (error: unknown) =>
  console.error(LOG_PREFIX, `${what} failed`, error);

export const get = (key: string) => getPath(key) as never;

export const set = (key: string, value: unknown) => {
  setPathLocally(key, value);
  invoke('config_set', { key, value }).catch(report(`config set '${key}'`));
};

export const setPartial = (key: string, value: object, defaultValue?: object) => {
  setPathLocally(
    key,
    deepmerge(defaultValue ?? {}, (getPath(key) as object) ?? {}, value),
  );
  invoke('config_set_partial', { key, value, defaultValue: defaultValue ?? null }).catch(
    report(`config setPartial '${key}'`),
  );
};

export const setMenuOption = (key: string, value: unknown) => {
  set(key, value);
  if (get('options.restartOnConfigChanges')) {
    invoke('app_restart').catch(report('restart'));
  }
};

export const edit = () => invoke('config_edit').catch(report('open config'));

export const watch = (callback: Watcher) => {
  watchers.add(callback);
  return () => watchers.delete(callback);
};

const storedPlugins = () => (store.plugins ?? {}) as Record<string, Store>;

export const plugins = {
  getPlugins() {
    const result = { ...storedPlugins() };
    for (const id of unavailable) result[id] = { ...result[id], enabled: false };
    return result as Record<string, PluginConfig>;
  },
  // Rust merges the plugin's own defaults (sent in the manifest) with the store
  async isEnabled(id: string) {
    const config = (await invoke('ipc_invoke', {
      channel: 'peard:get-config',
      args: [id],
    })) as PluginConfig;
    return config.enabled;
  },
  setOptions<T>(id: string, options: T, exclude: string[] = ['enabled']) {
    const filtered = { ...options } as Store;
    for (const key of exclude) delete filtered[key];
    set(`plugins.${id}`, { ...storedPlugins()[id], ...filtered });
  },
  setMenuOptions<T>(id: string, options: T, exclude: string[] = ['enabled']) {
    plugins.setOptions(id, options, exclude);
    if (get('options.restartOnConfigChanges')) {
      invoke('app_restart').catch(report('restart'));
    }
  },
  getOptions: <T>(id: string) => storedPlugins()[id] as T,
  enable: (id: string) => plugins.setMenuOptions(id, { enabled: true }, []),
  disable: (id: string) => plugins.setMenuOptions(id, { enabled: false }, []),
};
