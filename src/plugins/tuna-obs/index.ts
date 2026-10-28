import { t } from '@/i18n';
import { createPlugin } from '@/utils';

export default createPlugin({
  name: () => t('plugins.tuna-obs.name'),
  description: () => t('plugins.tuna-obs.description'),
  restartNeeded: true,
  config: {
    enabled: false,
  },
});
