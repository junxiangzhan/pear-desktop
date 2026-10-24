// Stands in for src/plugins/downloader/menu.ts: the same menu, with three
// changes. The folder pickers use the async dialog (`showOpenDialogSync`
// cannot wait on IPC); Android has no folder pickers (files go to
// Music/Pear Desktop); and the presets explain what Source and mp3 save
// (mp3 is encoded with LAME, there is no ffmpeg; Source is the Tauri default).
import { dialog } from 'electron';

import { t } from '@/i18n';
import { defaultConfig, type DownloaderPluginConfig } from '@/plugins/downloader';
// The original module: the backend proxy skips imports from this file
import { onMenu as originalMenu } from '@/plugins/downloader/menu';

import type { MenuTemplate } from '@/menu';
import type { MenuContext } from '@/types/contexts';

type MenuItem = MenuTemplate[number];

const pickFolder = async (defaultPath?: string) => {
  const result = await dialog.showOpenDialog({
    properties: ['openDirectory', 'createDirectory'],
    defaultPath,
  });
  return result.canceled ? undefined : result.filePaths[0];
};

export const onMenu = async (
  context: MenuContext<DownloaderPluginConfig>,
): Promise<MenuTemplate> => {
  const { getConfig, setConfig } = context;
  const config = await getConfig();
  const mobile = (window as { __PEAR_MOBILE__?: boolean }).__PEAR_MOBILE__ === true;
  const chooseFolder = t('plugins.downloader.menu.choose-download-folder');

  const replaceFolderPicker = (
    items: MenuItem[],
    current: string | undefined,
    save: (folder: string) => void,
  ) =>
    items.flatMap((item): MenuItem[] => {
      if (item.label !== chooseFolder) return [item];
      if (mobile) return [];
      return [
        {
          ...item,
          async click() {
            const folder = await pickFolder(current);
            if (folder) save(folder);
          },
        },
      ];
    });

  const template = replaceFolderPicker(
    await originalMenu(context),
    config.downloadFolder,
    (folder) => setConfig({ downloadFolder: folder }),
  );
  return template.map((item) => {
    if (!Array.isArray(item.submenu)) return item;
    if (item.label === t('plugins.downloader.menu.download-finish-settings.label')) {
      return {
        ...item,
        submenu: replaceFolderPicker(
          item.submenu,
          config.downloadOnFinish?.folder ?? config.downloadFolder,
          (folder) =>
            setConfig({
              downloadOnFinish: {
                ...defaultConfig.downloadOnFinish!,
                ...config.downloadOnFinish,
                folder,
              },
            }),
        ),
      };
    }
    if (item.label === t('plugins.downloader.menu.presets')) {
      return {
        ...item,
        submenu: [
          {
            label: "Tauri: Source keeps YouTube's .m4a; mp3 re-encodes it (bigger, not better)",
            enabled: false,
          },
          { type: 'separator' },
          ...item.submenu,
        ],
      };
    }
    return item;
  });
};
