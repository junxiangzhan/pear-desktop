import type { IpcRenderer } from '@/platform/ipc';
import type { PluginWindow } from '@/types/menu';
import type { PluginConfig } from '@/types/plugins';

export interface BaseContext<Config extends PluginConfig> {
  getConfig: () => Promise<Config> | Config;
  setConfig: (conf: Partial<Omit<Config, 'enabled'>>) => Promise<void> | void;
}

export interface MenuContext<
  Config extends PluginConfig,
> extends BaseContext<Config> {
  window: PluginWindow;
  refresh: () => Promise<void> | void;
}

/* oxlint-disable typescript/no-empty-object-type */
export interface PreloadContext<
  Config extends PluginConfig,
> extends BaseContext<Config> {}
/* oxlint-enable typescript/no-empty-object-type */

export interface RendererContext<
  Config extends PluginConfig,
> extends BaseContext<Config> {
  ipc: {
    send: IpcRenderer['send'];
    invoke: IpcRenderer['invoke'];
    on: (event: string, listener: CallableFunction) => void;
    removeAllListeners: (event: string) => void;
  };
}
