// Stands in for src/config/plugins.ts, which reads electron-store directly:
// the same functions, backed by the Rust-owned config through `./config`.
import { plugins } from './config';

export const {
  getPlugins,
  isEnabled,
  setOptions,
  setMenuOptions,
  getOptions,
  enable,
  disable,
} = plugins;
