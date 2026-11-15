// Whether the page shows Pear's own mobile UI instead of YouTube Music's.
//
// `options.mobileUi` in the app config, switched from the in-app menu
// (View > Pear mobile UI, Android only; the page reloads). Unset means off,
// on Android too: it is a preview. Same rule as Config::mobile_ui in
// src-tauri/src/config.rs.
//
// For development on desktop, `localStorage['pear-mobile-ui-dev'] = 'on'`
// forces it on (e.g. prepended to a bundle loaded with PEAR_INJECT_FILE).

export const isMobileUiEnabled = (): boolean => {
  try {
    if (localStorage.getItem('pear-mobile-ui-dev') === 'on') return true;
  } catch {
    // Storage blocked: no override
  }
  return window.mainConfig?.get('options.mobileUi') === true;
};
