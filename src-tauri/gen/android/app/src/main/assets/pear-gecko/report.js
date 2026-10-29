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
  const attributes = (selector) => {
    const element = document.querySelector(selector);
    return element ? [...element.attributes].map((a) => `${a.name}=${a.value}`.slice(0, 80)) : null;
  };
  return {
    appAttributes: attributes('ytmusic-app'),
    navBarAttributes: attributes("ytmusic-app-layout > [slot='nav-bar']"),
    playerBarAttributes: attributes("ytmusic-app-layout > [slot='player-bar']"),
    contentAttributes: attributes("ytmusic-app-layout > [slot='content']"),
    playerAttributes: attributes('ytmusic-player-page #player'),
    playerWrapperAttributes: attributes('ytmusic-player-page #music-player-wrapper'),
    playerSizing: (() => {
      const player = document.querySelector('ytmusic-player-page #player');
      if (!player) return null;
      const style = getComputedStyle(player);
      return ['width', 'height', 'max-width', 'min-width', 'aspect-ratio', 'flex', '--ytmusic-player-page-player-height']
        .map((name) => `${name}: ${style.getPropertyValue(name).trim()}`).join('; ');
    })(),
    guideAttributes: attributes("ytmusic-app-layout > [slot='guide']"),
    guideRendererAttributes: attributes("[slot='guide'] ytmusic-guide-renderer"),
    guideParts: [...document.querySelectorAll("[slot='guide'] #contentContainer > *, [slot='guide'] ytmusic-guide-renderer > *")].map(describe),
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

// YTM's viewport size classes (its media query table); which change
// events Gecko delivers to lists that are kept referenced
const mediaEvents = [];
const mediaLists = [
  '(max-width: 479px)',
  '(min-width: 480px) and (max-width: 615px)',
  '(min-width: 616px) and (max-width: 935px)',
  '(min-width: 936px) and (max-width: 1149px)',
  '(min-width: 1150px) and (max-width: 1363px)',
  '(min-width: 1364px) and (max-width: 1577px)',
  '(min-width: 1578px) and (max-width: 1799px)',
  '(min-width: 1800px)',
].map((query, index) => {
  const list = matchMedia(query);
  list.addListener((event) => {
    mediaEvents.push(`${Math.round(performance.now())}ms class ${index - 4} ${event.matches}`);
    if (mediaEvents.length > 12) mediaEvents.shift();
  });
  return list;
});

// Ad blocking: how often the player showed an ad, and whether the player
// response the page holds still lists any
let adsShown = 0;
const watchAds = () => {
  const player = document.getElementById('movie_player');
  if (!player) return setTimeout(watchAds, 2000);
  let showing = player.classList.contains('ad-showing');
  new MutationObserver(() => {
    const now = player.classList.contains('ad-showing');
    if (now && !showing) adsShown += 1;
    showing = now;
  }).observe(player, { attributes: true, attributeFilter: ['class'] });
};
watchAds();
const responseAds = () => {
  try {
    const response = window.wrappedJSObject.document.getElementById('movie_player')?.getPlayerResponse?.();
    if (!response) return null;
    return (response.adPlacements?.length ?? 0) + (response.playerAds?.length ?? 0);
  } catch {
    return null;
  }
};

// SponsorBlock: forward jumps of the video (seeks), with the song
const seeks = [];
let lastTime = 0;
const watchSeeks = () => {
  const video = document.querySelector('video');
  if (!video) return setTimeout(watchSeeks, 2000);
  video.addEventListener('timeupdate', () => {
    if (!video.seeking) lastTime = video.currentTime;
  });
  video.addEventListener('seeked', () => {
    const id = new URL(location.href).searchParams.get('v');
    seeks.push(`${id} ${Math.round(lastTime)}s->${Math.round(video.currentTime)}s`);
    if (seeks.length > 8) seeks.shift();
  });
};
watchSeeks();

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
    mediaEvents,
    ads: { shown: adsShown, inResponse: responseAds() },
    seeks,
    // Playback, to tell whether it continues in the background
    media: (() => {
      const video = document.querySelector('video');
      return video && {
        at: new Date().toISOString().slice(11, 19),
        time: Math.round(video.currentTime),
        paused: video.paused,
        readyState: video.readyState,
        bufferedEnd: video.buffered.length ? Math.round(video.buffered.end(video.buffered.length - 1)) : null,
        segmentRequests: performance.getEntriesByType('resource')
          .filter((entry) => entry.name.includes('googlevideo.com')).length,
        visibility: document.visibilityState,
      };
    })(),
    ytmViewport: document.querySelector('ytmusic-app')?.hasAttribute('is-guide-persistent'),
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
