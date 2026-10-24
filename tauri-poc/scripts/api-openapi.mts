// The api-server's OpenAPI document (`/doc`) and Swagger UI page
// (`/swagger`) for the Tauri port, whose Rust server cannot run the zod
// schemas they come from. Loads the real route modules
// (src/plugins/api-server/backend/routes) with the main-process modules
// they import stubbed out, registers them on a Hono app like
// backend/main.ts, and returns what Hono would serve. Used by
// vite.inject.config.mts, which writes both next to the renderer bundle.
import { resolve } from 'node:path';

import { createServer, type Plugin } from 'vite';

// Route handlers never run here; only their imports must resolve
const stubs: Record<string, string> = {
  'electron': `
    const stub = new Proxy({}, { get: () => () => {} });
    export const app = stub, dialog = stub, ipcMain = stub, nativeImage = stub, net = stub;
    export default stub;`,
  '@/config': 'export const get = () => undefined; export default {};',
  '@/providers/song-controls': 'export const getSongControls = () => ({});',
  'virtual:i18n': 'export const languageResources = {};',
};

const entry = `
import { swaggerUI } from '@hono/swagger-ui';
import { OpenAPIHono } from '@hono/zod-openapi';

import { APPLICATION_NAME } from '@/i18n';
import {
  registerAuth,
  registerControl,
  registerWebsocket,
} from '@/plugins/api-server/backend/routes';

export default async () => {
  const app = new OpenAPIHono();
  // Registration subscribes to IPC; any property or call is a no-op
  const context = new Proxy(function () {}, {
    get: (_, key) => (key === 'then' ? undefined : context),
    apply: () => context,
  });
  const none = () => undefined;
  registerControl(app, context, none, none, none, none);
  registerAuth(app, context);
  registerWebsocket(app, context, () => async (_, next) => await next());

  // Same as backend/main.ts
  app.openAPIRegistry.registerComponent('securitySchemes', 'bearerAuth', {
    type: 'http',
    scheme: 'bearer',
    bearerFormat: 'JWT',
  });
  app.doc('/doc', {
    openapi: '3.1.0',
    info: {
      version: '1.0.0',
      title: \`\${APPLICATION_NAME} API Server\`,
      description:
        'Note: You need to get an access token using the \`/auth/{id}\` endpoint first to call any API endpoints under \`/api\`.',
    },
    security: [{ bearerAuth: [] }],
  });
  app.get('/swagger', swaggerUI({ url: '/doc' }));

  const doc = await (await app.request('/doc')).text();
  const swagger = await (await app.request('/swagger')).text();
  return { doc, swagger };
};
`;

const stubModules = (): Plugin => ({
  name: 'pear-api-openapi-stubs',
  enforce: 'pre',
  resolveId(source) {
    if (source === 'virtual:api-openapi') return '\0virtual:api-openapi';
    const name = source.replace(/^pear-stub:/, '');
    return name in stubs ? `\0stub:${name}` : null;
  },
  load(id) {
    if (id === '\0virtual:api-openapi') return entry;
    if (id.startsWith('\0stub:')) return stubs[id.slice('\0stub:'.length)];
    return null;
  },
});

export const buildApiOpenApi = async (repoRoot: string) => {
  const server = await createServer({
    configFile: false,
    root: repoRoot,
    logLevel: 'error',
    // No file watcher: crawling the repository took half a minute
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    appType: 'custom',
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [stubModules()],
    // Otherwise the real packages load, bypassing the stubs
    ssr: { noExternal: Object.keys(stubs) },
    resolve: {
      // `@/` stubs before the generic alias, which runs ahead of plugins
      alias: [
        ...Object.keys(stubs)
          .filter((name) => name.startsWith('@/'))
          .map((name) => ({ find: new RegExp(`^${name}$`), replacement: `pear-stub:${name}` })),
        { find: '@', replacement: resolve(repoRoot, 'src') },
      ],
    },
  });
  try {
    const module = await server.ssrLoadModule('virtual:api-openapi');
    return await (module.default as () => Promise<{ doc: string; swagger: string }>)();
  } finally {
    await server.close();
  }
};
