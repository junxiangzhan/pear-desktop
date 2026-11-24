import prompt from 'custom-electron-prompt';
import { Innertube } from '\u0079\u006f\u0075\u0074\u0075\u0062\u0065i.js';

import { t } from '@/i18n';
import { getNetFetchAsFetch } from '@/plugins/utils/main';
import {
  mintPoToken,
  setupYoutubeiEvaluator,
} from '@/plugins/utils/main/youtubei';
import promptOptions from '@/providers/prompt-options';
import { createPlugin, LoggerPrefix } from '@/utils';

import { type VolumeControl, VolumeFader } from './fader';
import { clearTimer, setRepeatingTimer, setTimer } from './timers';

import type { RendererContext } from '@/types/contexts';
import type { QueueElement } from '@/types/queue';
import type { BrowserWindow } from 'electron';

export type CrossfadePluginConfig = {
  enabled: boolean;
  fadeInDuration: number;
  fadeOutDuration: number;
  secondsBeforeEnd: number;
  fadeScaling: 'linear' | 'logarithmic' | number;
};

export default createPlugin<
  unknown,
  unknown,
  {
    config?: CrossfadePluginConfig;
    ipc?: RendererContext<CrossfadePluginConfig>['ipc'];
  },
  CrossfadePluginConfig
>({
  name: () => t('plugins.crossfade.name'),
  description: () => t('plugins.crossfade.description'),
  restartNeeded: true,
  config: {
    enabled: false,
    /**
     * The duration of the fade in and fade out in milliseconds.
     *
     * @default 1500ms
     */
    fadeInDuration: 1500,
    /**
     * The duration of the fade in and fade out in milliseconds.
     *
     * @default 5000ms
     */
    fadeOutDuration: 5000,
    /**
     * The duration of the fade in and fade out in seconds.
     *
     * @default 10s
     */
    secondsBeforeEnd: 10,
    /**
     * The scaling algorithm to use for the fade.
     * (or a positive number in dB)
     *
     * @default 'linear'
     */
    fadeScaling: 'linear',
  },
  menu({ window, getConfig, setConfig }) {
    const promptCrossfadeValues = async (
      win: BrowserWindow,
      options: CrossfadePluginConfig,
    ): Promise<Omit<CrossfadePluginConfig, 'enabled'> | undefined> => {
      const res = await prompt(
        {
          title: t('plugins.crossfade.prompt.options'),
          type: 'multiInput',
          multiInputOptions: [
            {
              label: t(
                'plugins.crossfade.prompt.options.multi-input.fade-in-duration',
              ),
              value: options.fadeInDuration,
              inputAttrs: {
                type: 'number',
                required: true,
                min: '0',
                step: '100',
              },
            },
            {
              label: t(
                'plugins.crossfade.prompt.options.multi-input.fade-out-duration',
              ),
              value: options.fadeOutDuration,
              inputAttrs: {
                type: 'number',
                required: true,
                min: '0',
                step: '100',
              },
            },
            {
              label: t(
                'plugins.crossfade.prompt.options.multi-input.seconds-before-end',
              ),
              value: options.secondsBeforeEnd,
              inputAttrs: {
                type: 'number',
                required: true,
                min: '0',
              },
            },
            {
              label: t(
                'plugins.crossfade.prompt.options.multi-input.fade-scaling.label',
              ),
              selectOptions: {
                linear: t(
                  'plugins.crossfade.prompt.options.multi-input.fade-scaling.linear',
                ),
                logarithmic: t(
                  'plugins.crossfade.prompt.options.multi-input.fade-scaling.logarithmic',
                ),
              },
              value: options.fadeScaling,
            },
          ],
          resizable: true,
          height: 360,
          ...promptOptions(),
        },
        win,
      ).catch(console.error);

      if (!res) {
        return undefined;
      }

      let fadeScaling: 'linear' | 'logarithmic' | number;
      if (res[3] === 'linear' || res[3] === 'logarithmic') {
        fadeScaling = res[3];
      } else if (isFinite(Number(res[3]))) {
        fadeScaling = Number(res[3]);
      } else {
        fadeScaling = options.fadeScaling;
      }

      return {
        fadeInDuration: Number(res[0]),
        fadeOutDuration: Number(res[1]),
        secondsBeforeEnd: Number(res[2]),
        fadeScaling,
      };
    };

    return [
      {
        label: t('plugins.crossfade.menu.advanced'),
        async click() {
          const newOptions = await promptCrossfadeValues(
            window,
            await getConfig(),
          );
          if (newOptions) {
            setConfig(newOptions);
          }
        },
      },
    ];
  },

  async backend({ ipc }) {
    setupYoutubeiEvaluator();
    const yt = await Innertube.create({
      fetch: getNetFetchAsFetch(),
    });

    ipc.handle('audio-url', async (videoID: string) => {
      try {
        const poToken = await mintPoToken(videoID);
        const info = await yt.getBasicInfo(videoID, {
          client: 'YTMUSIC',
          po_token: poToken,
        });
        const url = new URL(
          await info
            .chooseFormat({ type: 'audio', quality: 'best' })
            .decipher(yt.session.player),
        );
        url.searchParams.set('pot', poToken);
        return url.toString();
      } catch (error) {
        console.error(
          LoggerPrefix,
          `[crossfade] Failed to get the stream URL of ${videoID}:`,
          error,
        );
        return '';
      }
    });
  },

  renderer: {
    async start({ ipc, getConfig }) {
      this.config = await getConfig();
      this.ipc = ipc;
    },
    onConfigChange(newConfig) {
      this.config = newConfig;
    },
    onPlayerApiReady(api) {
      const video = document.querySelector('video')!;

      // The current song as a second, silent stream. Near the end of a song
      // it takes over the old song, which then fades out on it while the
      // video already plays the next song.
      let transitionAudio: HTMLAudioElement | undefined;
      let transitionVideoID: string | undefined;
      let currentVideoID: string | undefined;
      // The end trigger moved the old song to the transition audio and
      // skipped to the next song; the navigation it causes finishes the handover
      let handedOver = false;
      let handoverTimeout: number | undefined;
      // A handover that got no song change back: don't retry it for this song
      let handoverGaveUpFor: string | undefined;
      // Outgoing streams. Each keeps its starting volume and follows the
      // user's volume relative to when its fade-out began, so volume changes
      // during a fade-out still apply to it. It does not follow the video's
      // target: YouTube Music rewrites that for the next song (loudness
      // normalization, sometimes briefly unnormalized first), which made the
      // outgoing song briefly louder after its fade-out began.
      type FadingOut = { start: number; user: number; level: number };
      const fadingOut = new Map<HTMLAudioElement, FadingOut>();
      const userVolume = () => (api.isMuted() ? 0 : api.getVolume() / 100);
      const applyFadingOut = (audio: HTMLAudioElement, entry: FadingOut) => {
        const scale = entry.user > 0 ? userVolume() / entry.user : 0;
        audio.volume = Math.min(entry.start * entry.level * scale, 1);
      };

      const getVideoIDFromURL = (url: string) =>
        new URLSearchParams(url.split('?')?.at(-1)).get('v');

      const fadeOptions = (fadeDuration?: number) => ({
        fadeScaling: this.config?.fadeScaling,
        fadeDuration,
      });

      // Video volume. YouTube Music writes `video.volume` for every song
      // (slider × loudness normalization), also while crossfade holds the
      // video silent. Its value is kept as the target, and the element
      // plays at `target × gain`; crossfade only moves the gain.
      // Look the prototype setter up on every write instead of caching it:
      // plugins such as Exponential Volume patch HTMLMediaElement.prototype
      // after this runs, and a cached setter would bypass them on the video
      // while the transition audio still goes through them.
      const setElementVolume = (volume: number) =>
        Object.getOwnPropertyDescriptor(
          HTMLMediaElement.prototype,
          'volume',
        )!.set!.call(video, volume);
      let targetVolume = video.volume;
      let gain = 1;
      const applyVideoVolume = () => setElementVolume(targetVolume * gain);
      Object.defineProperty(video, 'volume', {
        configurable: true,
        get: () => targetVolume,
        set: (volume: number) => {
          if (!(volume >= 0 && volume <= 1)) {
            // Throws the native IndexSizeError
            setElementVolume(volume);
          }
          targetVolume = volume;
          applyVideoVolume();
          for (const [audio, entry] of fadingOut) {
            applyFadingOut(audio, entry);
          }
        },
      });
      const videoGain: VolumeControl = {
        get volume() {
          return gain;
        },
        set volume(level: number) {
          gain = level;
          applyVideoVolume();
        },
      };
      let videoFader: VolumeFader | undefined;
      const fadeInVideo = () => {
        videoFader?.stop();
        videoGain.volume = 0;
        videoFader = new VolumeFader(
          videoGain,
          fadeOptions(this.config?.fadeInDuration),
        );
        videoFader.fadeTo(1, () => {
          videoFader = undefined;
        });
      };

      // The video plays through the app's Web Audio graph (src/renderer.ts),
      // which adds that context's output latency: measured on Windows, a
      // plain <audio> at the same currentTime was heard ~70-90 ms earlier,
      // a brief repeat at every handover. Route the transition audio through
      // the same context, so both reach the output with the same delay.
      // Element volume still applies before the source node.
      let sharedContext: AudioContext | undefined;
      const routedAudio = new WeakMap<
        HTMLAudioElement,
        MediaElementAudioSourceNode
      >();
      const routeThroughSharedContext = (audio: HTMLAudioElement) => {
        if (!sharedContext || routedAudio.has(audio)) return;
        try {
          const source = sharedContext.createMediaElementSource(audio);
          source.connect(sharedContext.destination);
          routedAudio.set(audio, source);
        } catch (error) {
          console.warn(LoggerPrefix, 'Crossfade could not route audio', error);
        }
      };
      document.addEventListener('peard:audio-can-play', (event) => {
        sharedContext = event.detail.audioContext;
        if (transitionAudio) routeThroughSharedContext(transitionAudio);
      });

      const unload = (audio: HTMLAudioElement | undefined) => {
        if (!audio) return;
        fadingOut.delete(audio);
        routedAudio.get(audio)?.disconnect();
        routedAudio.delete(audio);
        audio.pause();
        audio.removeAttribute('src');
        audio.load();
      };

      const fadeOut = (audio: HTMLAudioElement) => {
        fadingOut.set(audio, {
          start: audio.volume,
          user: userVolume(),
          level: 1,
        });
        const outGain: VolumeControl = {
          get volume() {
            return fadingOut.get(audio)?.level ?? 0;
          },
          set volume(level: number) {
            const entry = fadingOut.get(audio);
            if (!entry) return; // unloaded meanwhile
            entry.level = level;
            applyFadingOut(audio, entry);
          },
        };
        new VolumeFader(
          outGain,
          fadeOptions(this.config?.fadeOutDuration),
        ).fadeTo(0, () => unload(audio));
      };

      // Only crossfade into a song that will follow: at the end of the queue
      // with autoplay off (or on repeat one) skipping has nothing to go to,
      // and the handover would cut the last song off early. With autoplay
      // on, skipping past the last song starts a new radio queue. Without
      // the queue store, assume there is a next song, as before.
      const hasNextSong = () => {
        const queue = document.querySelector<QueueElement>('#queue')?.queue
          ?.store?.store?.getState()?.queue;
        if (!queue) return true;
        if (queue.repeatMode === 'ONE') return false;
        return (
          queue.autoplay ||
          queue.repeatMode === 'ALL' ||
          queue.selectedItemIndex < queue.items.length - 1 ||
          queue.automixItems.length > 0
        );
      };

      const isReadyToCrossfade = () =>
        !!transitionAudio &&
        transitionVideoID === currentVideoID &&
        transitionAudio.readyState >= HTMLMediaElement.HAVE_FUTURE_DATA &&
        Number.isFinite(transitionAudio.duration);

      // Song time comes from the player API: YouTube Music can append the
      // next song to the same <video> buffer, and then the element's
      // currentTime and duration span songs
      const songTime = () => api.getCurrentTime();

      // The transition audio follows songTime(): both go through the same
      // Web Audio context (see routeThroughSharedContext), so at the same
      // currentTime they are heard in step.
      // A seek leaves the audio tens of ms behind, which seeking again
      // cannot fix, so small offsets are corrected through the playback rate.
      // Single currentTime readings jitter by about ±10 ms on each element,
      // and correcting from them made the copy swing ±25 ms around the song
      // (heard as a small skip or repeat at the handover). The offset is
      // instead read off a line fitted through recent readings.
      const SAMPLE_MS = 50;
      const MAX_SAMPLES = 30; // 1.5 s
      const MIN_SAMPLES = 12; // 0.6 s
      // Catch up this share of the offset per second...
      const CATCH_UP_PER_S = 0.25;
      const MAX_CORRECTION = 0.03;
      // ...on top of a slowly learned base rate for any steady drift between
      // the two streams (a few tenths of a percent measured on Windows).
      // It is kept across songs, as the drift comes from the device.
      const DRIFT_GAIN_PER_S = 0.04;
      const MAX_DRIFT = 0.02;
      const DRIFT_LEARN_BELOW = 0.05; // s of offset; above it, only catch up

      const offsetSamples: [number, number][] = [];
      setRepeatingTimer(() => {
        const audio = transitionAudio;
        if (
          !audio ||
          handedOver ||
          transitionVideoID !== currentVideoID ||
          video.paused ||
          video.seeking ||
          audio.paused ||
          audio.seeking
        ) {
          return;
        }
        offsetSamples.push([
          performance.now() / 1000,
          audio.currentTime - songTime(),
        ]);
        if (offsetSamples.length > MAX_SAMPLES) offsetSamples.shift();
      }, SAMPLE_MS);
      let driftRate = 1;
      let lastDriftUpdate: number | undefined;
      const fittedOffset = () => {
        const n = offsetSamples.length;
        if (n < MIN_SAMPLES) return undefined;
        let meanT = 0;
        let meanD = 0;
        for (const [t, d] of offsetSamples) {
          meanT += t / n;
          meanD += d / n;
        }
        let covariance = 0;
        let variance = 0;
        for (const [t, d] of offsetSamples) {
          covariance += (t - meanT) * (d - meanD);
          variance += (t - meanT) ** 2;
        }
        const slope = variance > 0 ? covariance / variance : 0;
        return meanD + (slope * ((performance.now() / 1000) - meanT));
      };
      const resetSync = () => {
        offsetSamples.length = 0;
        lastDriftUpdate = undefined;
      };
      const clamp = (value: number, limit: number) =>
        Math.min(1 + limit, Math.max(1 - limit, value));

      const syncTransitionAudio = () => {
        const audio = transitionAudio;
        if (!audio || handedOver || transitionVideoID !== currentVideoID) {
          return;
        }
        if (video.paused) {
          audio.pause();
          return;
        }
        if (audio.paused) {
          resetSync();
          audio.currentTime = songTime();
          audio.play().catch(() => {});
          return;
        }
        if (Math.abs(audio.currentTime - songTime()) > 0.5) {
          resetSync();
          audio.currentTime = songTime();
          audio.playbackRate = video.playbackRate * driftRate;
          return;
        }
        const offset = fittedOffset();
        if (offset === undefined) {
          audio.playbackRate = video.playbackRate * driftRate;
          return;
        }
        const now = performance.now() / 1000;
        if (
          lastDriftUpdate !== undefined &&
          Math.abs(offset) < DRIFT_LEARN_BELOW
        ) {
          const elapsed = Math.min(now - lastDriftUpdate, 1);
          driftRate = clamp(
            driftRate - (offset * DRIFT_GAIN_PER_S * elapsed),
            MAX_DRIFT,
          );
        }
        lastDriftUpdate = now;
        const correction = clamp(
          driftRate - (offset * CATCH_UP_PER_S),
          MAX_CORRECTION,
        );
        audio.playbackRate = video.playbackRate * correction;
      };

      // Measured: at volume 0, Chromium plays an element on a stand-in clock
      // instead of the audio output, and switching to the output when the
      // volume rises moves its currentTime by ~40-60 ms. The copy came in
      // that far ahead at every handover, heard as a brief repeat. Keep the
      // copy just above 0 (-80 dB, or far less through Exponential Volume),
      // so it stays on the output and the sync loop measures what is heard.
      const SILENT_VOLUME = 0.0001;

      const loadTransitionAudio = async (videoID: string) => {
        const url = (await this.ipc?.invoke('audio-url', videoID)) as string;
        if (videoID !== currentVideoID) return; // skipped in the meantime
        unload(transitionAudio);
        transitionAudio = undefined;
        if (!url) return; // the next song change is a plain cut

        const audio = new Audio();
        // Needed to route it through Web Audio (googlevideo sends CORS headers)
        audio.crossOrigin = 'anonymous';
        audio.preload = 'auto';
        audio.volume = SILENT_VOLUME;
        audio.addEventListener('error', () => {
          if (transitionAudio === audio) {
            transitionAudio = undefined;
            unload(audio);
          }
        });
        audio.src = url;
        routeThroughSharedContext(audio);
        transitionAudio = audio;
        transitionVideoID = videoID;
        syncTransitionAudio();
      };

      // Both play the old song for a moment, with an equal-power curve.
      // Measured on Windows: element volume changes reach the output ~130 ms
      // late and in coarse steps (tens of ms), on both paths alike. A short
      // ramp then turns into a gap when the video's 0 lands a step before the
      // audio's full level, so ramp over longer and let the incoming side lead.
      const OVERLAP_MS = 400;
      const LEAD_MS = 100;
      const HANDOVER_MS = OVERLAP_MS + LEAD_MS;
      // No song change after the skip (e.g. end of the queue): give up
      const HANDOVER_TIMEOUT_MS = 3000;

      // Element volumes can go through a curve on the way out (Exponential
      // Volume cubes them), and an equal-power curve on the levels then dips
      // by several dB mid-overlap. Aim for equal power at the output instead:
      // a never-played probe element tells which output a level gives, read
      // with the getter as it is now. That is the native one, as long as
      // volume-patching plugins start after this one (plugins start in
      // alphabetical order, and Exponential Volume does).
      const outputVolume = Object.getOwnPropertyDescriptor(
        HTMLMediaElement.prototype,
        'volume',
      )!;
      const probe = document.createElement('audio');
      const outputFor = (level: number) => {
        probe.volume = level;
        return outputVolume.get!.call(probe) as number;
      };
      const levelFor = (output: number) => {
        if (output <= 0) return 0;
        let low = 0;
        let high = 1;
        for (let i = 0; i < 16; i++) {
          const middle = (low + high) / 2;
          if (outputFor(middle) < output) low = middle;
          else high = middle;
        }
        return high;
      };
      // Shares: 0 = silent, 1 = full.
      const setOverlap = (
        audio: HTMLAudioElement,
        startGain: number,
        videoShare: number,
        audioShare: number,
      ) => {
        const full = outputFor(targetVolume * startGain);
        const videoLevel = levelFor(full * Math.sin((videoShare * Math.PI) / 2));
        videoGain.volume =
          targetVolume > 0 ? Math.min(videoLevel / targetVolume, 1) : 0;
        audio.volume = Math.max(
          levelFor(full * Math.sin((audioShare * Math.PI) / 2)),
          SILENT_VOLUME,
        );
      };
      // Measured: the page can stall for ~0.5 s right as the handover begins,
      // and a wall-clock ramp then jumps from start to end in one step (heard
      // as a hitch). Advance the ramp by at most this much per step instead,
      // so a stall only delays it.
      const MAX_RAMP_STEP_MS = 30;
      const rampClock = () => {
        let last = performance.now();
        let elapsed = 0;
        return () => {
          const now = performance.now();
          elapsed += Math.min(now - last, MAX_RAMP_STEP_MS);
          last = now;
          return elapsed;
        };
      };
      // Any offset left when the ramp starts is heard through the whole
      // overlap. The copy is still silent then, so first pull it in line with
      // a faster rate than the sync loop uses, and ramp once it holds.
      const ALIGN_TOLERANCE = 0.02; // s; currentTime reads jitter by ~10 ms
      const ALIGN_TIME_S = 0.25; // close the gap over about this long
      const MAX_ALIGN_CORRECTION = 0.15;
      const MAX_ALIGN_MS = 1500;
      const ALIGN_STEP_MS = 20;
      const ALIGN_HOLD_STEPS = 3;
      const rampProgress = (elapsedMs: number) =>
        Math.min(Math.max(elapsedMs / OVERLAP_MS, 0), 1);
      // Each handover ramp belongs to one run; a song change, pause or new
      // handover starts another run, and stale ramps stop at their next step
      let handoverRun = 0;
      // A ramp (forward or back) is moving the gains, so the timeupdate
      // fallback must not start a fade-in of its own
      let ramping = false;
      const handOver = () => {
        const audio = transitionAudio!;
        const run = ++handoverRun;
        handedOver = true;
        ramping = true;
        resetSync();
        videoFader?.stop();
        videoFader = undefined;
        audio.playbackRate = video.playbackRate;
        const startGain = gain;
        let forwardClock = rampClock();
        const giveBack = () => {
          if (run !== handoverRun || !handedOver) return;
          handedOver = false;
          handoverGaveUpFor = currentVideoID;
          ramping = true;
          const backClock = rampClock();
          const stepBack = () => {
            if (run !== handoverRun) return;
            const elapsed = backClock();
            const audioShare = 1 - rampProgress(elapsed - LEAD_MS);
            if (audioShare > 0) {
              setOverlap(audio, startGain, rampProgress(elapsed), audioShare);
              setTimer(stepBack, 10);
              return;
            }
            ramping = false;
            audio.volume = SILENT_VOLUME;
            videoGain.volume = startGain;
          };
          stepBack();
        };
        const step = () => {
          if (run !== handoverRun) return;
          if (video.paused || video.seeking) {
            // Paused or seeking mid-ramp: stay on this song, the video at
            // full gain
            handoverRun++;
            handedOver = false;
            ramping = false;
            audio.volume = SILENT_VOLUME;
            videoGain.volume = startGain;
            return;
          }
          const elapsed = forwardClock();
          const videoShare = 1 - rampProgress(elapsed - LEAD_MS);
          if (videoShare > 0) {
            setOverlap(audio, startGain, videoShare, rampProgress(elapsed));
            setTimer(step, 10);
            return;
          }
          ramping = false;
          // Exact end values: the timeupdate fallback only fades the next
          // song in from a gain of exactly 0
          videoGain.volume = 0;
          audio.volume = targetVolume * startGain;
          // Go to next video (never on repeat one, see hasNextSong)
          api.nextVideo();
          // No song change came back (e.g. end of the queue): give the old
          // song back to the video with the same overlap instead of
          // switching in one step, which clicks, and don't try again
          handoverTimeout = setTimer(giveBack, HANDOVER_TIMEOUT_MS);
        };
        // Leave enough of the song for the ramp itself
        const alignFor = Math.min(
          MAX_ALIGN_MS,
          ((audio.duration - songTime()) * 1000) - HANDOVER_MS - 300,
        );
        const alignStart = performance.now();
        let inLine = 0;
        const align = () => {
          if (run !== handoverRun) return;
          const offset = audio.currentTime - songTime();
          inLine = Math.abs(offset) < ALIGN_TOLERANCE ? inLine + 1 : 0;
          if (
            video.paused ||
            video.seeking ||
            inLine >= ALIGN_HOLD_STEPS ||
            performance.now() - alignStart > alignFor
          ) {
            audio.playbackRate = video.playbackRate;
            forwardClock = rampClock();
            step();
            return;
          }
          const correction = Math.min(
            1 + MAX_ALIGN_CORRECTION,
            Math.max(1 - MAX_ALIGN_CORRECTION, 1 - (offset / ALIGN_TIME_S)),
          );
          audio.playbackRate = video.playbackRate * correction;
          setTimer(align, ALIGN_STEP_MS);
        };
        align();
      };

      const onSongChange = (videoID: string) => {
        const previous = transitionAudio;
        // Stops any ramp still running for the previous song; the video's
        // gain is then somewhere mid-ramp, so fade the new song in from 0
        const interruptedRamp = ramping;
        handoverRun++;
        ramping = false;
        clearTimer(handoverTimeout);
        handoverGaveUpFor = undefined;
        resetSync();
        if (handedOver) {
          handedOver = false;
          if (previous) fadeOut(previous);
        } else if (previous && isReadyToCrossfade()) {
          // Manual skip: the video has already switched, so hand over at once
          previous.playbackRate = video.playbackRate;
          previous.volume = targetVolume * gain;
          videoFader?.stop();
          videoFader = undefined;
          videoGain.volume = 0;
          fadeOut(previous);
        } else {
          unload(previous);
        }
        if (interruptedRamp && !video.paused) fadeInVideo();
        transitionAudio = undefined;
        currentVideoID = videoID;
        loadTransitionAudio(videoID).catch(() => {});
      };

      window.navigation.addEventListener('navigate', (event) => {
        const nextVideoID = getVideoIDFromURL(event.destination.url ?? '');
        if (nextVideoID && nextVideoID !== currentVideoID) {
          onSongChange(nextVideoID);
        }
      });

      // The URL only follows the song on the /watch page. While another page
      // is open (home, library, search...) song changes don't navigate, so
      // follow the player itself too
      api.addEventListener('videodatachange', (name, data) => {
        if (name !== 'dataloaded') return;
        // Ask the player rather than trusting the event: after quick skips a
        // late event can name a song that is no longer current
        const nextVideoID = api.getVideoData()?.video_id ?? data?.videoId;
        if (nextVideoID && nextVideoID !== currentVideoID) {
          onSongChange(nextVideoID);
        }
      });

      video.addEventListener('timeupdate', () => {
        if (handedOver) return;
        syncTransitionAudio();
        // The new song started without a `play` event: don't stay silent
        if (gain === 0 && !videoFader && !ramping && !video.paused) {
          fadeInVideo();
        }

        const before = this.config?.secondsBeforeEnd ?? 0;
        if (
          video.paused ||
          !isReadyToCrossfade() ||
          handoverGaveUpFor === currentVideoID
        ) {
          return;
        }
        // Length from the same song's stream: `getDuration()` is sometimes
        // short right after a natural transition
        const length = transitionAudio!.duration;
        const time = songTime();
        if (
          length > before * 2 &&
          time >= length - before &&
          time < length - (HANDOVER_MS / 1000) - 0.5 &&
          hasNextSong()
        ) {
          handOver();
        }
      });

      video.addEventListener('seeked', () => {
        if (transitionAudio && !handedOver) {
          resetSync();
          transitionAudio.currentTime = songTime();
        }
      });

      video.addEventListener('pause', () => {
        transitionAudio?.pause();
        // A fade-out cannot resume in place; drop it
        for (const audio of [...fadingOut.keys()]) unload(audio);
      });

      video.addEventListener('play', () => {
        syncTransitionAudio();
        if (!handedOver) fadeInVideo();
      });

      const initialVideoID =
        getVideoIDFromURL(location.href) ?? api.getVideoData()?.video_id;
      if (initialVideoID) onSongChange(initialVideoID);
    },
  },
});
