// Phase 1 stand-in for Rust: answers the bundle's `__TAURI_INTERNALS__`
// calls (inject/tauri.ts) in the page. Plugin configs come from the plugin
// defaults the page sends plus the stored config, like Rust's
// `peard:get-config`; everything else is logged and either ignored (sends) or
// rejected (invokes Rust would answer). Counts land in
// `document.documentElement.dataset.pearStub` for report.js.
(() => {
  const stats = { invokes: {}, rejected: {}, errors: [] };
  const count = (map, key) => (map[key] = (map[key] ?? 0) + 1);
  const stored = window.__PEAR_STORED_CONFIG__ ?? {};
  const defaults = new Map();
  const merge = (a, b) => {
    if (!b || typeof b !== 'object' || Array.isArray(b)) return b ?? a;
    const out = { ...(a ?? {}) };
    for (const [k, v] of Object.entries(b)) out[k] = merge(out[k], v);
    return out;
  };
  const callbacks = new Map();
  let nextCallback = 1;
  const ignored = new Set([
    'plugin:event|listen',
    'ipc_send',
    'menu_update',
    'menu_set_checked',
    'config_set',
    'config_set_partial',
  ]);

  window.__TAURI_INTERNALS__ = {
    transformCallback(callback) {
      const id = nextCallback++;
      callbacks.set(id, callback);
      return id;
    },
    async invoke(cmd, args = {}) {
      count(stats.invokes, cmd === 'ipc_invoke' || cmd === 'ipc_send' ? `${cmd} ${args.channel}` : cmd);
      if (cmd === 'plugins_manifest') {
        for (const entry of args.manifest ?? []) defaults.set(entry.id, entry.config);
        return null;
      }
      if (cmd === 'ipc_invoke' && args.channel === 'peard:get-config') {
        const id = args.args?.[0];
        return merge(defaults.get(id) ?? { enabled: false }, stored.plugins?.[id] ?? {});
      }
      if (ignored.has(cmd)) return null;
      const what = cmd === 'ipc_invoke' ? `ipc_invoke ${args.channel}` : cmd;
      count(stats.rejected, what);
      throw new Error(`${what}: no Rust behind the stub IPC (phase 1)`);
    },
  };

  const record = (kind, message, stack) => {
    if (stats.errors.length >= 150) return;
    stats.errors.push({
      kind,
      message: String(message).slice(0, 300),
      stack: String(stack ?? '').split('\n').slice(0, 4).join(' | ').slice(0, 400),
    });
  };
  addEventListener('error', (e) => record('error', e.message, e.error?.stack));
  addEventListener('unhandledrejection', (e) =>
    record('rejection', e.reason?.message ?? e.reason, e.reason?.stack),
  );
  // Firefox starts an AudioContext created without a user gesture as
  // "suspended"; the renderer routes the <video> through one
  const audioContexts = [];
  const NativeAudioContext = window.AudioContext;
  window.AudioContext = class extends NativeAudioContext {
    constructor(...args) {
      super(...args);
      audioContexts.push(this);
    }
  };
  // Web APIs the plugins use that differ between engines
  stats.apis = {
    navigation: typeof window.navigation?.addEventListener,
    mediaSetSinkId: typeof HTMLMediaElement.prototype.setSinkId,
    audioContextSetSinkId: typeof AudioContext.prototype.setSinkId,
    enumerateDevices: typeof navigator.mediaDevices?.enumerateDevices,
    requestPictureInPicture: typeof HTMLVideoElement.prototype.requestPictureInPicture,
    mediaSession: typeof navigator.mediaSession,
    adoptedStyleSheets: Array.isArray(document.adoptedStyleSheets),
  };
  const publish = () => {
    stats.audioContexts = audioContexts.map((context) => context.state);
    const video = document.querySelector('video');
    stats.video = video && { paused: video.paused, time: Math.round(video.currentTime) };
    document.documentElement.dataset.pearStub = JSON.stringify(stats);
  };
  addEventListener('DOMContentLoaded', publish);
  setInterval(publish, 3000);
})();
