// `electron-is` for the page bundle: platform from the user agent, never dev.
const ua = navigator.userAgent;
const platform = {
  windows: /Windows/.test(ua),
  macOS: /Mac OS X/.test(ua) && !/Mobile/.test(ua),
  linux: /Linux|X11/.test(ua),
};

const is = {
  windows: () => platform.windows,
  macOS: () => platform.macOS,
  osx: () => platform.macOS,
  linux: () => platform.linux,
  freebsd: () => false,
  dev: () => false,
  production: () => true,
  renderer: () => true,
  main: () => false,
};

export default is;
