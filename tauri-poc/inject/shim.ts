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
  // Debug hook: deliver a message to page listeners from the DevTools console
  __pearIpcEmit: emitLocal,
});

// Electron injects this from the main process (`initTheme` in src/index.ts)
const sheet = new CSSStyleSheet();
sheet.replaceSync(musicPlayerCss);
document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];

console.log('[pear-tauri]', 'shim installed');
