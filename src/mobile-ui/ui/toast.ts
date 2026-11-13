// Short feedback at the bottom of the UI ("Saved to …"), like YTM's toasts,
// with an optional button ("Add anyway")
import { createSignal } from 'solid-js';

export interface Toast {
  text: string;
  action?: { label: string; run: () => void };
}

const [toast, setToast] = createSignal<Toast | null>(null);
let timer: ReturnType<typeof setTimeout> | undefined;

export { toast };

export const showToast = (text: string, action?: Toast['action']) => {
  setToast({ text, action });
  clearTimeout(timer);
  // Longer with a button, so there is time to press it
  timer = setTimeout(() => setToast(null), action ? 5000 : 2500);
};

export const hideToast = () => {
  clearTimeout(timer);
  setToast(null);
};
