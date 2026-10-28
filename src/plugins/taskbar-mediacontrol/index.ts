import { t } from '@/i18n';
import { Platform } from '@/types/plugins';
import { createPlugin } from '@/utils';

export default createPlugin({
  name: () => t('plugins.taskbar-mediacontrol.name'),
  description: () => t('plugins.taskbar-mediacontrol.description'),
  restartNeeded: true,
  platform: Platform.Windows,
  config: {
    enabled: false,
  },
});
