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

// YTM bug: with the player page open in its narrow layout, it hides the nav
// bar and the player bar (inline `visibility: hidden`), and one jump to a
// wide layout leaves them hidden until the size crosses another of its
// breakpoints. Android floating windows jump like that (phone size, then the
// full size scaled down). Once a resize settles in the wide layout, show them
// again as YTM would; video full screen hides them on purpose.
let resizeSettled: ReturnType<typeof setTimeout> | undefined;
window.addEventListener('resize', () => {
  clearTimeout(resizeSettled);
  resizeSettled = setTimeout(() => {
    const layout = document.querySelector('ytmusic-app-layout');
    const playerPage = document.querySelector('ytmusic-player-page');
    if (
      !layout ||
      playerPage?.hasAttribute('is-mweb-modernization-enabled') ||
      layout.getAttribute('player-ui-state') === 'FULLSCREEN'
    ) {
      return;
    }
    for (const slot of ['nav-bar', 'player-bar']) {
      const bar = layout.querySelector<HTMLElement>(`:scope > [slot="${slot}"]`);
      if (bar?.style.visibility === 'hidden') bar.style.visibility = '';
    }
  }, 500);
});

console.log('[pear-tauri]', 'shim installed');
