// Builds the renderer (plus the Electron shim) into one IIFE that the Tauri
// shell injects as an initialization script. Run from the repository root:
//   pnpm exec vite build -c tauri-poc/vite.inject.config.mts
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import builtinModules from 'builtin-modules';
import { defineConfig, withFilter, type Plugin } from 'vite';
import viteResolve from 'vite-plugin-resolve';
import solidPlugin from 'vite-plugin-solid';

import { i18nImporter } from '../vite-plugins/i18n-importer.mts';
import { pluginVirtualModuleGenerator } from '../vite-plugins/plugin-importer.mts';
import pluginLoader from '../vite-plugins/plugin-loader.mts';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, '..');
const compat = (file: string) => resolve(__dirname, 'inject/compat', file);

// Menus of plugins without a Tauri backend import main-process code
const { unavailable } = JSON.parse(
  readFileSync(resolve(__dirname, 'plugin-support.json'), 'utf8'),
) as { unavailable: string[] };

// Electron only injects the renderer's <script> (see `get-renderer-script` in
// src/index.ts), so CSS extracted from side-effect imports such as
// `mdui/mdui.css` never reaches the page there. Drop it to match: applying it
// paints an opaque `:root` background behind YTM's translucent layers.
const dropExtractedCss = (): Plugin => ({
  name: 'pear-drop-extracted-css',
  apply: 'build',
  enforce: 'post',
  generateBundle(_, bundle) {
    for (const file of Object.values(bundle)) {
      if (file.type === 'asset' && file.fileName.endsWith('.css')) {
        delete bundle[file.fileName];
      }
    }
  },
});

export default defineConfig({
  root: resolve(repoRoot, 'src'),
  plugins: [
    // The page runs what Electron splits across the preload, renderer and
    // menu-building main process; backends are reimplemented in Rust
    pluginLoader(['renderer', 'preload', 'menu'], {
      omit: { menu: unavailable, preload: unavailable },
    }),
    viteResolve({
      'virtual:i18n': i18nImporter(),
      'virtual:plugins': pluginVirtualModuleGenerator(['renderer', 'preload']),
    }),
    withFilter(solidPlugin(), {
      load: { id: [/\.(tsx|jsx)$/, '/@solid-refresh'] },
    }),
    dropExtractedCss(),
  ],
  define: {
    'process.env.NODE_ENV': JSON.stringify('production'),
  },
  build: {
    lib: {
      entry: resolve(__dirname, 'inject/entry.ts'),
      formats: ['iife'],
      name: 'pearRenderer',
      fileName: () => 'renderer.js',
    },
    outDir: resolve(__dirname, 'src-tauri/inject'),
    emptyOutDir: true,
    rolldownOptions: {
      external: [...builtinModules],
    },
    minify: true,
  },
  resolve: {
    // Exact-match compat aliases first: the generic `@` alias would win otherwise
    alias: [
      { find: /^electron$/, replacement: compat('electron.ts') },
      { find: /^electron-is$/, replacement: compat('electron-is.ts') },
      { find: /^custom-electron-prompt$/, replacement: compat('prompt.ts') },
      { find: /^@\/config$/, replacement: compat('config.ts') },
      { find: /^@\/providers\/prompt-options$/, replacement: compat('prompt-options.ts') },
      { find: '@assets', replacement: resolve(repoRoot, 'assets') },
      { find: '@', replacement: resolve(repoRoot, 'src') },
    ],
  },
});
