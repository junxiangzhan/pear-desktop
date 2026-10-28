import { t } from '@/i18n';
import prompt, { type KeybindOptions } from '@/platform/prompt';

import type { ShortcutsPluginConfig } from './index';
import type { MenuContext } from '@/types/contexts';
import type { MenuTemplate } from '@/types/menu';

export const onMenu = async ({
  getConfig,
  setConfig,
}: MenuContext<ShortcutsPluginConfig>): Promise<MenuTemplate> => {
  const config = await getConfig();

  /**
   * Helper function for keybind prompt
   */
  const kb = (
    label_: string,
    value_: string,
    default_?: string,
  ): KeybindOptions => ({ value: value_, label: label_, default: default_ });

  async function promptKeybind(config: ShortcutsPluginConfig) {
    const output = await prompt({
      title: t('plugins.shortcuts.prompt.keybind.title'),
      label: t('plugins.shortcuts.prompt.keybind.label'),
      type: 'keybind',
      keybindOptions: [
        // If default=undefined then no default is used
        kb(
          t('plugins.shortcuts.prompt.keybind.keybind-options.previous'),
          'previous',
          config.global?.previous,
        ),
        kb(
          t('plugins.shortcuts.prompt.keybind.keybind-options.play-pause'),
          'playPause',
          config.global?.playPause,
        ),
        kb(
          t('plugins.shortcuts.prompt.keybind.keybind-options.next'),
          'next',
          config.global?.next,
        ),
      ],
      height: 270,
    });

    if (output) {
      const newConfig = { ...config };

      for (const { value, accelerator } of output) {
        newConfig.global[value as keyof ShortcutsPluginConfig['global']] =
          accelerator;
      }

      setConfig(config);
    }
    // Else -> pressed cancel
  }

  return [
    {
      label: t('plugins.shortcuts.menu.set-keybinds'),
      click: () => promptKeybind(config),
    },
    {
      label: t('plugins.shortcuts.menu.override-media-keys'),
      type: 'checkbox',
      checked: config.overrideMediaKeys,
      click: (item) => setConfig({ overrideMediaKeys: item.checked }),
    },
  ];
};
