// Plugin "backends" that only show UI, answered in the page: they need the
// plugin's translations (i18n lives in TS) and open the Rust prompt window
// through the `custom-electron-prompt` compat. Same strings and results as
// the Electron backends they replace.
import prompt from './compat/prompt';
import { handleLocally, ipcRenderer } from './ipc';

import { t } from '@/i18n';

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
  prompt({ title, label, type: 'input' }),
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
    selectOptions: Object.fromEntries(labels.map((label, i) => [String(i), label])),
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
