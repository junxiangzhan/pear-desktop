// Pear's own mobile UI for YouTube Music.
//
// YouTube Music's interface is not loaded: its scripts are stopped as the
// page starts, the UI calls YouTube Music's API itself and plays through
// YouTube's own player (see engine/). The UI is drawn in a shadow root on
// the page YTM's HTML leaves (its ytcfg, cookies and origin are what the API
// and the player need).
//
// Off on desktop: see enable.ts.
import { render } from 'solid-js/web';

import { isMobileUiEnabled } from './enable';
import {
  blockYtmInterface,
  PLAYER_CSS,
  SKIP_AD_TARGET,
  startEngine,
} from './engine';
import { App } from './ui/App';
import { installRouter } from './ui/router';
import { s } from './ui/strings';
import css from './ui/styles.css?inline';

// What YTM's HTML still has of its (never started) app
const HIDE_YTM = `
ytmusic-app {
  display: none !important;
}
`;

// Plugins' places in the UI (light DOM, shown through the UI's slots):
// they fill the box the UI gives them
const PLUGIN_MOUNTS = `
pear-mobile-ui > #synced-lyrics-container {
  display: block;
  height: 100%;
}
pear-mobile-ui > :is(#pear-visualizer, #pear-ambient-mode) {
  position: relative;
  display: block;
  width: 100%;
  height: 100%;
}
`;

const HIDE_TITLE_BAR = `
nav#ytmd-title-bar-main-panel {
  display: none !important;
}
:root {
  --menu-bar-height: 0px !important;
}
`;

const onDomReady = (run: () => void) => {
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', run, { once: true });
  } else {
    run();
  }
};

const adopt = (target: Document | ShadowRoot, cssText: string) => {
  const sheet = new CSSStyleSheet();
  sheet.replaceSync(cssText);
  target.adoptedStyleSheets = [...target.adoptedStyleSheets, sheet];
};

const HOST_TAG = 'pear-mobile-ui';

const isEditable = (target: EventTarget | undefined) =>
  target instanceof HTMLElement &&
  (target.isContentEditable ||
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement);

/**
 * Keys typed in the UI's text fields reach the page's key listeners (Pear's
 * plugins, the player) retargeted to the host, which is not a text field:
 * space could toggle playback while typing a search. They are stopped at the
 * window in the capture phase, as soon as the page starts. Typing is the
 * keys' default action, which stopping them leaves alone; the UI has no key
 * listeners of its own (Enter submits the search form by default too).
 */
const keepTypingFromYtm = () => {
  for (const type of ['keydown', 'keypress', 'keyup']) {
    window.addEventListener(
      type,
      (event) => {
        const path = event.composedPath();
        if (
          isEditable(path[0]) &&
          path.some((node) => (node as Element).localName === HOST_TAG)
        ) {
          event.stopImmediatePropagation();
        }
      },
      { capture: true },
    );
  }
};

/**
 * "Skip ad" buttons are <label>s for YouTube's skip button: a tap on a label
 * reaches its control as the user's own click, which YouTube requires. A
 * label only finds its control in the same tree, so they are children of the
 * host (the page's tree), shown inside the UI through slots.
 */
const skipLabels = (host: HTMLElement) => {
  // The player page shows YouTube's own skip button on the video
  for (const where of ['mini']) {
    const label = document.createElement('label');
    label.htmlFor = SKIP_AD_TARGET;
    label.slot = `skip-ad-${where}`;
    label.className = 'skip-ad';
    label.textContent = s('skipAd');
    // The mini player opens the full one on a tap
    label.addEventListener('click', (event) => event.stopPropagation());
    host.append(label);
  }
};

const mount = () => {
  const host = document.createElement(HOST_TAG);
  skipLabels(host);
  document.body.append(host);
  const root = host.attachShadow({ mode: 'open' });
  adopt(root, css);
  render(() => <App />, root);
  return host;
};

export const startMobileUi = () => {
  if (!isMobileUiEnabled()) return;
  // As the page starts: before YTM's scripts are added
  blockYtmInterface();
  installRouter();
  keepTypingFromYtm();
  adopt(
    document,
    HIDE_YTM +
      PLUGIN_MOUNTS +
      PLAYER_CSS(HOST_TAG) +
      // Android: the avatar's sheet has in-app-menu's menu, so its title bar
      // goes and the UI takes its height
      ((window as { __PEAR_MOBILE__?: boolean }).__PEAR_MOBILE__
        ? HIDE_TITLE_BAR
        : ''),
  );
  onDomReady(() => {
    startEngine(mount()).catch(console.error);
  });
};
