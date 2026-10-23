// Runs the plugins' preload parts in the page, as `src/preload.ts` does in
// Electron's isolated world. `@/config` resolves to the Rust-backed mirror
// (`compat/config.ts`) in this bundle, so the shared loader works unchanged.
import { ipcRenderer } from './ipc';

import {
  forceLoadPreloadPlugin,
  forceUnloadPreloadPlugin,
  loadAllPreloadPlugins,
} from '@/loader/preload';

void loadAllPreloadPlugins();

ipcRenderer.on('plugin:unload', (_, id) => void forceUnloadPreloadPlugin(id as string));
ipcRenderer.on('plugin:enable', (_, id) => void forceLoadPreloadPlugin(id as string));
