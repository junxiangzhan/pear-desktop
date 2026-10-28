// Minimal access to Tauri's IPC from the remote page, without bundling
// `@tauri-apps/api`. Mirrors what `@tauri-apps/api/core#invoke` and
// `@tauri-apps/api/event#listen` do on top of `__TAURI_INTERNALS__`.
//
// On Android the page runs in GeckoView instead of Tauri's WebView
// (GECKOVIEW-PLAN.md), where the built-in extension provides
// `__pearNative`: commands go to Rust as JSON `{ id, cmd, args }` over native
// messaging (src-tauri/src/gecko_bridge.rs), which answers `{ id, ok | err }`
// and pushes `{ event, payload }` through `__pearReceive`.

interface TauriInternals {
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  transformCallback: (
    callback: (payload: unknown) => void,
    once?: boolean,
  ) => number;
}

interface PearNative {
  send: (message: string) => void;
}

type FromRust =
  | { id: number; ok?: unknown; err?: unknown }
  | { event: string; payload: unknown };

declare global {
  interface Window {
    __TAURI_INTERNALS__?: TauriInternals;
    __pearNative?: PearNative;
    __pearReceive?: (message: string) => void;
    /** Round trips of bridged invokes per command, for measuring the bridge. */
    __pearBridgeStats?: Record<
      string,
      { count: number; totalMs: number; maxMs: number }
    >;
  }
}

const internals = (): TauriInternals => {
  const value = window.__TAURI_INTERNALS__;
  if (!value) throw new Error('Tauri IPC is not available on this page');
  return value;
};

const gecko = (() => {
  const native = window.__pearNative;
  if (!native) return null;

  let nextId = 1;
  const pending = new Map<
    number,
    {
      resolve: (value: unknown) => void;
      reject: (reason: unknown) => void;
      cmd: string;
      at: number;
    }
  >();
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  const stats: NonNullable<Window['__pearBridgeStats']> = {};
  window.__pearBridgeStats = stats;

  window.__pearReceive = (raw) => {
    const message = JSON.parse(raw) as FromRust;
    if ('event' in message) {
      for (const handler of listeners.get(message.event) ?? []) {
        handler(message.payload);
      }
      return;
    }
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    const ms = performance.now() - request.at;
    const entry = (stats[request.cmd] ??= { count: 0, totalMs: 0, maxMs: 0 });
    entry.count += 1;
    entry.totalMs += ms;
    entry.maxMs = Math.max(entry.maxMs, ms);
    if ('err' in message) request.reject(message.err);
    else request.resolve(message.ok ?? null);
  };

  return {
    invoke: (cmd: string, args?: Record<string, unknown>) =>
      new Promise<unknown>((resolve, reject) => {
        const id = nextId++;
        pending.set(id, { resolve, reject, cmd, at: performance.now() });
        native.send(JSON.stringify({ id, cmd, args: args ?? {} }));
      }),
    // Rust pushes every page event over the bridge; no subscription needed
    listen: (event: string, handler: (payload: unknown) => void) => {
      const set = listeners.get(event) ?? new Set();
      set.add(handler);
      listeners.set(event, set);
    },
  };
})();

export const invoke = <T = unknown>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> =>
  (gecko
    ? gecko.invoke(cmd, args)
    : internals().invoke(cmd, args)) as Promise<T>;

/** Subscribes to an event emitted from Rust with `emit_to("main", …)`. */
export const listen = async <T>(
  event: string,
  handler: (payload: T) => void,
): Promise<void> => {
  if (gecko) {
    gecko.listen(event, handler as (payload: unknown) => void);
    return;
  }
  const { transformCallback } = internals();
  await invoke('plugin:event|listen', {
    event,
    target: { kind: 'Any' },
    handler: transformCallback((message) =>
      handler((message as { payload: T }).payload),
    ),
  });
};
