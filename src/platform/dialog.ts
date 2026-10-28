// Native file and folder pickers, opened by the Rust shell
// (`pear:show-open-dialog` in tauri-poc/src-tauri/src/dialogs.rs).
import { ipcRenderer } from './ipc';

export interface OpenDialogOptions {
  title?: string;
  defaultPath?: string;
  filters?: { name: string; extensions: string[] }[];
  /** `openFile` (the default) or `openDirectory`; `createDirectory` is implied */
  properties?: ('openFile' | 'openDirectory' | 'createDirectory')[];
}

export interface OpenDialogResult {
  canceled: boolean;
  filePaths: string[];
}

export const showOpenDialog = async (
  options: OpenDialogOptions,
): Promise<OpenDialogResult> =>
  (await ipcRenderer.invoke(
    'pear:show-open-dialog',
    options,
  )) as OpenDialogResult;
