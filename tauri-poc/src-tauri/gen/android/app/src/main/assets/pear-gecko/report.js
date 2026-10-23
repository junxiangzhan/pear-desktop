// Diagnostics: sends the page state, the IPC bridge's round trips and layout
// metrics to GeckoPage, which saves the latest report to
// files/gecko-report.json.
const rect = (selector) => {
  const element = document.querySelector(selector);
  if (!element) return null;
  const box = element.getBoundingClientRect();
  return { top: Math.round(box.top), bottom: Math.round(box.bottom), height: Math.round(box.height) };
};

const layout = () => {
  const app = document.querySelector('ytmusic-app');
  const appLayout = document.querySelector('ytmusic-app-layout');
  const styles = appLayout && getComputedStyle(appLayout);
  return {
    inner: [innerWidth, innerHeight],
    visualViewport: [visualViewport.width, visualViewport.height, visualViewport.scale],
    devicePixelRatio,
    appAttributes: app ? [...app.attributes].map((a) => a.name).filter((n) => n.startsWith('is-')) : null,
    titleBar: rect('nav[data-ytmd-main-panel]'),
    navBar: rect("ytmusic-app-layout > [slot='nav-bar']"),
    navBarBackground: rect('#nav-bar-background'),
    appLayout: rect('ytmusic-app-layout'),
    guideSpacer: rect('#guide-spacer'),
    guide: rect('ytmusic-guide-renderer'),
    playerBar: rect('ytmusic-player-bar'),
    vars: styles && Object.fromEntries(
      ['--ytmusic-nav-bar-height', '--ytmusic-player-bar-height', '--menu-bar-height', 'height', 'overflow-y']
        .map((name) => [name, styles.getPropertyValue(name).trim()]),
    ),
  };
};

// Per command: count, average and maximum round trip (ms)
const bridgeStats = () => {
  const page = window.wrappedJSObject;
  if (!page.__pearBridgeStats) return null;
  const stats = JSON.parse(page.JSON.stringify(page.__pearBridgeStats));
  return Object.fromEntries(
    Object.entries(stats).map(([cmd, { count, totalMs, maxMs }]) => [
      cmd,
      [count, Math.round((totalMs / count) * 10) / 10, Math.round(maxMs)],
    ]),
  );
};

const report = () => {
  const signInButton = document.querySelector('a[href*="ServiceLogin"], ytmusic-guide-signin-promo-renderer');
  browser.runtime.sendNativeMessage('pear', {
    url: location.href,
    rendererLoaded: !!window.wrappedJSObject.__pearRendererLoaded,
    shim: typeof window.wrappedJSObject.ipcRenderer,
    signedIn: !!document.querySelector('ytmusic-settings-button img') && !signInButton,
    bridge: bridgeStats(),
    layout: layout(),
  });
};
report();
setInterval(report, 10000);
