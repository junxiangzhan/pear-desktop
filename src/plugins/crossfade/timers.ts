// Timers that run on a worker thread.
//
// The fades and the sync loop need steady 10–50 ms ticks. Window timers
// don't give that: the Performance Improvement plugin holds them until the
// next animation frame, and Chromium slows them while the window is hidden.
// With the window minimized, a 10 ms step can take a third of a second,
// which stretches the handover and lets the transition copy drift.
// A dedicated worker's timers are not held back by either, so the ticks
// come from there. If a worker can't be started, window timers are used.

type Entry = { callback: () => void; repeat: boolean; fallback?: number };

const WORKER_SOURCE = `
const timers = new Map();
onmessage = ({ data: [id, ms, repeat] }) => {
  if (ms < 0) {
    clearTimeout(timers.get(id));
    clearInterval(timers.get(id));
    timers.delete(id);
    return;
  }
  const fire = () => {
    if (!repeat) timers.delete(id);
    postMessage(id);
  };
  timers.set(id, repeat ? setInterval(fire, ms) : setTimeout(fire, ms));
};
`;

const entries = new Map<number, Entry>();
let nextId = 1;
let worker: Worker | null | undefined;

const getWorker = () => {
  if (worker !== undefined) return worker;
  try {
    const url = URL.createObjectURL(
      new Blob([WORKER_SOURCE], { type: 'text/javascript' }),
    );
    worker = new Worker(url);
    worker.onmessage = ({ data: id }: MessageEvent<number>) => {
      const entry = entries.get(id);
      if (!entry) return;
      if (!entry.repeat) entries.delete(id);
      entry.callback();
    };
  } catch {
    worker = null;
  }
  return worker;
};

const start = (callback: () => void, ms: number, repeat: boolean) => {
  const id = nextId++;
  const entry: Entry = { callback, repeat };
  entries.set(id, entry);
  const thread = getWorker();
  if (thread) {
    thread.postMessage([id, ms, repeat]);
  } else if (repeat) {
    entry.fallback = window.setInterval(callback, ms);
  } else {
    entry.fallback = window.setTimeout(() => {
      entries.delete(id);
      callback();
    }, ms);
  }
  return id;
};

export const setTimer = (callback: () => void, ms: number) =>
  start(callback, ms, false);

export const setRepeatingTimer = (callback: () => void, ms: number) =>
  start(callback, ms, true);

export const clearTimer = (id: number | undefined) => {
  if (id === undefined) return;
  const entry = entries.get(id);
  if (!entry) return;
  entries.delete(id);
  if (entry.fallback !== undefined) {
    window.clearTimeout(entry.fallback);
    window.clearInterval(entry.fallback);
  } else {
    worker?.postMessage([id, -1, false]);
  }
};
