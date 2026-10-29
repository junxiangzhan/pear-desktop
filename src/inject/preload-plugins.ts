import {
  forceLoadPreloadPlugin,
  forceUnloadPreloadPlugin,
  loadAllPreloadPlugins,
} from '@/loader/preload';
// Runs the plugins' preload parts in the page, as `src/preload.ts` does in
// Electron's isolated world. `@/config` is the Rust-backed mirror, so the
// shared loader works unchanged.
import { ipcRenderer } from '@/platform/ipc';

loadAllPreloadPlugins().catch(console.error);

ipcRenderer.on('plugin:unload', (_, id) => {
  forceUnloadPreloadPlugin(id as string).catch(console.error);
});
ipcRenderer.on('plugin:enable', (_, id) => {
  forceLoadPreloadPlugin(id as string).catch(console.error);
});
