import { t } from '@/i18n';

import './crossfade';
import './downloader';
import { emitLocal, handleLocally, ipcRenderer } from '@/platform/ipc';
// Plugin "backends" that only show UI, answered in the page: they need the
// plugin's translations (i18n lives in TS) and open the Rust prompt window
// (`@/platform/prompt`). Same strings and results as
// the Electron backends they replace.
import prompt from '@/platform/prompt';
import { invoke } from '@/platform/tauri';

// src/plugins/captions-selector/back.ts
handleLocally(
  'peard:captions-selector',
  async (captionLabels, currentIndex) => {
    const labels = captionLabels as Record<string, string>;
    return await prompt({
      title: t('plugins.captions-selector.prompt.selector.title'),
      label: t('plugins.captions-selector.prompt.selector.label', {
        language:
          labels[currentIndex as string] ||
          t('plugins.captions-selector.prompt.selector.none'),
      }),
      type: 'select',
      value: currentIndex,
      selectOptions: labels,
      resizable: true,
    });
  },
);

// src/plugins/music-together/index.ts (backend)
handleLocally('music-together:prompt', async (title, label) =>
  prompt({ title: title as string, label: label as string, type: 'input' }),
);

// src/plugins/quality-changer/index.tsx (backend). Electron showed one button
// per quality and answered `{ response: index }`, -1 when cancelled.
handleLocally('peard:quality-changer', async (qualityLabels, currentIndex) => {
  const labels = qualityLabels as string[];
  const index = currentIndex as number;
  const choice = await prompt({
    title: t('plugins.quality-changer.backend.dialog.quality-changer.title'),
    label: `${t('plugins.quality-changer.backend.dialog.quality-changer.message')}\n${t(
      'plugins.quality-changer.backend.dialog.quality-changer.detail',
      { quality: labels[index] },
    )}`,
    type: 'select',
    value: String(index),
    selectOptions: Object.fromEntries(
      labels.map((label, i) => [String(i), label]),
    ),
  });
  return { response: choice === null ? -1 : Number(choice) };
});

// picture-in-picture: Electron's main process blocked F and turned Esc into
// "leave PiP" via `before-input-event`; Rust can't see webview key events.
const pipKeys = (event: KeyboardEvent) => {
  const key = event.key.toLowerCase();
  if (key === 'f') {
    event.preventDefault();
    event.stopImmediatePropagation();
  } else if (key === 'escape') {
    event.preventDefault();
    event.stopImmediatePropagation();
    ipcRenderer.send('plugin:toggle-picture-in-picture');
  }
};
ipcRenderer.on('peard:pip-toggle', (_, inPiP) => {
  if (inPiP) window.addEventListener('keydown', pipKeys, true);
  else window.removeEventListener('keydown', pipKeys, true);
});

// api-server's like state: Electron read it with `executeJavaScript`
ipcRenderer.on('pear:get-like-state', () => {
  const button = document.querySelector<HTMLElement & { likeStatus?: string }>(
    '#like-button-renderer',
  );
  ipcRenderer.send('pear:like-state', button?.likeStatus ?? null);
});

// Frameless window: CSS `app-region: drag` (in-app-menu's title bar) moves the
// window, as in Electron. The nearest element with an app-region decides.
const appRegion = (target: EventTarget | null) => {
  for (let node = target as Element | null; node; node = node.parentElement) {
    const region = getComputedStyle(node)
      .getPropertyValue('-webkit-app-region')
      .trim();
    if (region === 'drag' || region === 'no-drag') return region;
  }
  return null;
};
window.addEventListener(
  'mousedown',
  (event) => {
    if (event.button !== 0 || appRegion(event.target) !== 'drag') return;
    event.preventDefault();
    const command =
      event.detail === 2 ? 'window_toggle_maximize' : 'window_start_dragging';
    invoke(command).catch(console.error);
  },
  true,
);

// in-app-menu: backtick toggles the menu (`electron-localshortcut` in Electron)
window.addEventListener('keydown', (event) => {
  if (event.key !== '`' || event.ctrlKey || event.altKey || event.metaKey)
    return;
  const target = event.target as HTMLElement | null;
  if (
    target?.isContentEditable ||
    ['INPUT', 'TEXTAREA', 'SELECT'].includes(target?.tagName ?? '')
  ) {
    return;
  }
  emitLocal('toggle-in-app-menu');
});
