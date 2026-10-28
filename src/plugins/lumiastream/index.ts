import { t } from '@/i18n';
import { createPlugin } from '@/utils';

export default createPlugin({
  name: () => t('plugins.lumiastream.name'),
  description: () => t('plugins.lumiastream.description'),
  restartNeeded: true,
  config: {
    enabled: false,
  },
});
