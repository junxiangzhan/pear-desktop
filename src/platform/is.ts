// Platform checks for the page, from the user agent. Same API as electron-is,
// which plugins were written against; the page bundle is never a dev build.
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
