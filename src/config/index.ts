// The app config, owned by Rust (see ./store.ts). Same API as the Electron
// app's electron-store wrapper, so it serves both as `window.mainConfig` and
// as `@/config` for page modules.
import { invoke } from '@/platform/tauri';

import {
  addWatcher,
  getPath,
  report,
  restartIfWanted,
  setPath,
  setPathPartial,
  type Watcher,
} from './store';

import type { defaultConfig } from './defaults';

export { defaultConfig } from './defaults';
export * as plugins from './plugins';

export const set = (key: string, value: unknown) => setPath(key, value);

export const setPartial = (key: string, value: object, defaultValue?: object) =>
  setPathPartial(key, value, defaultValue);

export const setMenuOption = (key: string, value: unknown) => {
  set(key, value);
  restartIfWanted();
};

// MAGIC OF TYPESCRIPT

type Prev = [
  never,
  0,
  1,
  2,
  3,
  4,
  5,
  6,
  7,
  8,
  9,
  10,
  11,
  12,
  13,
  14,
  15,
  16,
  17,
  18,
  19,
  20,
  ...0[],
];
type Join<K, P> = K extends string | number
  ? P extends string | number
    ? `${K}${'' extends P ? '' : '.'}${P}`
    : never
  : never;
type Paths<T, D extends number = 10> = [D] extends [never]
  ? never
  : T extends object
    ? {
        [K in keyof T]-?: K extends string | number
          ? `${K}` | Join<K, Paths<T[K], Prev[D]>>
          : never;
      }[keyof T]
    : '';

type SplitKey<K> = K extends `${infer A}.${infer B}` ? [A, B] : [K, string];
type PathValue<T, K extends string> =
  SplitKey<K> extends [infer A extends keyof T, infer B extends string]
    ? PathValue<T[A], B>
    : T;

export const get = <Key extends Paths<typeof defaultConfig>>(key: Key) =>
  getPath(key) as PathValue<typeof defaultConfig, typeof key>;

export const edit = () => invoke('config_edit').catch(report('open config'));

export const watch = (callback: Watcher) => addWatcher(callback);
