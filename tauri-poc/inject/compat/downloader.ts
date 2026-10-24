// Stands in for src/plugins/downloader/main/index.ts and main/utils.ts in the
// page bundle, where the downloader menu runs: "Download playlist" runs the
// page-side downloader (`inject/downloader.ts`), and the Rust backend picks
// the default folder.
export { downloadPlaylist } from '../downloader';

/** The folder dialogs start in; empty lets the native dialog choose. */
export const getFolder = (customFolder?: string) => customFolder ?? '';

// The plugin's `backend` (stripped from the page bundle) imports these
export const onMainLoad = () => {};
export const onConfigChange = () => {};
