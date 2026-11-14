import Butterchurn from 'butterchurn';
import ButterchurnPresets from 'butterchurn-presets';

import { Visualizer } from './visualizer';

import type { VisualizerPluginConfig } from '../index';

/**
 * How often the visualizer checks that its WebGL context still draws.
 *
 * Gecko on Android runs WebGL in its GPU process. Android ends that process
 * once the app has been in the background a while, and Gecko starts a new
 * one when it is needed again, but a context made in the old one neither
 * reports the loss (no `webglcontextlost`, `isContextLost()` stays false)
 * nor draws again: the visualizer froze on its last frame.
 */
const CONTEXT_CHECK_MS = 3000;

/**
 * Whether a context of butterchurn's has stopped working. Its contexts have
 * no alpha, so a working one reads alpha 255 back from its own framebuffer,
 * whatever was drawn; a dead one reads 0.
 */
const contextDead = (gl: WebGL2RenderingContext | null) => {
  if (!gl) return false;
  if (gl.isContextLost()) return true;
  const bound = gl.getParameter(
    gl.FRAMEBUFFER_BINDING,
  ) as WebGLFramebuffer | null;
  gl.bindFramebuffer(gl.FRAMEBUFFER, null);
  const pixel = new Uint8Array(4);
  gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
  gl.bindFramebuffer(gl.FRAMEBUFFER, bound);
  return pixel[3] === 0;
};

class ButterchurnVisualizer extends Visualizer {
  private visualizer: ReturnType<typeof Butterchurn.createVisualizer>;
  private destroyed: boolean = false;
  private animFrameHandle: number | null;
  private lastCheck = 0;

  constructor(
    audioContext: AudioContext,
    audioSource: MediaElementAudioSourceNode,
    canvas: HTMLCanvasElement,
    audioNode: GainNode,
    _stream: MediaStream,
    config: VisualizerPluginConfig,
  ) {
    super(audioSource, audioNode);

    const preset = ButterchurnPresets[config.butterchurn.preset];
    const create = (blendTime: number) => {
      const visualizer = Butterchurn.createVisualizer(audioContext, canvas, {
        width: canvas.width,
        height: canvas.height,
      });
      visualizer.loadPreset(preset, blendTime);
      visualizer.connectAudio(audioNode);
      return visualizer;
    };

    const renderVisualizer = (now: number) => {
      if (this.destroyed) return;
      // Also the first frame after the page was hidden (no frames then)
      if (now - this.lastCheck > CONTEXT_CHECK_MS) {
        this.lastCheck = now;
        if (contextDead(this.visualizer.gl)) {
          try {
            this.visualizer.disconnectAudio(audioNode);
          } catch {}
          this.visualizer = create(0);
        }
      }
      this.visualizer.render();
      this.animFrameHandle = requestAnimationFrame(renderVisualizer);
    };

    this.visualizer = create(config.butterchurn.blendTimeInSeconds);

    // Start animation request loop. Do not use setInterval!
    this.animFrameHandle = requestAnimationFrame(renderVisualizer);
  }

  resize(width: number, height: number) {
    this.visualizer.setRendererSize(width, height);
  }

  destroy() {
    if (this.animFrameHandle) cancelAnimationFrame(this.animFrameHandle);
    this.destroyed = true;
    try {
      this.audioSource.disconnect(this.audioNode);
    } catch {}
  }
}

export default ButterchurnVisualizer;
