// Stands in for do-not-track's blocklist mode (src/plugins/do-not-track/
// blocker.ts and injectors/inject-cliqz-preload.ts) in the page bundle. That
// mode filters network requests through Electron's `session.webRequest`,
// which Tauri has no equivalent of, so it is not ported; the default
// "In player" mode runs in the page unchanged.
const notPorted = () =>
  console.warn(
    '[pear-tauri]',
    'do-not-track: the "With blocklists" mode is not available in the Tauri build; use "In player"',
  );

export const isBlockerEnabled = () => false;
export const loadTrackerBlockerEngine = async () => notPorted();
export const unloadTrackerBlockerEngine = () => {};

// inject-cliqz-preload's default export
export default async () => notPorted();
