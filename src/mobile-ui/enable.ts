// Whether the page shows Pear's own mobile UI instead of YouTube Music's.
//
// `options.mobileUi` in the app config, switched from the in-app menu
// (View > Pear mobile UI, Android only; the page reloads). Unset means the
// platform default: on for Android (`__PEAR_MOBILE__`), off on desktop.
//
// For development on desktop, `localStorage['pear-mobile-ui-dev'] = 'on'`
// forces it on (e.g. prepended to a bundle loaded with PEAR_INJECT_FILE).

export const isMobileUiEnabled = (): boolean => {
  try {
    if (localStorage.getItem('pear-mobile-ui-dev') === 'on') return true;
  } catch {
    // Storage blocked: no override
  }
  const option = window.mainConfig?.get('options.mobileUi');
  if (typeof option === 'boolean') return option;
  return (window as { __PEAR_MOBILE__?: boolean }).__PEAR_MOBILE__ === true;
};
