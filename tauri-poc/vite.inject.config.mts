// Builds the renderer (plus the Electron shim) into one IIFE that the Tauri
// shell injects as an initialization script. Run from the repository root:
//   pnpm exec vite build -c tauri-poc/vite.inject.config.mts
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
    pluginLoader('renderer'),
    viteResolve({
      'virtual:i18n': i18nImporter(),
      'virtual:plugins': pluginVirtualModuleGenerator('renderer'),
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
      external: ['electron', ...builtinModules],
    },
    minify: true,
  },
  resolve: {
    alias: {
      '@': resolve(repoRoot, 'src'),
      '@assets': resolve(repoRoot, 'assets'),
    },
  },
});
