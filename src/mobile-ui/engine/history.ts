// Keeps the session history for Pear's UI: the history only holds the
// entries the UI pushed, one per layer it opened (a page, the player, a
// sheet), so the Android back button pops exactly one of them.
//
// YTM's interface, which pushed entries of its own and navigated on
// popstate, is no longer loaded (engine/boot.ts); what is left of it:
// - pushState calls from other scripts in the page become replaceState, so
//   they cannot add entries the UI does not know of;
// - popstate is caught first (capture, at document start) and handed to the
//   UI, which pops one of its layers.

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
