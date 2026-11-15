// Pear's plugins in the new UI. Many plugins put their controls into YouTube
// Music's interface (its menus, player bar, player page), which the new UI
// does not load; the UI offers the same features in its own places instead,
// while the plugin is enabled, and uses the plugin's own logic for them.
import { createSignal } from 'solid-js';

import { s } from './strings';
import { showToast } from './toast';

import { currentEntry, qualityLevels, setQuality } from '../engine';

import type { MenuEntry } from '../model/types';

interface PluginConfigs {
  getPlugins(): Record<string, { enabled?: boolean } | undefined>;
}

const ipc = () =>
  (
    window as {
      ipcRenderer?: {
        on(
          channel: string,
          listener: (event: unknown, ...args: unknown[]) => void,
        ): void;
        invoke(channel: string, ...args: unknown[]): Promise<unknown>;
      };
    }
  ).ipcRenderer;

const readEnabled = () => {
  const plugins =
    (
      window as { mainConfig?: { plugins?: PluginConfigs } }
    ).mainConfig?.plugins?.getPlugins() ?? {};
  return new Set(
    Object.entries(plugins)
      .filter(([, config]) => config?.enabled)
      .map(([id]) => id),
  );
};

const [enabled, setEnabled] = createSignal(readEnabled());

// Switched from the menu while the page runs
ipc()?.on('plugin:enable', (_, id) =>
  setEnabled((ids) => new Set([...ids, String(id)])),
);
ipc()?.on('plugin:unload', (_, id) =>
  setEnabled(
    (ids) => new Set([...ids].filter((other) => other !== String(id))),
  ),
);

/** Whether a plugin is on (reactive) */
export const pluginEnabled = (id: string) => enabled().has(id);

// ---- downloader ------------------------------------------------------

const MUSIC = 'https://music.youtube.com';

/** What a menu is about, for the downloader: a song, or a playlist/album */
export interface MenuTarget {
  videoId?: string;
  playlistId?: string;
}

let feedbackShown = false;

/**
 * The downloader's own backend does the work (`download-song`,
 * `download-playlist-request`, as its button in YTM's menu calls them); its
 * progress comes back as `downloader-feedback`, shown as a toast
 */
const download = (target: MenuTarget) => {
  if (!feedbackShown) {
    feedbackShown = true;
    ipc()?.on('downloader-feedback', (_, feedback) => {
      if (typeof feedback === 'string' && feedback) showToast(feedback);
    });
  }
  if (target.videoId) {
    ipc()
      ?.invoke('download-song', `${MUSIC}/watch?v=${target.videoId}`)
      .catch(() => showToast(s('actionFailed')));
  } else if (target.playlistId) {
    ipc()
      ?.invoke(
        'download-playlist-request',
        `${MUSIC}/playlist?list=${target.playlistId}`,
      )
      .catch(() => showToast(s('actionFailed')));
  }
};

// ---- playback-speed ---------------------------------------------------

const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];

/**
 * The plugin keeps the speed itself (it puts it back whenever something
 * else changes the rate), so the UI asks it to change
 * (`pear:set-playback-speed`, plugins/playback-speed/renderer.tsx)
 */
const setSpeed = (speed: number) =>
  document.dispatchEvent(
    new CustomEvent('pear:set-playback-speed', { detail: speed }),
  );

const currentSpeed = () => document.querySelector('video')?.playbackRate ?? 1;

/** A choice of a sub-menu: run locally, the current one marked */
const choice = (
  label: string,
  selected: boolean,
  run: () => void,
): MenuEntry => ({
  label,
  icon: selected ? 'PEAR_CHECK' : undefined,
  action: { kind: 'local', run },
});

/**
 * Entries plugins add to a menu: the downloader on songs, albums and
 * playlists; speed and quality on the playing song's menu.
 * `openChoices` shows a sub-menu (the UI's menu sheet).
 */
export const pluginMenuEntries = (
  target: MenuTarget | undefined,
  playing: boolean,
  openChoices: (title: string, entries: MenuEntry[]) => void,
): MenuEntry[] => {
  const entries: MenuEntry[] = [];
  if (pluginEnabled('downloader') && (target?.videoId || target?.playlistId)) {
    entries.push({
      label: s('download'),
      icon: 'PEAR_DOWNLOAD',
      action: { kind: 'local', run: () => download(target) },
    });
  }
  if (playing && currentEntry()) {
    if (pluginEnabled('playback-speed')) {
      entries.push({
        label: s('playbackSpeed'),
        icon: 'PEAR_SPEED',
        action: {
          kind: 'local',
          keepOpen: true,
          run: () => {
            const now = currentSpeed();
            openChoices(
              s('playbackSpeed'),
              SPEEDS.map((speed) =>
                choice(`${speed}x`, Math.abs(speed - now) < 0.01, () =>
                  setSpeed(speed),
                ),
              ),
            );
          },
        },
      });
    }
    if (pluginEnabled('quality-changer')) {
      entries.push({
        label: s('quality'),
        icon: 'PEAR_QUALITY',
        action: {
          kind: 'local',
          keepOpen: true,
          run: () => {
            const { levels, labels, current } = qualityLevels();
            openChoices(
              s('quality'),
              levels.map((level, index) =>
                choice(labels[index] ?? level, level === current, () =>
                  setQuality(level),
                ),
              ),
            );
          },
        },
      });
    }
  }
  return entries;
};
