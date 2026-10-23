// Registers the content script that loads the renderer bundle, the GeckoView
// counterpart of the Tauri loader (src-tauri/src/lib.rs#loader_script). The
// settings snapshot is part of the registered code, so it is in the page
// synchronously before the bundle runs. The snapshot comes from Rust over
// native messaging (src-tauri/src/gecko_bridge.rs), and every config change
// re-registers the code, so the next page load starts with it.

// Runs in the content script (isolated world) at document_start
function load(payload, bundleUrl) {
  if (window.top !== window) return;
  const page = window.wrappedJSObject;
  if (page.__pearRendererLoaded) return;
  const read = (url) => {
    const xhr = new XMLHttpRequest();
    xhr.open('GET', url, false);
    xhr.send();
    return xhr.responseText;
  };
  page.__PEAR_STORED_CONFIG__ = cloneInto(payload.config, window);
  page.__PEAR_UNAVAILABLE_PLUGINS__ = cloneInto(payload.unavailable, window);
  page.__PEAR_MOBILE__ = true;
  page.__pearRendererLoaded = true;
  // YTM requires Trusted Types for script sinks, including eval
  const policy = page.trustedTypes?.createPolicy(
    'pear-gecko-loader',
    cloneInto({ createScript: (script) => script }, window, { cloneFunctions: true }),
  );
  const run = (code) => page.eval(policy ? policy.createScript(code) : code);

  // The IPC bridge (inject/tauri.ts): JSON strings both ways, `{ eval }`
  // for scripts Rust runs in the page
  const port = browser.runtime.connectNative('pear');
  port.onMessage.addListener((message) => {
    try {
      if (message.eval !== undefined) run(message.eval);
      else page.__pearReceive?.(message.m);
    } catch (error) {
      console.error('[pear-gecko] handling a message from Rust failed', error);
    }
  });
  page.__pearNative = cloneInto(
    { send: (message) => port.postMessage({ m: String(message) }) },
    window,
    { cloneFunctions: true },
  );

  run(`${read(bundleUrl)}\n//# sourceURL=pear-renderer.js`);
}

const port = browser.runtime.connectNative('pear');
let nextId = 1;
const pending = new Map();

const invoke = (cmd, args = {}) =>
  new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    port.postMessage({ m: JSON.stringify({ id, cmd, args }) });
  });

let payload;
let registered;
// One registration at a time, in the order of the config changes
let queue = Promise.resolve();
const register = () => {
  queue = queue.then(async () => {
    const args = [payload, browser.runtime.getURL('renderer.js')]
      .map((arg) => JSON.stringify(arg))
      .join(', ');
    const previous = registered;
    registered = await browser.contentScripts.register({
      matches: ['https://music.youtube.com/*'],
      js: [{ code: `(${load})(${args});` }],
      runAt: 'document_start',
    });
    await previous?.unregister();
  });
  return queue;
};

port.onMessage.addListener(({ m }) => {
  const message = JSON.parse(m);
  if ('event' in message) {
    if (message.event === 'pear:config' && payload) {
      payload.config = message.payload;
      register().catch(report);
    }
    return;
  }
  const request = pending.get(message.id);
  if (!request) return;
  pending.delete(message.id);
  if ('err' in message) request.reject(new Error(message.err));
  else request.resolve(message.ok);
});

const report = (error) =>
  browser.runtime.sendNativeMessage('pear', { error: String(error?.stack ?? error) });

const start = async () => {
  payload = await invoke('gecko_payload');
  await register();
  // GeckoPage loads the page once this arrives
  await browser.runtime.sendNativeMessage('pear', { ready: true, startUrl: payload.startUrl });
};

start().catch(report);
