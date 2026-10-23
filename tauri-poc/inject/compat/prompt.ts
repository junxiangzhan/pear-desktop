// `custom-electron-prompt` for the page bundle: same call shape and result
// types (null on cancel), rendered by the Rust prompt window instead
// (`src-tauri/src/prompt.rs`, `placeholder/prompt.html`).
import { invoke } from '../tauri';

// Electron-only presentation options; the Tauri prompt has its own styling
const ELECTRON_ONLY = [
  'customStylesheet',
  'customScript',
  'icon',
  'enableRemoteModule',
  'menuBarVisible',
  'frame',
] as const;

const prompt = async (options: Record<string, unknown>, _parent?: unknown) => {
  const portable = { ...options };
  for (const key of ELECTRON_ONLY) delete portable[key];
  return invoke('prompt_open', { options: portable });
};

export default prompt;
