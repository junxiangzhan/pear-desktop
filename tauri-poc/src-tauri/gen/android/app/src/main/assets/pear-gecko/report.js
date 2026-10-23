// Phase 1: sends the stub's counters, the page state and layout metrics to
// GeckoActivity, which saves the latest report to files/gecko-report.json.
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

const report = () => {
  const signInButton = document.querySelector('a[href*="ServiceLogin"], ytmusic-guide-signin-promo-renderer');
  browser.runtime.sendNativeMessage('pear', {
    url: location.href,
    rendererLoaded: !!window.wrappedJSObject.__pearRendererLoaded,
    shim: typeof window.wrappedJSObject.ipcRenderer,
    signedIn: !!document.querySelector('ytmusic-settings-button img') && !signInButton,
    stub: JSON.parse(document.documentElement.dataset.pearStub ?? 'null'),
    layout: layout(),
  });
};
report();
setInterval(report, 10000);
