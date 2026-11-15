// Layers that animate out as they animate in: the player, the sheets and the
// toast stay on screen for their exit animation after they are closed.
import {
  createEffect,
  createSignal,
  type JSX,
  onCleanup,
  Show,
  untrack,
} from 'solid-js';

/** The exit animations' length (styles.css: `.leaving`) */
const EXIT_MS = 180;

/**
 * Shows `when`'s value like a keyed `<Show>`, and keeps the last one on
 * screen (marked `.leaving`) for its exit animation once it is gone. The
 * wrapper is `display: contents`: the layer's own boxes lay out as before.
 */
export const Presence = <T,>(props: {
  when: T | null | undefined | false;
  children: (value: T) => JSX.Element;
}) => {
  const [shown, setShown] = createSignal<T | null>(
    untrack(() => props.when) || null,
  );
  const [leaving, setLeaving] = createSignal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  createEffect(() => {
    const value = props.when;
    clearTimeout(timer);
    if (value) {
      setLeaving(false);
      setShown(() => value);
    } else if (untrack(shown) !== null) {
      setLeaving(true);
      timer = setTimeout(() => {
        setShown(null);
        setLeaving(false);
      }, EXIT_MS);
    }
  });
  onCleanup(() => clearTimeout(timer));
  return (
    <Show keyed when={shown()}>
      {(value) => (
        <div class="presence" classList={{ leaving: leaving() }}>
          {props.children(value as T)}
        </div>
      )}
    </Show>
  );
};
