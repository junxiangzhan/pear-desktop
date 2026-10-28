import { render } from 'solid-js/web';

import { t } from '@/i18n';
import { createPlugin } from '@/utils';

import { QualitySettingButton } from './templates/quality-setting-button';

import type { MusicPlayer } from '@/types/music-player';

export default createPlugin({
  name: () => t('plugins.quality-changer.name'),
  description: () => t('plugins.quality-changer.description'),
  restartNeeded: false,
  config: {
    enabled: false,
  },

  renderer: {
    qualitySettingsButtonContainer: document.createElement('div'),
    onPlayerApiReady(api: MusicPlayer, context) {
      const chooseQuality = async (e: MouseEvent) => {
        e.stopPropagation();

        const qualityLevels = api.getAvailableQualityLevels();

        const currentIndex = qualityLevels.indexOf(api.getPlaybackQuality());

        const quality = (await context.ipc.invoke(
          'peard:quality-changer',
          api.getAvailableQualityLabels(),
          currentIndex,
        )) as {
          response: number;
        };

        if (quality.response === -1) {
          return;
        }

        const newQuality = qualityLevels[quality.response];
        api.setPlaybackQualityRange(newQuality);
        api.setPlaybackQuality(newQuality);
      };

      render(
        () => (
          <QualitySettingButton
            label={t(
              'plugins.quality-changer.renderer.quality-settings-button.label',
            )}
            onClick={chooseQuality}
          />
        ),
        this.qualitySettingsButtonContainer,
      );

      const setup = () => {
        document
          .querySelector('.top-row-buttons.ytmusic-player')
          ?.prepend(this.qualitySettingsButtonContainer);
      };

      setup();
    },
    stop() {
      document
        .querySelector('.top-row-buttons.ytmusic-player')
        ?.removeChild(this.qualitySettingsButtonContainer);
    },
  },
});
