// `electron` for the page bundle. Preload plugins run in the page's main
// world here (there is no isolated world), so `contextBridge` and `webFrame`
// map onto `window` and `eval`. Main-process modules only appear in type
// positions or in stripped backend code; they are stubs that throw if used.
import { ipcRenderer } from '../ipc';

// YTM enforces Trusted Types for script sinks, including eval
const scriptPolicy = window.trustedTypes?.createPolicy('pear-tauri-compat', {
  createScript: (script: string) => script,
});

const evalInPage = (code: string): unknown =>
  // oxlint-disable-next-line no-eval
  (0, eval)((scriptPolicy?.createScript(code) ?? code) as string);

export const contextBridge = {
  exposeInMainWorld(key: string, api: unknown) {
    (window as unknown as Record<string, unknown>)[key] = api;
  },
};

export const webFrame = {
  executeJavaScript: async (code: string) => evalInPage(code),
  executeJavaScriptInIsolatedWorld: async (
    _worldId: number,
    scripts: { code: string }[],
  ) => scripts.map(({ code }) => evalInPage(code)).at(-1),
};

export { ipcRenderer };

const mainProcessOnly = (name: string) =>
  new Proxy(
    {},
    {
      get(_, property) {
        throw new Error(
          `electron.${name}.${String(property)} is main-process only; it is not available in the Tauri page`,
        );
      },
    },
  );

export const app = mainProcessOnly('app');
export const BrowserWindow = mainProcessOnly('BrowserWindow');
type OpenDialogOptions = {
  title?: string;
  filters?: { name: string; extensions: string[] }[];
  properties?: string[];
};

// Menus pick files (api-server's HTTPS certificate); the Rust side opens the
// native dialog. Only single-file picking, the one use so far.
export const dialog = new Proxy(
  {
    showOpenDialog: async (
      windowOrOptions: unknown,
      maybeOptions?: OpenDialogOptions,
    ): Promise<{ canceled: boolean; filePaths: string[] }> =>
      (await ipcRenderer.invoke(
        'pear:show-open-dialog',
        maybeOptions ?? windowOrOptions,
      )) as { canceled: boolean; filePaths: string[] },
  },
  {
    get(target, property) {
      if (property in target) return target[property as keyof typeof target];
      throw new Error(
        `electron.dialog.${String(property)} is main-process only; it is not available in the Tauri page`,
      );
    },
  },
);
export const globalShortcut = mainProcessOnly('globalShortcut');
export const ipcMain = mainProcessOnly('ipcMain');
export const Menu = mainProcessOnly('Menu');
export const MenuItem = mainProcessOnly('MenuItem');
export const nativeImage = mainProcessOnly('nativeImage');
export const nativeTheme = mainProcessOnly('nativeTheme');
export const net = mainProcessOnly('net');
export const Notification = mainProcessOnly('Notification');
export const screen = mainProcessOnly('screen');
export const session = mainProcessOnly('session');
export const shell = mainProcessOnly('shell');
export const TouchBar = mainProcessOnly('TouchBar');
export const Tray = mainProcessOnly('Tray');

export default {
  contextBridge,
  webFrame,
  ipcRenderer,
};
