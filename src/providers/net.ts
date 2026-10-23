// `fetch` for renderer code that requests other origins (e.g. lyrics
// providers). Electron strips CORS headers for the whole session (see
// `removeContentSecurityPolicy` in src/index.ts), so plain `fetch` works
// there. Hosts without that override install their own implementation as
// `window.__pearCrossOriginFetch` (the Tauri port routes it through Rust).

declare global {
  interface Window {
    __pearCrossOriginFetch?: typeof fetch;
  }
}

export const crossOriginFetch: typeof fetch = (input, init) =>
  (window.__pearCrossOriginFetch ?? window.fetch)(input, init);
