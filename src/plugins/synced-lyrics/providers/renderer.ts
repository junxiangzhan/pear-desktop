import { ProviderNames } from './index';
import { LRCLib } from './LRCLib';
import { LyricsGenius } from './LyricsGenius';
import { YTMusic } from './YTMusic';

export const providers = {
  [ProviderNames.YTMusic]: new YTMusic(),
  [ProviderNames.LRCLib]: new LRCLib(),
  [ProviderNames.LyricsGenius]: new LyricsGenius(),
} as const;
