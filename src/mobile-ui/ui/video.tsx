// Where YouTube's player shows in the UI (an ad, the song's video in video
// mode): the topmost slot on screen (the full player's, else the mini
// player's), by its layer, not by when it mounted (an ad starting under the
// open player mounts both, in either order); each is there only while its
// place shows the video. The player's
// box is a child of the UI's host; the engine assigns it to the slot's name
// (see showVideo), and to the parking slot when no part of the UI shows it.
import {
  createEffect,
  createSignal,
  onCleanup,
  onMount,
  type JSX,
} from 'solid-js';

import { showVideo } from '../engine';

interface Slot {
  name: string;
  /** Higher is over the lower ones (the player page over the bar) */
  layer: number;
}

const [slots, setSlots] = createSignal<Slot[]>([]);

/** Mounted once by the App: puts the video in the topmost slot */
export const AdVideo = () => {
  createEffect(() => {
    // The highest layer; the latest of equal ones
    const top = slots().reduce<Slot | undefined>(
      (best, slot) => (!best || slot.layer >= best.layer ? slot : best),
      undefined,
    );
    showVideo(top?.name ?? null);
  });
  return null;
};

export const AdVideoSlot = (props: {
  class: string;
  name: string;
  /** Its place in the UI's layers: 0 the bar, 1 the player page */
  layer: number;
  style?: JSX.CSSProperties;
  children?: JSX.Element;
}) => {
  // Registered once in the page (a ref runs before the element is attached)
  onMount(() => {
    const slot = { name: props.name, layer: props.layer };
    setSlots((list) => [...list, slot]);
    onCleanup(() => setSlots((list) => list.filter((entry) => entry !== slot)));
  });
  return (
    <div class={props.class} style={props.style}>
      <slot name={props.name} />
      {props.children}
    </div>
  );
};
