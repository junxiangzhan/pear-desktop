// Pear's own mobile UI over YouTube Music.
//
// YTM keeps running underneath as the engine (sign-in, playback, the queue,
// ads handling, every plugin that hooks the player); its layout is made
// invisible and the new UI is drawn over it in a shadow root, between
// in-app-menu's title bar and the bottom of the screen. YTM's popups (sign-in
// prompts, "save to playlist", toasts) still show, above the new UI.
//
// Off on desktop: see enable.ts.
import { render } from 'solid-js/web';

import { isMobileUiEnabled } from './enable';
import {
  connectPlayer,
  readAccount,
  SKIP_AD_TARGET,
  VIDEO_CSS,
  whenAppReady,
} from './engine/ytm';
import { App } from './ui/App';
import { installRouter } from './ui/router';
import { s } from './ui/strings';
import css from './ui/styles.css?inline';

// opacity (unlike visibility) cannot be undone by YTM's own rules on the
// children, and the <video> keeps playing under it
const HIDE_YTM = `
ytmusic-app-layout {
  opacity: 0 !important;
  pointer-events: none !important;
}
/* The Premium upsell bar, made for YTM's layout */
tp-yt-paper-dialog:has(> ytmusic-mealbar-promo-renderer) {
  display: none !important;
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
 * Keys typed in the UI's text fields reach YTM's own hotkey listeners
 * retargeted to the host, which is not a text field: space would toggle
 * playback while typing a search. YTM listens in the capture phase, so the
 * keys are stopped before it, at the window, as soon as the page starts.
 * Typing is the keys' default action, which stopping them leaves alone; the
 * UI has no key listeners of its own (Enter submits the search form by
 * default too).
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
  for (const where of ['mini', 'player']) {
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
};

export const startMobileUi = () => {
  if (!isMobileUiEnabled()) return;
  // Before YTM's scripts add their popstate and key listeners
  installRouter();
  keepTypingFromYtm();
  adopt(
    document,
    HIDE_YTM +
      VIDEO_CSS(HOST_TAG) +
      // Android: the avatar's sheet has in-app-menu's menu, so its title bar
      // goes and the UI takes its height
      ((window as { __PEAR_MOBILE__?: boolean }).__PEAR_MOBILE__
        ? HIDE_TITLE_BAR
        : ''),
  );
  onDomReady(() => {
    mount();
    whenAppReady().then(() => {
      readAccount();
      return connectPlayer();
    });
  });
};
