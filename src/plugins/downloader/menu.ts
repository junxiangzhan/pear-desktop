import { deepmerge } from 'deepmerge-ts';

import { t } from '@/i18n';
import { showOpenDialog } from '@/platform/dialog';
import prompt from '@/platform/prompt';

import { type DownloaderPluginConfig, defaultConfig } from './index';
import { DefaultPresetList } from './types';

import type { MenuContext } from '@/types/contexts';
import type { MenuTemplate } from '@/types/menu';

type MenuItem = MenuTemplate[number];

const pickFolder = async (defaultPath?: string) => {
  const result = await showOpenDialog({
    properties: ['openDirectory', 'createDirectory'],
    defaultPath,
  });
  return result.canceled ? undefined : result.filePaths[0];
};

export const onMenu = async ({
  getConfig,
  setConfig,
}: MenuContext<DownloaderPluginConfig>): Promise<MenuTemplate> => {
  const config = await getConfig();
  // Android has no folder pickers: files go to Music/Pear Desktop
  const mobile =
    (window as { __PEAR_MOBILE__?: boolean }).__PEAR_MOBILE__ === true;
  const folderPicker = (
    current: string | undefined,
    save: (folder: string) => void,
  ): MenuItem[] =>
    mobile
      ? []
      : [
          {
            label: t('plugins.downloader.menu.choose-download-folder'),
            async click() {
              const folder = await pickFolder(current);
              if (folder) save(folder);
            },
          },
        ];

  return [
    {
      label: t('plugins.downloader.menu.download-finish-settings.label'),
      type: 'submenu',
      submenu: [
        {
          label: t(
            'plugins.downloader.menu.download-finish-settings.submenu.enabled',
          ),
          type: 'checkbox',
          checked: config.downloadOnFinish?.enabled ?? false,
          click(item) {
            setConfig({
              downloadOnFinish: {
                ...deepmerge(
                  defaultConfig.downloadOnFinish,
                  config.downloadOnFinish,
                ),
                enabled: item.checked,
              },
            });
          },
        },
        {
          type: 'separator',
        },
        ...folderPicker(
          config.downloadOnFinish?.folder ?? config.downloadFolder,
          (folder) =>
            setConfig({
              downloadOnFinish: {
                ...deepmerge(
                  defaultConfig.downloadOnFinish,
                  config.downloadOnFinish,
                ),
                folder,
              },
            }),
        ),
        {
          label: t(
            'plugins.downloader.menu.download-finish-settings.submenu.mode',
          ),
          type: 'submenu',
          submenu: [
            {
              label: t(
                'plugins.downloader.menu.download-finish-settings.submenu.seconds',
              ),
              type: 'radio',
              checked: config.downloadOnFinish?.mode === 'seconds',
              click() {
                setConfig({
                  downloadOnFinish: {
                    ...deepmerge(
                      defaultConfig.downloadOnFinish,
                      config.downloadOnFinish,
                    ),
                    mode: 'seconds',
                  },
                });
              },
            },
            {
              label: t(
                'plugins.downloader.menu.download-finish-settings.submenu.percent',
              ),
              type: 'radio',
              checked: config.downloadOnFinish?.mode === 'percent',
              click() {
                setConfig({
                  downloadOnFinish: {
                    ...deepmerge(
                      defaultConfig.downloadOnFinish,
                      config.downloadOnFinish,
                    ),
                    mode: 'percent',
                  },
                });
              },
            },
          ],
        },
        {
          label: t(
            'plugins.downloader.menu.download-finish-settings.submenu.advanced',
          ),
          async click() {
            const res = await prompt({
              title: t(
                'plugins.downloader.menu.download-finish-settings.prompt.title',
              ),
              type: 'multiInput',
              multiInputOptions: [
                {
                  label: t(
                    'plugins.downloader.menu.download-finish-settings.prompt.last-seconds',
                  ),
                  inputAttrs: {
                    type: 'number',
                    required: true,
                    min: '0',
                    step: '1',
                  },
                  value:
                    config.downloadOnFinish?.seconds ??
                    defaultConfig.downloadOnFinish!.seconds,
                },
                {
                  label: t(
                    'plugins.downloader.menu.download-finish-settings.prompt.last-percent',
                  ),
                  inputAttrs: {
                    type: 'number',
                    required: true,
                    min: '1',
                    max: '100',
                    step: '1',
                  },
                  value:
                    config.downloadOnFinish?.percent ??
                    defaultConfig.downloadOnFinish!.percent,
                },
              ],
              height: 240,
              resizable: true,
            }).catch(console.error);

            if (!res) {
              return undefined;
            }

            setConfig({
              downloadOnFinish: {
                ...deepmerge(
                  defaultConfig.downloadOnFinish,
                  config.downloadOnFinish,
                ),
                seconds: Number(res[0]),
                percent: Number(res[1]),
              },
            });
            return;
          },
        },
      ],
    },

    {
      label: t('plugins.downloader.menu.download-playlist'),
      // Answered by the page-side downloader (src/inject/downloader.ts)
      click() {
        window.ipcRenderer
          .invoke('download-playlist-request')
          .catch(console.error);
      },
    },
    ...folderPicker(config.downloadFolder, (folder) =>
      setConfig({ downloadFolder: folder }),
    ),
    {
      label: t('plugins.downloader.menu.presets'),
      submenu: [
        {
          label:
            "Source keeps YouTube's .m4a; mp3 re-encodes it (bigger, not better)",
          enabled: false,
        },
        { type: 'separator' },
        ...Object.keys(DefaultPresetList).map(
          (preset): MenuItem => ({
            label: preset,
            type: 'radio',
            checked: config.selectedPreset === preset,
            click() {
              setConfig({ selectedPreset: preset });
            },
          }),
        ),
      ],
    },
    {
      label: t('plugins.downloader.menu.skip-existing'),
      type: 'checkbox',
      checked: config.skipExisting,
      click(item) {
        setConfig({ skipExisting: item.checked });
      },
    },
  ];
};
