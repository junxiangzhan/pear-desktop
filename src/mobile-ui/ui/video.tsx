// Where YTM's <video> shows in the UI (an ad's picture): the topmost slot
// (the full player's, else the mini player's). The engine moves the element
// into the host and assigns it to the slot's name (see showVideo).
import {
  createEffect,
  createSignal,
  onCleanup,
  onMount,
  type JSX,
} from 'solid-js';

import { adShowing, showVideo } from '../engine/ytm';

interface Slot {
  name: string;
}

const [slots, setSlots] = createSignal<Slot[]>([]);

/** Mounted once by the App: shows the video while an ad plays */
export const AdVideo = () => {
  createEffect(() => {
    const top = slots().at(-1);
    const host = document.querySelector('pear-mobile-ui');
    if (adShowing() && top && host) showVideo(host, top.name);
    else showVideo(null);
  });
  return null;
};

export const AdVideoSlot = (props: {
  class: string;
  name: string;
  style?: JSX.CSSProperties;
  children?: JSX.Element;
}) => {
  // Registered once in the page (a ref runs before the element is attached)
  onMount(() => {
    const slot = { name: props.name };
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
