import { StatusDisplayType } from 'discord-api-types/v10';

import { setMenuOptions } from '@/config/plugins';
import { APPLICATION_NAME, t } from '@/i18n';
import prompt from '@/platform/prompt';
import { singleton } from '@/providers/decorators';

import { discordService } from './service';

import type { DiscordPluginConfig } from './index';
import type { MenuContext } from '@/types/contexts';
import type { MenuItem, MenuTemplate } from '@/types/menu';

const registerRefreshOnce = singleton((refreshMenu: () => void) => {
  discordService?.registerRefreshCallback(refreshMenu);
});

const DiscordStatusDisplayTypeLabels: Record<StatusDisplayType, string> = {
  [StatusDisplayType.Name]:
    'plugins.discord.menu.set-status-display-type.submenu.application',
  [StatusDisplayType.State]:
    'plugins.discord.menu.set-status-display-type.submenu.artist',
  [StatusDisplayType.Details]:
    'plugins.discord.menu.set-status-display-type.submenu.title',
};

export const onMenu = async ({
  getConfig,
  setConfig,
  refresh,
}: MenuContext<DiscordPluginConfig>): Promise<MenuTemplate> => {
  const config = await getConfig();
  registerRefreshOnce(refresh);

  return [
    {
      label: discordService?.isConnected()
        ? t('plugins.discord.menu.connected')
        : t('plugins.discord.menu.disconnected'),
      enabled: !discordService?.isConnected(),
      click: () => discordService?.connect(true),
    },
    {
      label: t('plugins.discord.menu.auto-reconnect'),
      type: 'checkbox',
      checked: config.autoReconnect,
      click(item: MenuItem) {
        setConfig({
          autoReconnect: item.checked,
        });
      },
    },
    {
      label: t('plugins.discord.menu.clear-activity'),
      click: () => discordService?.clearActivity(),
    },
    {
      label: t('plugins.discord.menu.clear-activity-after-timeout'),
      type: 'checkbox',
      checked: config.activityTimeoutEnabled,
      click(item: MenuItem) {
        setConfig({
          activityTimeoutEnabled: item.checked,
        });
      },
    },
    {
      label: t('plugins.discord.menu.play-on-application'),
      type: 'checkbox',
      checked:
        config[
          'playOn\u0059\u006f\u0075\u0054\u0075\u0062\u0065\u004d\u0075\u0073\u0069\u0063'
        ],
      click(item: MenuItem) {
        setConfig({
          'playOn\u0059\u006f\u0075\u0054\u0075\u0062\u0065\u004d\u0075\u0073\u0069\u0063':
            item.checked,
        });
      },
    },
    {
      label: t('plugins.discord.menu.hide-github-button'),
      type: 'checkbox',
      checked: config.hideGitHubButton,
      click(item: MenuItem) {
        setConfig({
          hideGitHubButton: item.checked,
        });
      },
    },
    {
      label: t('plugins.discord.menu.hide-duration-left'),
      type: 'checkbox',
      checked: config.hideDurationLeft,
      click(item: MenuItem) {
        setConfig({
          hideDurationLeft: item.checked,
        });
      },
    },
    {
      label: t('plugins.discord.menu.set-inactivity-timeout'),
      click: () => setInactivityTimeout(config),
    },
    {
      label: t('plugins.discord.menu.set-status-display-type.label'),
      submenu: Object.values(StatusDisplayType)
        .filter(
          (v) => typeof StatusDisplayType[v as StatusDisplayType] !== 'number',
        )
        .map((statusDisplayType) => ({
          label: t(
            DiscordStatusDisplayTypeLabels[
              statusDisplayType as StatusDisplayType
            ],
            {
              applicationName: APPLICATION_NAME,
            },
          ),
          type: 'radio',
          checked: config.statusDisplayType === statusDisplayType,
          click() {
            setConfig({
              statusDisplayType: statusDisplayType as StatusDisplayType,
            });
          },
        })),
    },
  ];
};

async function setInactivityTimeout(options: DiscordPluginConfig) {
  const output = await prompt({
    title: t('plugins.discord.prompt.set-inactivity-timeout.title'),
    label: t('plugins.discord.prompt.set-inactivity-timeout.label'),
    value: String(Math.round((options.activityTimeoutTime ?? 0) / 1e3)),
    type: 'counter',
    counterOptions: { minimum: 0, multiFire: true },
    width: 450,
  });

  if (output) {
    options.activityTimeoutTime = Math.round(~~output * 1e3);
    setMenuOptions('discord', options);
  }
}
