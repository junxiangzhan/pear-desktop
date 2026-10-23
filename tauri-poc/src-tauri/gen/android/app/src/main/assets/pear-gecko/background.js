// Registers the content script that loads the renderer bundle, the GeckoView
// counterpart of the Tauri loader (src-tauri/src/lib.rs#loader_script). The
// settings snapshot is part of the registered code, so it is in the page
// synchronously before the bundle runs; re-registering updates it.
//
// Phase 1 (GECKOVIEW-PLAN.md): the snapshot is a fixed config with plugin
// defaults and in-app-menu on (stub-config.json; the comparison with every
// plugin on used a variant of it), and Rust is replaced by stub-ipc.js.
// Phase 2 gets the snapshot from Rust over native messaging.

// Runs in the content script (isolated world) at document_start
function load(payload, stubUrl, bundleUrl) {
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
  const run = (source, name) => {
    const code = `${source}\n//# sourceURL=${name}`;
    page.eval(policy ? policy.createScript(code) : code);
  };
  run(read(stubUrl), 'pear-stub-ipc.js');
  run(read(bundleUrl), 'pear-renderer.js');
}

const register = async () => {
  const payload = await (await fetch(browser.runtime.getURL('stub-config.json'))).json();
  const args = [payload, browser.runtime.getURL('stub-ipc.js'), browser.runtime.getURL('renderer.js')]
    .map((arg) => JSON.stringify(arg))
    .join(', ');
  await browser.contentScripts.register({
    matches: ['https://music.youtube.com/*'],
    js: [{ code: `(${load})(${args});` }],
    runAt: 'document_start',
  });
  // GeckoActivity loads the page once this arrives
  await browser.runtime.sendNativeMessage('pear', { ready: true });
};

register().catch((error) =>
  browser.runtime.sendNativeMessage('pear', { error: String(error?.stack ?? error) }),
);
