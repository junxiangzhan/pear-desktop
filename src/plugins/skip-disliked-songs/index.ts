import { t } from '@/i18n';
import { isMobileUiEnabled } from '@/mobile-ui/enable';
import { playerControls } from '@/types/player-controls';
import { createPlugin } from '@/utils';
import { waitForElement } from '@/utils/wait-for-element';

export default createPlugin<
  unknown,
  {
    observer?: MutationObserver;
    unwatch?: () => void;
    watchEngine?: () => boolean;
    start(): void;
    stop(): void;
  }
>({
  name: () => t('plugins.skip-disliked-songs.name'),
  description: () => t('plugins.skip-disliked-songs.description'),
  restartNeeded: false,
  renderer: {
    start() {
      // The mobile UI: its engine's like state (YTM's like button is not in
      // the page); a song that comes up disliked gives way to the next
      const watchEngine = () => {
        this.watchEngine = undefined;
        const controls = playerControls();
        if (!controls) return false;
        this.unwatch = controls.onLikeChange((status) => {
          if (status === 'DISLIKE') controls.next();
        });
        return true;
      };
      if (!watchEngine()) {
        this.watchEngine = watchEngine;
        document.addEventListener('pear:player-controls', watchEngine, {
          once: true,
        });
      }
      // YTM's like button never comes in the mobile UI (the wait would poll
      // for good). Its controls may arrive after this, so ask whether it runs
      if (isMobileUiEnabled()) return;
      waitForElement<HTMLElement>('#like-button-renderer').then(
        (dislikeBtn) => {
          this.observer = new MutationObserver(() => {
            if (dislikeBtn?.getAttribute('like-status') == 'DISLIKE') {
              document
                .querySelector<HTMLButtonElement>('yt-icon-button.next-button')
                ?.click();
            }
          });
          this.observer.observe(dislikeBtn, {
            attributes: true,
            childList: false,
            subtree: false,
          });
        },
      );
    },
    stop() {
      this.observer?.disconnect();
      this.unwatch?.();
      if (this.watchEngine) {
        document.removeEventListener('pear:player-controls', this.watchEngine);
        this.watchEngine = undefined;
      }
    },
  },
});
