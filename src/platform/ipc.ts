// `window.ipcRenderer`, backed by the Rust dispatcher
// (tauri-poc/src-tauri/src/ipc.rs).
// Sends and invokes go to Rust as `{ channel, args }`; Rust pushes messages
// back as `pear:ipc` events, which are fanned out to `on`/`once` listeners
// with an Electron-style `(event, ...args)` signature.
import { invoke, listen } from './tauri';

/** What listeners get first, where Electron passed its IpcRendererEvent */
export interface IpcEvent {
  sender: null;
}

// Arguments come from Rust untyped; listeners declare what they expect, as
// they did with Electron's `...args: any[]`
// oxlint-disable-next-line typescript/no-explicit-any
export type IpcListener = (event: IpcEvent, ...args: any[]) => void;
type Listener = IpcListener;

const LOG_PREFIX = '[pear-tauri]';
const listeners = new Map<string, Set<Listener>>();
const localHandlers = new Map<string, (...args: unknown[]) => unknown>();

/**
 * Answers `invoke(channel)` in the page instead of Rust. For backends that
 * only show UI (a translated prompt), which belongs on the TS side.
 */
export const handleLocally = (
  channel: string,
  handler: (...args: unknown[]) => unknown,
) => {
  localHandlers.set(channel, handler);
};

/** Delivers a message to page listeners, as if it came from the main process. */
export const emitLocal = (channel: string, ...args: unknown[]) => {
  for (const listener of [...(listeners.get(channel) ?? [])]) {
    try {
      listener({ sender: null }, ...args);
    } catch (error) {
      console.error(LOG_PREFIX, `listener for '${channel}' threw`, error);
    }
  }
};

listen<{ channel: string; args: unknown[] }>('pear:ipc', ({ channel, args }) =>
  emitLocal(channel, ...args),
).catch((error: unknown) =>
  console.error(LOG_PREFIX, 'listening for IPC failed', error),
);

export const ipcRenderer = {
  on(channel: string, listener: Listener) {
    if (!listeners.has(channel)) listeners.set(channel, new Set());
    listeners.get(channel)!.add(listener);
    return ipcRenderer;
  },
  once(channel: string, listener: Listener) {
    const wrapped: Listener = (...args) => {
      ipcRenderer.off(channel, wrapped);
      listener(...args);
    };
    return ipcRenderer.on(channel, wrapped);
  },
  off(channel: string, listener: Listener) {
    listeners.get(channel)?.delete(listener);
    return ipcRenderer;
  },
  removeListener(channel: string, listener: Listener) {
    return ipcRenderer.off(channel, listener);
  },
  removeAllListeners(channel: string) {
    listeners.delete(channel);
    return ipcRenderer;
  },
  send(channel: string, ...args: unknown[]) {
    invoke('ipc_send', { channel, args }).catch((error: unknown) =>
      console.error(LOG_PREFIX, `send '${channel}' failed`, error),
    );
  },
  // Rejects when Rust has no handler, like Electron's ipcRenderer.invoke.
  // Resolves to `any` like Electron's: callers know the reply's shape.
  // oxlint-disable-next-line typescript/no-explicit-any
  invoke: async (channel: string, ...args: unknown[]): Promise<any> => {
    const local = localHandlers.get(channel);
    if (local) return await local(...args);
    return await invoke('ipc_invoke', { channel, args });
  },
};

export type IpcRenderer = typeof ipcRenderer;
