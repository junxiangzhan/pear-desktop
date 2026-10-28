// do-not-track's blocklist mode in the page. The network side runs in Rust
// (tauri-poc/src-tauri/src/plugins/do_not_track.rs); what Electron's Ghostery
// preload did in the page, the URL's cosmetic filters (hiding CSS and
// scriptlets), is asked from Rust here.
const { ipcRenderer } = window;

const LOG_PREFIX = '[pear-tauri]';

const scriptPolicy = window.trustedTypes?.createPolicy(
  'pear-tauri-do-not-track',
  {
    createScript: (script: string) => script,
  },
);

let injected = false;

const injectCosmetics = async () => {
  // The backend starts once the page has sent its plugin list
  let cosmetics: { css: string; script: string } | undefined;
  for (let attempt = 0; attempt < 20 && !cosmetics; attempt++) {
    try {
      cosmetics = (await ipcRenderer.invoke(
        'do-not-track:cosmetics',
        location.href,
      )) as typeof cosmetics;
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
      // oxlint-disable-next-line typescript/no-implied-eval, typescript/no-unsafe-call
      new Function(
        (scriptPolicy?.createScript(cosmetics.script) ??
          cosmetics.script) as string,
      )();
    } catch (error) {
      console.error(LOG_PREFIX, 'do-not-track: a scriptlet failed', error);
    }
  }
};

// Run by the plugin's preload. Does not wait: the first run downloads the
// lists, which would hold up the other preload plugins.
export default () => {
  if (injected) return;
  injected = true;
  injectCosmetics().catch((error: unknown) =>
    console.error(LOG_PREFIX, 'do-not-track: cosmetic filters failed', error),
  );
};
