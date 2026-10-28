// Imported after `./shim`, so every plugin module evaluates with the shimmed
// globals already defined. Sends Rust the plugin defaults it needs to answer
// `peard:get-config` and to start Rust backends with the merged config.
import { allPlugins } from 'virtual:plugins';

import { invoke } from '@/platform/tauri';

export interface ManifestEntry {
  id: string;
  config: Record<string, unknown>;
  restartNeeded: boolean;
}

void allPlugins().then((stubs) => {
  const manifest: ManifestEntry[] = Object.entries(stubs).map(([id, stub]) => ({
    id,
    config: (stub.config ?? { enabled: false }) as Record<string, unknown>,
    restartNeeded: stub.restartNeeded ?? false,
  }));
  return invoke('plugins_manifest', { manifest });
}).catch((error: unknown) =>
  console.error('[pear-tauri]', 'sending the plugin manifest failed', error),
);
