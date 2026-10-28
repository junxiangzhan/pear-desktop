import '@total-typescript/ts-reset';

import type * as config from './config';
import type { t } from '@/i18n';
import type { IpcRenderer } from '@/platform/ipc';
import type { VideoDataChanged } from '@/types/video-data-changed';
import type { trustedTypes } from 'trusted-types';

declare global {
  interface Compressor {
    audioSource: MediaElementAudioSourceNode;
    audioContext: AudioContext;
  }

  interface DocumentEventMap {
    'peard:audio-can-play': CustomEvent<Compressor>;
    'videodatachange': CustomEvent<VideoDataChanged>;
  }

  interface Window {
    trustedTypes?: typeof trustedTypes;
    ipcRenderer: IpcRenderer;
    mainConfig: typeof config;
    /**
     * Internal variable (Last interaction time)
     */
    _lact: number;
    navigation: Navigation;
    download: () => void;
    togglePictureInPicture: () => void;
    reload: () => void;
    i18n: {
      t: typeof t;
    };
  }
}

