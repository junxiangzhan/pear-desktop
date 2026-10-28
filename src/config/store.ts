// The page's mirror of the config that Rust owns
// (tauri-poc/src-tauri/src/config.rs). Reads are synchronous from the mirror,
// seeded from the payload Rust bakes into the page; writes go to Rust, which
// persists them, runs the plugin watcher and broadcasts the new store back as
// `pear:config`.
import { deepmergeCustom } from 'deepmerge-ts';

import { invoke, listen } from '@/platform/tauri';

export type Store = Record<string, unknown>;
export type Watcher = (newValue: Store, oldValue: Store) => void;

declare global {
  interface Window {
    __PEAR_STORED_CONFIG__?: Store;
    /** Plugins the Tauri build cannot run; always reported as off. */
    __PEAR_UNAVAILABLE_PLUGINS__?: string[];
  }
}

const LOG_PREFIX = '[pear-tauri]';
const deepmerge = deepmergeCustom({ mergeArrays: false });

let store: Store = structuredClone(window.__PEAR_STORED_CONFIG__ ?? {});
export const unavailable = new Set(window.__PEAR_UNAVAILABLE_PLUGINS__ ?? []);
const watchers = new Set<Watcher>();

export const report = (what: string) => (error: unknown) =>
  console.error(LOG_PREFIX, `${what} failed`, error);

listen<Store>('pear:config', (next) => {
  const previous = store;
  store = next;
  for (const watcher of watchers) watcher(store, previous);
}).catch(report('listening for config changes'));

export const getPath = (key: string): unknown =>
  key
    .split('.')
    .reduce<unknown>(
      (node, part) => (node as Store | undefined)?.[part],
      store,
    );

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

export const setPath = (key: string, value: unknown) => {
  setPathLocally(key, value);
  invoke('config_set', { key, value }).catch(report(`config set '${key}'`));
};

export const setPathPartial = (
  key: string,
  value: object,
  defaultValue?: object,
) => {
  setPathLocally(
    key,
    deepmerge(defaultValue ?? {}, (getPath(key) as object) ?? {}, value),
  );
  invoke('config_set_partial', {
    key,
    value,
    defaultValue: defaultValue ?? null,
  }).catch(report(`config setPartial '${key}'`));
};

/** Restarts the app when the user asked for it after config changes. */
export const restartIfWanted = () => {
  if (getPath('options.restartOnConfigChanges')) {
    invoke('app_restart').catch(report('restart'));
  }
};

export const addWatcher = (watcher: Watcher) => {
  watchers.add(watcher);
  return () => watchers.delete(watcher);
};
