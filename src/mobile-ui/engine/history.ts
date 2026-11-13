// Keeps the session history for Pear's UI while YTM still navigates behind it.
//
// YTM pushes an entry for each of its navigations (playing a song goes to
// /watch) and reacts to popstate. With its UI hidden, those entries would make
// the Android back button step through pages nobody sees, and popstate would
// navigate YTM. So while the new UI runs:
// - YTM's pushState calls become replaceState: the URL still follows what YTM
//   plays (a reload, or GeckoPage reopening a killed session, lands there),
//   but the history only holds entries the new UI pushed;
// - popstate is caught before YTM sees it and handed to the new UI, which
//   pops one of its own layers (a page, the player, a sheet).
//
// This has to run before YTM's scripts register their popstate listener, as
// it does when the bundle is injected at document start (Tauri, GeckoView).

const nativePushState = history.pushState.bind(history);

let installed = false;
let onBack: () => void = () => {};

export const installHistoryIsolation = (handler: () => void) => {
  onBack = handler;
  if (installed) return;
  installed = true;

  history.pushState = function (
    this: History,
    data: unknown,
    unused: string,
    url?: string | URL | null,
  ) {
    history.replaceState(data, unused, url);
  };

  window.addEventListener(
    'popstate',
    (event) => {
      event.stopImmediatePropagation();
      onBack();
    },
    { capture: true },
  );
};

/** Adds an entry the back button will pop (handed to the handler) */
export const pushBackEntry = () => {
  nativePushState(history.state, '', location.href);
};
