// Stands in for do-not-track's blocklist mode modules (src/plugins/do-not-track/
// blocker.ts and injectors/inject-cliqz-preload.ts) in the page bundle. The
// network side runs in Rust (src-tauri/src/plugins/do_not_track.rs); what
// Electron's Ghostery preload did in the page, the URL's cosmetic filters
// (hiding CSS and scriptlets), is asked from Rust here.
import { ipcRenderer } from '../ipc';

const LOG_PREFIX = '[pear-tauri]';

const scriptPolicy = window.trustedTypes?.createPolicy('pear-tauri-do-not-track', {
  createScript: (script: string) => script,
});

// Backend-only exports (the plugin's stripped `backend` imports them)
export const isBlockerEnabled = () => false;
export const loadTrackerBlockerEngine = async () => {};
export const unloadTrackerBlockerEngine = () => {};

let injected = false;

// inject-cliqz-preload's default export, run by the plugin's preload
export default async () => {
  if (injected) return;
  injected = true;
  // The backend starts once the page has sent its plugin list
  let cosmetics: { css: string; script: string } | undefined;
  for (let attempt = 0; attempt < 20 && !cosmetics; attempt++) {
    try {
      cosmetics = (await ipcRenderer.invoke('do-not-track:cosmetics', location.href)) as typeof cosmetics;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
  }
  if (!cosmetics) {
    console.warn(LOG_PREFIX, 'do-not-track: no cosmetic filters');
    return;
  }
  if (cosmetics.css) {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(cosmetics.css);
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  }
  if (cosmetics.script) {
    try {
      // oxlint-disable-next-line typescript/no-implied-eval
      new Function((scriptPolicy?.createScript(cosmetics.script) ?? cosmetics.script) as string)();
    } catch (error) {
      console.error(LOG_PREFIX, 'do-not-track: a scriptlet failed', error);
    }
  }
};
