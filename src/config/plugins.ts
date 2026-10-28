import { invoke } from '@/platform/tauri';

import { getPath, restartIfWanted, setPath, unavailable } from './store';

import type { PluginConfig } from '@/types/plugins';

const storedPlugins = () =>
  (getPath('plugins') ?? {}) as Record<string, Record<string, unknown>>;

export function getPlugins() {
  const result = { ...storedPlugins() };
  for (const id of unavailable) result[id] = { ...result[id], enabled: false };
  return result as Record<string, PluginConfig>;
}

// Rust merges the plugin's own defaults (sent in the manifest) with the store
export async function isEnabled(plugin: string) {
  const config = (await invoke('ipc_invoke', {
    channel: 'peard:get-config',
    args: [plugin],
  })) as PluginConfig;
  return config.enabled;
}

/**
 * Set options for a plugin
 * @param plugin Plugin name
 * @param options Options to set
 * @param exclude Options to exclude from the options object
 */
export function setOptions<T>(
  plugin: string,
  options: T,
  exclude: string[] = ['enabled'],
) {
  const filtered = { ...options } as Record<string, unknown>;
  for (const key of exclude) delete filtered[key];
  setPath(`plugins.${plugin}`, { ...storedPlugins()[plugin], ...filtered });
}

export function setMenuOptions<T>(
  plugin: string,
  options: T,
  exclude: string[] = ['enabled'],
) {
  setOptions(plugin, options, exclude);
  restartIfWanted();
}

export function getOptions<T>(plugin: string): T {
  return storedPlugins()[plugin] as T;
}

export function enable(plugin: string) {
  setMenuOptions(plugin, { enabled: true }, []);
}

export function disable(plugin: string) {
  setMenuOptions(plugin, { enabled: false }, []);
}
