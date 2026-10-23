// Stands in for src/plugins/discord/main.ts in the page bundle, where the
// discord menu runs: its `discordService` talks to the Rust service
// (`src-tauri/src/plugins/discord.rs`) instead of owning the RPC client.
import { ipcRenderer } from '../ipc';

let connected = false;
const refreshCallbacks: (() => void)[] = [];

ipcRenderer.on('discord:status', (_, value) => {
  connected = Boolean(value);
  refreshCallbacks.forEach((cb) => cb());
});

// After a page reload the service may already be connected. Rejects while
// the plugin is off, which leaves `false`.
ipcRenderer
  .invoke('discord:status')
  .then((value) => {
    connected = Boolean(value);
  })
  .catch(() => {});

export const discordService = {
  isConnected: () => connected,
  connect: (showErrorDialog = false) =>
    ipcRenderer.send('discord:connect', showErrorDialog),
  clearActivity: () => ipcRenderer.send('discord:clear-activity'),
  registerRefreshCallback: (cb: () => void) => {
    refreshCallbacks.push(cb);
  },
};
