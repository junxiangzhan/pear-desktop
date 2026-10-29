// The discord menu's view of the Rust service
// (src-tauri/src/plugins/discord.rs), which owns the RPC client.
const { ipcRenderer } = window;

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
