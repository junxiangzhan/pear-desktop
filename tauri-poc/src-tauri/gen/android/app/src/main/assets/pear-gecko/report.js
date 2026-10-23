// Phase 1: sends the stub's counters and the page state to GeckoActivity,
// which saves the latest report to files/gecko-report.json.
const report = () => {
  const signInButton = document.querySelector('a[href*="ServiceLogin"], ytmusic-guide-signin-promo-renderer');
  browser.runtime.sendNativeMessage('pear', {
    url: location.href,
    rendererLoaded: !!window.wrappedJSObject.__pearRendererLoaded,
    shim: typeof window.wrappedJSObject.ipcRenderer,
    signedIn: !!document.querySelector('ytmusic-settings-button img') && !signInButton,
    stub: JSON.parse(document.documentElement.dataset.pearStub ?? 'null'),
  });
};
report();
setInterval(report, 10000);
