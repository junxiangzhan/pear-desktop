import { For, Show } from 'solid-js';

import { AccountSheet } from './account';
import { DialogView } from './dialog';
import { Icon, type IconName } from './icons';
import { PageStack } from './pages';
import { MiniPlayer, PlayerSheet } from './player';
import { pluginEnabled } from './plugins';
import { Presence } from './presence';
import { currentTab, switchTab, type Tab } from './router';
import { MenuSheetView, SaveSheetView } from './sheets';
import { s } from './strings';
import { hideToast, toast } from './toast';
import { AdVideo } from './video';

import { PARKED_SLOT } from '../engine';

const TABS: { tab: Tab; icon: IconName }[] = [
  { tab: 'home', icon: 'home' },
  { tab: 'explore', icon: 'explore' },
  { tab: 'library', icon: 'library' },
];

const BottomNav = () => (
  <nav class="bottom-nav">
    <For each={TABS}>
      {({ tab, icon }) => (
        <button
          class="nav-item"
          classList={{ active: currentTab() === tab }}
          onClick={() => switchTab(tab)}
        >
          <span class="nav-pill">
            <Icon name={icon} />
          </span>
          <span class="nav-label">{s(tab)}</span>
        </button>
      )}
    </For>
  </nav>
);

// album-color-theme: its colors (CSS variables it sets on the page) tint the
// UI's surfaces
export const App = () => (
  <div
    class="app"
    classList={{ 'album-color': pluginEnabled('album-color-theme') }}
  >
    <main class="pages">
      <PageStack />
    </main>
    <MiniPlayer />
    <BottomNav />
    <PlayerSheet />
    <MenuSheetView />
    <SaveSheetView />
    <AccountSheet />
    <DialogView />
    <AdVideo />
    {/* The player, while no part of the UI shows it: off screen, still laid out */}
    <div aria-hidden="true" class="player-parking">
      <slot name={PARKED_SLOT} />
    </div>
    <Presence when={toast()}>
      {(shown) => (
        <div class="toast">
          <span>{shown.text}</span>
          <Show when={shown.action}>
            {(action) => (
              <button
                class="toast-action"
                onClick={() => {
                  hideToast();
                  action().run();
                }}
              >
                {action().label}
              </button>
            )}
          </Show>
        </div>
      )}
    </Presence>
  </div>
);
