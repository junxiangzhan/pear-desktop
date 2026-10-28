// Evaluates the plugins' `menu(ctx)` templates in the page and hands Rust a
// serializable copy to render as the native "Plugins" menu
// (`src-tauri/src/menu.rs`). Click handlers stay here, keyed by command id;
// Rust reports clicks back as `peard:menu-event` with the item's new
// `checked` state. Same round trip as `src/plugins/in-app-menu/main.ts`.
import { deepmerge } from 'deepmerge-ts';
import i18next from 'i18next';
import { languageResources } from 'virtual:i18n';
import { allPlugins } from 'virtual:plugins';

import { APPLICATION_NAME } from '@/i18n';
import { startingPages } from '@/providers/extracted-data';

import * as config from '@/config';
import { emitLocal, ipcRenderer } from '@/platform/ipc';
import { invoke } from '@/platform/tauri';

import type { MenuContext } from '@/types/contexts';
import type { PluginConfig } from '@/types/plugins';
import type { MenuItemOptions, PluginWindow } from '@/types/menu';

const LOG_PREFIX = '[pear-tauri]';

interface SerializedItem {
  commandId?: number;
  label?: string;
  type?: MenuItemOptions['type'];
  checked?: boolean;
  enabled?: boolean;
  visible?: boolean;
  accelerator?: string;
  role?: string;
  submenu?: SerializedItem[];
}

interface PluginMenuEntry {
  id: string;
  name: string;
  description?: string;
  enabled: boolean;
  /** False while the plugin's backend has no Tauri port (shown greyed out) */
  available: boolean;
  submenu?: SerializedItem[];
}

const unavailable = new Set(window.__PEAR_UNAVAILABLE_PLUGINS__ ?? []);

interface Handler {
  item: { checked?: boolean };
  click: NonNullable<MenuItemOptions['click']>;
}

let handlers = new Map<number, Handler>();
let nextCommandId = 1;

// What plugin menus do with `ctx.window`, over Tauri; other calls are logged
const windowFacade = new Proxy(
  {
    setAlwaysOnTop: (flag: boolean) =>
      invoke('window_set_always_on_top', { flag }),
    webContents: { send: (channel: string, ...args: unknown[]) => emitLocal(channel, ...args) },
  } as Record<string, unknown>,
  {
    get(target, property) {
      if (property in target) return target[property as string];
      return (...args: unknown[]) =>
        console.warn(LOG_PREFIX, `menu called unsupported window.${String(property)}`, args);
    },
  },
) as unknown as PluginWindow;

const serialize = (items: MenuItemOptions[], next: Map<number, Handler>): SerializedItem[] =>
  items.map((item) => {
    const serialized: SerializedItem = {
      label: item.label,
      type: item.type ?? (item.submenu ? 'submenu' : 'normal'),
      checked: item.checked,
      enabled: item.enabled,
      visible: item.visible,
      accelerator: typeof item.accelerator === 'string' ? item.accelerator : undefined,
      role: item.role,
    };
    if (item.click) {
      const commandId = nextCommandId++;
      next.set(commandId, { item: { checked: item.checked }, click: item.click });
      serialized.commandId = commandId;
    }
    if (Array.isArray(item.submenu)) {
      serialized.submenu = serialize(item.submenu as MenuItemOptions[], next);
    }
    return serialized;
  });

const createContext = (id: string): MenuContext<PluginConfig> => ({
  getConfig: () => ipcRenderer.invoke('peard:get-config', id) as Promise<PluginConfig>,
  setConfig: async (newConfig) => {
    await ipcRenderer.invoke('peard:set-config', id, newConfig);
  },
  window: windowFacade,
  refresh: () => scheduleRebuild(),
});

const build = async () => {
  const stubs = await allPlugins();
  const stored = config.plugins.getPlugins();
  const next = new Map<number, Handler>();
  const entries: PluginMenuEntry[] = [];

  for (const [id, stub] of Object.entries(stubs)) {
    const merged = deepmerge(stub.config ?? { enabled: false }, stored[id] ?? {}) as PluginConfig;
    const entry: PluginMenuEntry = {
      id,
      name: stub.name?.() ?? id,
      description: stub.description?.(),
      enabled: merged.enabled && !unavailable.has(id),
      available: !unavailable.has(id),
    };
    if (merged.enabled && stub.menu) {
      try {
        const template = await stub.menu(createContext(id));
        if (template.length > 0) entry.submenu = serialize(template, next);
      } catch (error) {
        console.error(LOG_PREFIX, `menu of '${id}' failed`, error);
      }
    }
    entries.push(entry);
  }

  entries.sort((a, b) => a.name.localeCompare(b.name));
  handlers = next;
  await invoke('menu_update', {
    data: {
      plugins: entries,
      labels: await coreMenuLabels(),
      startingPages: Object.keys(startingPages),
      languages: await languageList(),
    },
  });
};

// Translations for every `main.menu.*` and `main.tray.*` key, plus dialogs
// that Rust backends show; Rust builds the core menu and the tray with them.
// Interpolated values stay as `{name}` placeholders for Rust to fill (e.g.
// the tray tooltip's artist and title).
const coreMenuLabels = async () => {
  const flatten = (node: unknown, prefix: string): string[] =>
    node && typeof node === 'object'
      ? Object.entries(node).flatMap(([key, value]) => flatten(value, `${prefix}.${key}`))
      : [prefix];
  const english = (await languageResources()).en?.translation as {
    main?: { menu?: unknown; tray?: unknown };
    plugins?: Record<string, { dialog?: unknown }>;
  };
  const keys = [
    ...flatten(english?.main?.menu, 'main.menu'),
    ...flatten(english?.main?.tray, 'main.tray'),
    ...flatten(english?.plugins?.['api-server']?.dialog, 'plugins.api-server.dialog'),
    'plugins.amuse.response.query',
  ];
  const values = {
    applicationName: APPLICATION_NAME,
    artist: '{artist}',
    title: '{title}',
    ID: '{ID}',
    origin: '{origin}',
  };
  return Object.fromEntries(keys.map((key) => [key, i18next.t(key, values)]));
};

// Same labels as the language submenu in `src/menu.ts`
const languageList = async () => {
  const resources = await languageResources();
  return Object.entries(resources).map(([code, resource]) => {
    const language = (resource.translation as { language?: Record<string, string> }).language;
    return [code, `${language?.name ?? 'Unknown'} (${language?.['local-name'] ?? 'Unknown'})`];
  });
};

let pending: Promise<void> | null = null;
let again = false;
// Coalesces bursts (config changes often arrive several at once)
const scheduleRebuild = async () => {
  if (pending) {
    again = true;
    return pending;
  }
  pending = (async () => {
    do {
      again = false;
      await build().catch((error: unknown) =>
        console.error(LOG_PREFIX, 'building the plugin menu failed', error),
      );
    } while (again);
    pending = null;
  })();
  return pending;
};

ipcRenderer.on('peard:menu-event', async (_, commandId, checked) => {
  const handler = handlers.get(commandId as number);
  if (!handler) return;
  if (typeof checked === 'boolean') handler.item.checked = checked;
  const before = handler.item.checked;
  // Electron calls click(menuItem, browserWindow, event)
  await (handler.click as (...args: unknown[]) => unknown)(handler.item, windowFacade, {});
  // Handlers may write `item.checked` (e.g. undo after a cancelled prompt);
  // in Electron that updates the native item directly
  if (handler.item.checked !== before && typeof handler.item.checked === 'boolean') {
    await invoke('menu_set_checked', { commandId, checked: handler.item.checked });
  }
});

ipcRenderer.on('plugin:enable', () => void scheduleRebuild());
ipcRenderer.on('plugin:unload', () => void scheduleRebuild());
ipcRenderer.on('config-changed', () => void scheduleRebuild());

// Plugin names are translated, so build once i18n is ready and on language change
if (i18next.isInitialized) void scheduleRebuild();
i18next.on('initialized', () => void scheduleRebuild());
i18next.on('languageChanged', () => void scheduleRebuild());
