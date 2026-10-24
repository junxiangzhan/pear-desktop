// Stand-ins for the globals that `src/preload.ts` exposes through Electron's
// contextBridge, so the renderer bundle runs unchanged in the Tauri webview:
// `ipcRenderer` talks to the Rust dispatcher, `mainConfig` mirrors the
// Rust-owned config, `electronIs` reads the platform from the user agent.
//
// This module must not import `virtual:plugins`: that pulls in every plugin,
// and their top-level code would run before the globals below exist.
// `plugin-stubs.ts` handles the plugin list once the globals are in place.
import * as mainConfig from './compat/config';
import electronIs from './compat/electron-is';
import { emitLocal, ipcRenderer } from './ipc';
import { invoke } from './tauri';

import musicPlayerCss from '@/music-player.css?inline';

// `crossOriginFetch` (src/providers/net.ts): Electron strips CORS for the
// whole session; here cross-origin requests go through Rust (`net_fetch`,
// limited to an allowlist of hosts).
const crossOriginFetch: typeof fetch = async (input, init) => {
  const request = new Request(input, init);
  const body = ['GET', 'HEAD'].includes(request.method) ? undefined : await request.text();
  const response = await invoke<{ status: number; headers: [string, string][]; body: string }>(
    'net_fetch',
    {
      url: request.url,
      init: {
        method: request.method,
        headers: Object.fromEntries(request.headers.entries()),
        body,
      },
    },
  );
  const nullBody = [101, 204, 205, 304].includes(response.status);
  return new Response(nullBody ? null : response.body, {
    status: response.status,
    headers: response.headers,
  });
};

Object.assign(window, {
  ipcRenderer,
  electronIs,
  mainConfig,
  reload: () => location.reload(),
  ELECTRON_RENDERER_URL: undefined,
  __pearCrossOriginFetch: crossOriginFetch,
  // No native window controls over a frameless window (Electron's
  // `titleBarOverlay`), so in-app-menu draws its own
  __pearNoTitleBarOverlay: true,
  // Android has no window to minimize, maximize or close
  __pearNoWindowControls: (window as { __PEAR_MOBILE__?: boolean }).__PEAR_MOBILE__ === true,
  // Debug hook: deliver a message to page listeners from the DevTools console
  __pearIpcEmit: emitLocal,
});

// Electron injects this from the main process (`initTheme` in src/index.ts)
const sheet = new CSSStyleSheet();
sheet.replaceSync(musicPlayerCss);
document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];

// YTM bug: it tracks its layout size class (-4 to 3) with one media query
// listener per class, then applies it with checks like `size && size >= -1`
// (persistent sidebar) and `size && size >= -2` (showing the nav bar and the
// player bar again under the player page). Class 0 (1150 to 1363 px wide) is
// falsy, so a jump from a narrow layout straight into class 0 leaves the
// narrow layout in place. Dragging a window passes through class -1 first;
// an Android floating window jumps (a phone-sized window, then the full one
// scaled down). Do what dragging does: when the class 0 query starts
// matching, report class -1 to YTM's listeners first, and class 0 a moment
// later (YTM batches its store updates, so back to back only 0 would count).
const CLASS_MINUS_1 = '(min-width:936px)and(max-width:1149px)';
const CLASS_0 = '(min-width:1150px)and(max-width:1363px)';
type MediaListener = (event: { matches: boolean }) => void;
const classMinus1Listeners: MediaListener[] = [];
const addMediaListener = Object.getOwnPropertyDescriptor(MediaQueryList.prototype, 'addListener')
  ?.value as (this: MediaQueryList, listener: MediaListener) => void;
Object.defineProperty(MediaQueryList.prototype, 'addListener', {
  configurable: true,
  writable: true,
  value(this: MediaQueryList, listener: MediaListener) {
    const query = this.media.replace(/\s+/g, '');
    if (query === CLASS_MINUS_1) classMinus1Listeners.push(listener);
    if (query !== CLASS_0) return addMediaListener.call(this, listener);
    addMediaListener.call(this, (event) => {
      if (!event.matches) return listener(event);
      console.log('[pear-tauri]', 'viewport class 0: passing through class -1');
      for (const stepListener of classMinus1Listeners) stepListener({ matches: true });
      setTimeout(() => listener(event), 100);
    });
  },
});

console.log('[pear-tauri]', 'shim installed');
