<script lang="ts">
  import { onMount } from 'svelte';

  import type { SettingsWriteValues } from '../api';
  import type { components } from '../generated/api';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  type NormalizedRegion = components['schemas']['NormalizedRegion'];
  type StickName = components['schemas']['StickName'];
  type TouchscreenArea = NonNullable<SettingsWriteValues['input.touchscreen_area']>;
  type DragMode = 'capture' | 'download' | 'pixel' | 'stick' | 'touch';

  interface Props {
    fps: number;
    guideVisible: boolean;
    leftStickEnabled: boolean;
    liveViewEnabled: boolean;
    oncapture: (region: NormalizedRegion) => void;
    ondownload: (region: NormalizedRegion) => void;
    ontoucharea: (area: TouchscreenArea) => void;
    pixelValuesVisible: boolean;
    rightStickEnabled: boolean;
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  interface Point {
    readonly x: number;
    readonly y: number;
  }

  interface Drag {
    readonly mode: DragMode;
    readonly pointerId: number;
    readonly start: Point;
    readonly stick: StickName | null;
  }

  interface PixelSample {
    readonly blue: number;
    readonly green: number;
    readonly hue: number;
    readonly red: number;
    readonly saturation: number;
    readonly value: number;
    readonly x: number;
    readonly y: number;
  }

  let {
    fps,
    guideVisible,
    leftStickEnabled,
    liveViewEnabled,
    oncapture,
    ondownload,
    ontoucharea,
    pixelValuesVisible,
    rightStickEnabled,
    runtime,
    view
  }: Props = $props();
  let animationFrame: number | null = null;
  let canvas: HTMLCanvasElement | undefined;
  let current = $state<Point | null>(null);
  let drag = $state<Drag | null>(null);
  let fallbackImage: HTMLImageElement | null = null;
  let fallbackUrl: string | null = null;
  let lastDrawAt = 0;
  let lastStickAt = 0;
  let pendingStick: Point | null = null;
  let pixelSample = $state<PixelSample | null>(null);
  let renderFrame: number | null = null;
  let sourceHeight = $state(720);
  let sourceWidth = $state(1280);
  let video: HTMLVideoElement | undefined;

  const selection = $derived.by(() => {
    if (drag === null || current === null || drag.mode === 'pixel' || drag.mode === 'stick') {
      return null;
    }
    const left = Math.min(drag.start.x, current.x);
    const top = Math.min(drag.start.y, current.y);
    return {
      height: Math.abs(drag.start.y - current.y),
      left,
      top,
      width: Math.abs(drag.start.x - current.x)
    };
  });

  $effect(() => {
    const resolution = view.state?.camera_resolution ?? '1280x720';
    const [widthText, heightText] = resolution.split('x');
    const width = Number(widthText);
    const height = Number(heightText);
    if (Number.isSafeInteger(width) && Number.isSafeInteger(height) && width > 0 && height > 0) {
      sourceWidth = width;
      sourceHeight = height;
    }
  });

  $effect(() => {
    if (video === undefined || video.srcObject === view.media.stream) return;
    video.srcObject = view.media.stream;
    if (view.media.stream !== null) void video.play().catch(() => undefined);
  });

  $effect(() => {
    if (liveViewEnabled && view.media.mode === 'fallback' && fallbackImage !== null) {
      draw(fallbackImage, fallbackImage.naturalWidth, fallbackImage.naturalHeight);
    }
  });

  function clamp(value: number): number {
    return Math.min(1, Math.max(0, value));
  }

  function point(event: PointerEvent): Point {
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    return {
      x: clamp((event.clientX - bounds.left) / Math.max(1, bounds.width)),
      y: clamp((event.clientY - bounds.top) / Math.max(1, bounds.height))
    };
  }

  function context(): CanvasRenderingContext2D | null {
    return canvas?.getContext('2d', { willReadFrequently: true }) ?? null;
  }

  function draw(source: CanvasImageSource, width: number, height: number): void {
    if (!liveViewEnabled || canvas === undefined || width <= 0 || height <= 0) return;
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
    sourceWidth = width;
    sourceHeight = height;
    context()?.drawImage(source, 0, 0, width, height);
  }

  function render(timestamp: number): void {
    renderFrame = requestAnimationFrame(render);
    if (
      !liveViewEnabled ||
      view.media.mode !== 'webrtc' ||
      video === undefined ||
      video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA ||
      timestamp - lastDrawAt < 1000 / Math.max(1, fps)
    ) {
      return;
    }
    lastDrawAt = timestamp;
    draw(video, video.videoWidth, video.videoHeight);
    runtime.markVideoActivity();
  }

  function receiveFallback(frame: ArrayBuffer | Blob): void {
    const blob = frame instanceof Blob ? frame : new Blob([frame], { type: 'image/jpeg' });
    const nextUrl = URL.createObjectURL(blob);
    const image = new Image();
    image.onload = () => {
      if (fallbackUrl !== nextUrl) {
        URL.revokeObjectURL(nextUrl);
        return;
      }
      fallbackImage = image;
      draw(image, image.naturalWidth, image.naturalHeight);
    };
    image.onerror = () => URL.revokeObjectURL(nextUrl);
    const previousUrl = fallbackUrl;
    fallbackUrl = nextUrl;
    image.src = nextUrl;
    if (previousUrl !== null) URL.revokeObjectURL(previousUrl);
  }

  function stickPosition(position: Point, start: Point): Point {
    const rawX = (position.x - start.x) / 0.18;
    const rawY = (position.y - start.y) / 0.18;
    const distance = Math.hypot(rawX, rawY);
    const scale = distance > 1 ? 1 / distance : 1;
    return { x: rawX * scale, y: rawY * scale };
  }

  function flushStick(timestamp: number): void {
    animationFrame = null;
    if (drag?.mode !== 'stick' || drag.stick === null || pendingStick === null) return;
    if (timestamp - lastStickAt < 16) {
      animationFrame = requestAnimationFrame(flushStick);
      return;
    }
    const position = stickPosition(pendingStick, drag.start);
    pendingStick = null;
    lastStickAt = timestamp;
    runtime.setGamepadStick(
      drag.stick,
      Math.round(128 + position.x * 127),
      Math.round(128 + position.y * 127)
    );
  }

  function scheduleStick(position: Point): void {
    pendingStick = position;
    animationFrame ??= requestAnimationFrame(flushStick);
  }

  function stickFor(event: PointerEvent): StickName | null {
    if (event.button === 2 && rightStickEnabled) return 'RSTICK';
    if (event.button !== 0) return null;
    if (leftStickEnabled) return 'LSTICK';
    return rightStickEnabled ? 'RSTICK' : null;
  }

  function pointerDown(event: PointerEvent): void {
    const start = point(event);
    let mode: DragMode;
    let stick: StickName | null = null;
    if (event.ctrlKey && event.button === 2) mode = 'touch';
    else if (event.ctrlKey && event.shiftKey && event.button === 0) mode = 'capture';
    else if (event.ctrlKey && event.altKey && event.button === 0) mode = 'download';
    else if (event.ctrlKey && event.button === 0) mode = 'pixel';
    else {
      stick = stickFor(event);
      if (stick === null) return;
      mode = 'stick';
    }
    event.preventDefault();
    const target = event.currentTarget as HTMLElement;
    if (typeof target.setPointerCapture === 'function') target.setPointerCapture(event.pointerId);
    drag = { mode, pointerId: event.pointerId, start, stick };
    current = start;
    if (mode === 'stick') scheduleStick(start);
  }

  function pointerMove(event: PointerEvent): void {
    if (drag?.pointerId !== event.pointerId) return;
    event.preventDefault();
    const next = point(event);
    current = next;
    if (drag.mode === 'stick') scheduleStick(next);
  }

  function toRegion(start: Point, end: Point): NormalizedRegion | null {
    const x = Math.min(start.x, end.x);
    const y = Math.min(start.y, end.y);
    const width = Math.abs(start.x - end.x);
    const height = Math.abs(start.y - end.y);
    return width <= 0.002 || height <= 0.002 ? null : { height, width, x, y };
  }

  function sample(position: Point): void {
    const drawing = context();
    if (drawing === null || canvas === undefined || canvas.width === 0 || canvas.height === 0) {
      return;
    }
    const x = Math.min(canvas.width - 1, Math.floor(position.x * canvas.width));
    const y = Math.min(canvas.height - 1, Math.floor(position.y * canvas.height));
    const [red = 0, green = 0, blue = 0] = drawing.getImageData(x, y, 1, 1).data;
    const maximum = Math.max(red, green, blue) / 255;
    const minimum = Math.min(red, green, blue) / 255;
    const difference = maximum - minimum;
    let hue = 0;
    if (difference > 0) {
      if (maximum === red / 255) hue = 60 * (((green - blue) / 255 / difference) % 6);
      else if (maximum === green / 255) hue = 60 * ((blue - red) / 255 / difference + 2);
      else hue = 60 * ((red - green) / 255 / difference + 4);
    }
    pixelSample = {
      blue,
      green,
      hue: hue < 0 ? hue + 360 : hue,
      red,
      saturation: maximum === 0 ? 0 : difference / maximum,
      value: maximum,
      x,
      y
    };
  }

  function finish(event: PointerEvent): void {
    if (drag?.pointerId !== event.pointerId) return;
    event.preventDefault();
    const completed = drag;
    const end = point(event);
    if (completed.mode === 'stick' && completed.stick !== null) {
      runtime.setGamepadStick(completed.stick, 128, 128);
    } else if (completed.mode === 'pixel') {
      sample(end);
    } else {
      const region = toRegion(completed.start, end);
      if (region !== null && completed.mode === 'capture') oncapture(region);
      else if (region !== null && completed.mode === 'download') ondownload(region);
      else if (region !== null && completed.mode === 'touch') {
        ontoucharea({
          bottom: region.y + region.height,
          left: region.x,
          right: region.x + region.width,
          top: region.y
        });
      }
    }
    cancelPendingStick();
    drag = null;
    current = null;
  }

  function cancelPendingStick(): void {
    pendingStick = null;
    if (animationFrame !== null) cancelAnimationFrame(animationFrame);
    animationFrame = null;
  }

  function cancel(event: PointerEvent): void {
    if (drag?.pointerId !== event.pointerId) return;
    if (drag.mode === 'stick' && drag.stick !== null) {
      runtime.setGamepadStick(drag.stick, 128, 128);
    }
    cancelPendingStick();
    drag = null;
    current = null;
  }

  onMount(() => {
    const unsubscribe = runtime.onFallbackFrame(receiveFallback);
    renderFrame = requestAnimationFrame(render);
    return () => {
      unsubscribe();
      if (renderFrame !== null) cancelAnimationFrame(renderFrame);
      cancelPendingStick();
      if (fallbackUrl !== null) URL.revokeObjectURL(fallbackUrl);
    };
  });
</script>

<div class="space-y-2">
  <div class="relative overflow-hidden rounded-xl border border-white/10 bg-black shadow-inner shadow-black/60">
    <video bind:this={video} class="pointer-events-none absolute size-px opacity-0" autoplay muted playsinline aria-hidden="true" onloadeddata={() => runtime.markVideoActivity()}></video>
    <canvas
      bind:this={canvas}
      class="block aspect-video w-full touch-none bg-[radial-gradient(circle_at_center,#172033,#080b12)] object-contain"
      width={sourceWidth}
      height={sourceHeight}
      aria-label="Camera capture area"
      oncontextmenu={(event) => event.preventDefault()}
      onpointercancel={cancel}
      onpointerdown={pointerDown}
      onpointermove={pointerMove}
      onpointerup={finish}
    ></canvas>
    {#if guideVisible}
      <div class="pointer-events-none absolute inset-0 bg-[linear-gradient(90deg,transparent_33%,rgba(255,255,255,.15)_33.2%,transparent_33.4%,transparent_66%,rgba(255,255,255,.15)_66.2%,transparent_66.4%),linear-gradient(0deg,transparent_33%,rgba(255,255,255,.15)_33.2%,transparent_33.4%,transparent_66%,rgba(255,255,255,.15)_66.2%,transparent_66.4%)]" aria-hidden="true"></div>
    {/if}
    {#if selection !== null}
      <div
        class="pointer-events-none absolute border border-yellow-300 bg-yellow-300/10"
        style={`left: ${String(selection.left * 100)}%; top: ${String(selection.top * 100)}%; width: ${String(selection.width * 100)}%; height: ${String(selection.height * 100)}%`}
        aria-hidden="true"
      ></div>
    {/if}
    {#if !liveViewEnabled}
      <span class="absolute top-3 left-3 rounded-md bg-black/60 px-2 py-1 text-xs text-slate-300">Live view paused</span>
    {:else if !(view.state?.camera_opened ?? false)}
      <span class="absolute inset-0 grid place-items-center text-sm text-slate-500">Camera unavailable</span>
    {/if}
    <span class="absolute right-3 bottom-3 rounded-md bg-black/60 px-2 py-1 text-[10px] tracking-wider text-slate-400 uppercase">{view.media.mode} · {String(sourceWidth)}×{String(sourceHeight)}</span>
  </div>

  {#if pixelValuesVisible && pixelSample !== null}
    <output class="block rounded-lg bg-black/20 px-3 py-2 font-mono text-xs text-slate-300">
      ({String(pixelSample.x)}, {String(pixelSample.y)}) RGB {String(pixelSample.red)}, {String(pixelSample.green)}, {String(pixelSample.blue)} · HSV {pixelSample.hue.toFixed(1)}°, {(pixelSample.saturation * 100).toFixed(1)}%, {(pixelSample.value * 100).toFixed(1)}%
    </output>
  {/if}
  <p class="text-[11px] text-slate-500">Ctrl+click: pixel · Ctrl+Shift+drag: captures · Ctrl+Alt+drag: download · Ctrl+right-drag: touch area</p>
</div>
