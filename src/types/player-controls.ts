// What the mobile UI's engine (src/mobile-ui/engine/index.ts) offers the
// rest of the page when YouTube Music's interface is not loaded: the player
// bar, the queue and the like button the renderer and the song-info provider
// used to read and press are not in the page then.

export interface PlayerControls {
  next(): void;
  previous(): void;
  shuffle(): void;
  cycleRepeat(): void;
  setLike(status: 'LIKE' | 'DISLIKE'): void;
  /** NONE, ALL or ONE */
  repeatMode(): string;
  /** LIKE, DISLIKE or INDIFFERENT, of the current song */
  likeStatus(): string;
  /** Calls `listener` on each change; returns what stops it */
  onRepeatChange(listener: (mode: string) => void): () => void;
  onLikeChange(listener: (status: string) => void): () => void;
  /** What the queue knows of a song: the title YTM shows, its album */
  songDetails(videoId: string): { title?: string; album?: string } | undefined;
  /** YouTube Music's API, for what used to ask YTM's network manager */
  innertube<T = unknown>(path: string, body: Record<string, unknown>): Promise<T>;
  /** What the player page's cover shows: the song's picture, or a video */
  cover(): Cover;
  onCoverChange(listener: (cover: Cover) => void): () => void;
  /**
   * Song or video (video-toggle): the cover shows the song's video instead
   * of its picture while on, when the song has one
   */
  setVideoMode(on: boolean): void;
}

export interface Cover {
  /** The picture's URL (the UI's cover image) */
  image?: string;
  /** Whether the cover shows the player's video (a music video, an ad) */
  video: boolean;
}

/**
 * The engine's controls, present only while the mobile UI runs; the document
 * gets `pear:player-controls` when they arrive
 */
export const playerControls = () =>
  (window as { __pearPlayerControls?: PlayerControls }).__pearPlayerControls;

/**
 * Runs `callback` with the engine's controls once they are there: only in
 * the mobile UI, whose host (for `mobileUiMount`) is in the page by then
 */
export const withPlayerControls = (
  callback: (controls: PlayerControls) => void,
) => {
  const controls = playerControls();
  if (controls) {
    callback(controls);
    return;
  }
  document.addEventListener(
    'pear:player-controls',
    () => {
      const ready = playerControls();
      if (ready) callback(ready);
    },
    { once: true },
  );
};

/**
 * A plugin's place in the mobile UI: an element the plugin renders into as
 * it would into YTM's interface. It is a child of the UI's host (the page's
 * tree, so the plugin's own stylesheet applies) shown in the UI's `slot`:
 * `synced-lyrics` (the lyrics tab), `visualizer` (over the player page's
 * cover), `ambient-mode` (behind it), `video-toggle` (over it). A mount is
 * rendered only while its place is on screen. Null without the mobile UI.
 */
export const mobileUiMount = (slot: string, id: string) => {
  const host = document.querySelector('pear-mobile-ui');
  if (!host) return null;
  let element = document.getElementById(id);
  if (!element) {
    element = Object.assign(document.createElement('div'), { id, slot });
    host.append(element);
  }
  return element;
};
