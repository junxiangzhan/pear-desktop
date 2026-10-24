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

const describe = (element) => {
  const box = element.getBoundingClientRect();
  const style = getComputedStyle(element);
  const name = element.localName + (element.id ? `#${element.id}` : '') +
    (element.slot ? `[slot=${element.slot}]` : '') +
    (element.classList.length ? `.${[...element.classList].slice(0, 2).join('.')}` : '');
  return [name, Math.round(box.left), Math.round(box.top), Math.round(box.width), Math.round(box.height),
    style.position, style.display, style.visibility, style.opacity, style.zIndex].join(' ');
};

// The app layout's parts and the player page's, as
// "name left top width height position display visibility opacity z-index"
// (GeckoView's messages take no arrays of mixed types)
const parts = () => {
  const appLayout = document.querySelector('ytmusic-app-layout');
  const playerPage = document.querySelector('ytmusic-player-page');
  const guideEntry = document.querySelector('#mini-guide ytmusic-guide-entry-renderer, ytmusic-guide-renderer[mini] ytmusic-guide-entry-renderer');
  return {
    layoutAttributes: appLayout ? [...appLayout.attributes].map((a) => a.name) : null,
    layoutChildren: appLayout ? [...appLayout.children].map(describe) : null,
    miniGuideFirstEntry: guideEntry && describe(guideEntry),
    playerPageAttributes: playerPage ? [...playerPage.attributes].map((a) => `${a.name}=${a.value}`.slice(0, 60)) : null,
    playerPageParts: playerPage
      ? [...playerPage.querySelectorAll(':scope > *, :scope #main-panel > *, :scope .side-panel, :scope #player')].map(describe)
      : null,
  };
};

// The last window sizes, to follow floating window and rotation changes
const resizes = [];
addEventListener('resize', () => {
  resizes.push(`${Math.round(performance.now())}ms ${innerWidth}x${innerHeight}@${devicePixelRatio.toFixed(2)}`);
  if (resizes.length > 12) resizes.shift();
});

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
    parts: parts(),
    resizes,
  });
};
report();
setInterval(report, 10000);
// Also shortly after a resize settles
let resizeTimer;
addEventListener('resize', () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(report, 1500);
});
