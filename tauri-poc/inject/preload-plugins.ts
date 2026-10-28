// Runs the plugins' preload parts in the page, as `src/preload.ts` does in
// Electron's isolated world. `@/config` is the Rust-backed mirror, so the
// shared loader works unchanged.
import { ipcRenderer } from '@/platform/ipc';

import {
  forceLoadPreloadPlugin,
  forceUnloadPreloadPlugin,
  loadAllPreloadPlugins,
} from '@/loader/preload';

void loadAllPreloadPlugins();

ipcRenderer.on('plugin:unload', (_, id) => void forceUnloadPreloadPlugin(id as string));
ipcRenderer.on('plugin:enable', (_, id) => void forceLoadPreloadPlugin(id as string));
