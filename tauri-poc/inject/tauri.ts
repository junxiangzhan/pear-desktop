// Minimal access to Tauri's IPC from the remote page, without bundling
// `@tauri-apps/api`. Mirrors what `@tauri-apps/api/core#invoke` and
// `@tauri-apps/api/event#listen` do on top of `__TAURI_INTERNALS__`.

interface TauriInternals {
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  transformCallback: (callback: (payload: unknown) => void, once?: boolean) => number;
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: TauriInternals;
  }
}

const internals = (): TauriInternals => {
  const value = window.__TAURI_INTERNALS__;
  if (!value) throw new Error('Tauri IPC is not available on this page');
  return value;
};

export const invoke = <T = unknown>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> => internals().invoke(cmd, args) as Promise<T>;

/** Subscribes to an event emitted from Rust with `emit_to("main", …)`. */
export const listen = async <T>(
  event: string,
  handler: (payload: T) => void,
): Promise<void> => {
  const { transformCallback } = internals();
  await invoke('plugin:event|listen', {
    event,
    target: { kind: 'Any' },
    handler: transformCallback((message) =>
      handler((message as { payload: T }).payload),
    ),
  });
};
