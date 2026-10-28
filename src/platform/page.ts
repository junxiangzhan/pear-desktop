// Runs a script in the page's main world. Electron's preload did this through
// `webFrame.executeJavaScript`; here plugins already run in the page.

// YTM enforces Trusted Types for script sinks, including eval
const scriptPolicy = window.trustedTypes?.createPolicy('pear-tauri-compat', {
  createScript: (script: string) => script,
});

export const runInPage = (code: string): unknown =>
  // oxlint-disable-next-line no-eval
  (0, eval)((scriptPolicy?.createScript(code) ?? code) as string);
