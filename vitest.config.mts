// Unit tests for the page code (`pnpm test`): plain functions such as
// parsers, run in Node. vite.config.mts is the page bundle's build and isn't
// shared, so only the path aliases are repeated here.
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { defineConfig } from 'vitest/config';

const repoRoot = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  resolve: {
    alias: [
      { find: '@assets', replacement: resolve(repoRoot, 'assets') },
      { find: '@', replacement: resolve(repoRoot, 'src') },
    ],
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
