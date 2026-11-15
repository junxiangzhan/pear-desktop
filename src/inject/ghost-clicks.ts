// GeckoView (Android): drops the click a tap leaves behind on whatever
// opened under the finger.
//
// After a tap, the browser sends compatibility mouse events and a click at
// the same point, about 20 ms after touchend. YTM opens some popups on the tap
// itself (Polymer's `tap` fires on touchend), so that click lands on the new
// popup: on its backdrop, where YTM's overlay manager takes it as a click
// outside and closes the popup at once (Settings > Downloads > "Clear
// downloads": the confirmation flashed and vanished), or on one of its
// buttons, which would answer a confirmation nobody saw.
//
// A click the touch made has to land on what the touch started on, or on the
// control (button, link, ...) that contains it; one that does not is dropped
// before YTM sees it (window capture runs before YTM's document listeners).
// Gecko marks these clicks with `mozInputSource` (touch: 5); clicks from a
// mouse, from code, or forwarded by a label (the UI's skip ad button) pass.
// Chromium (Windows) has no `mozInputSource`: a no-op there.

const MOZ_SOURCE_TOUCH = 5;
// The click follows touchend within a frame or two; later ones are not it
const CLICK_AFTER_TOUCH_MS = 1000;
// What a tap means to press: a finger that starts on its icon and lifts on
// its padding (within the tap slop) clicks the control, not the icon
const CONTROL = [
  'a[href]',
  'button',
  'input',
  'select',
  'textarea',
  'summary',
  ...[
    'button',
    'link',
    'menuitem',
    'menuitemcheckbox',
    'menuitemradio',
    'option',
    'tab',
    'checkbox',
    'radio',
    'switch',
  ].map((role) => `[role="${role}"]`),
].join(', ');

// The flat-tree parent, as in an event's composed path
const parentOf = (node: Node): Node | null => {
  const slot = (node as Partial<Slottable>).assignedSlot;
  if (slot) return slot;
  const parent = node.parentNode;
  return parent instanceof ShadowRoot ? parent.host : parent;
};

const controlOf = (node: Node): Element | null => {
  for (let at: Node | null = node; at; at = parentOf(at)) {
    if (at instanceof Element && at.matches(CONTROL)) return at;
  }
  return null;
};

if ('mozInputSource' in MouseEvent.prototype) {
  let touched: EventTarget | null = null;
  let touchEnd = Number.NEGATIVE_INFINITY;
  const options = { capture: true, passive: true };
  window.addEventListener(
    'touchstart',
    (event) => {
      touched = event.composedPath()[0] ?? null;
    },
    options,
  );
  window.addEventListener(
    'touchend',
    (event) => {
      touchEnd = event.timeStamp;
    },
    options,
  );
  window.addEventListener(
    'click',
    (event) => {
      const source = (event as MouseEvent & { mozInputSource?: number })
        .mozInputSource;
      if (
        !event.isTrusted ||
        source !== MOZ_SOURCE_TOUCH ||
        event.timeStamp - touchEnd > CLICK_AFTER_TOUCH_MS ||
        // Replaced by a re-render: nothing to compare with
        !(touched instanceof Node) ||
        !touched.isConnected
      ) {
        return;
      }
      const path = event.composedPath();
      if (path.includes(touched)) return;
      // Lifted elsewhere on the control the touch started in. Only up to
      // that control: a click on any ancestor would let through the one
      // landing beside a popup the tap opened, which closes it
      const control = controlOf(touched);
      if (control && path.includes(control)) return;
      // A label passes its click on to its control
      const label =
        touched instanceof Element ? touched.closest('label') : null;
      if (label?.control && path.includes(label.control)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
    },
    true,
  );
}
