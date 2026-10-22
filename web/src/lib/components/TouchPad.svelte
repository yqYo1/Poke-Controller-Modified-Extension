<script lang="ts">
  import { onDestroy, onMount } from 'svelte';

  import type { components } from '../api/openapi';
  import { onInputNeutralized } from '../input-safety';

  type TouchPoint = components['schemas']['TouchPoint'];

  interface Props {
    onchange: (touch: TouchPoint | null) => void;
    touch: TouchPoint | null;
    actualSize?: boolean;
    stream?: MediaStream | null;
  }

  let { onchange, touch, actualSize = false, stream = null }: Props = $props();
  let activePointer: number | null = null;
  let video = $state<HTMLVideoElement>();

  $effect(() => {
    if (video === undefined) return;
    video.srcObject = actualSize ? stream : null;
    if (actualSize && stream !== null) void video.play().catch(() => undefined);
  });

  function point(event: PointerEvent): TouchPoint {
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const horizontal = Math.min(1, Math.max(0, (event.clientX - bounds.left) / bounds.width));
    const vertical = Math.min(1, Math.max(0, (event.clientY - bounds.top) / bounds.height));
    return {
      pressed: true,
      x: Math.min(319, Math.floor(horizontal * 320)),
      y: Math.min(239, Math.floor(vertical * 240))
    };
  }

  function handlePointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    activePointer = event.pointerId;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    onchange(point(event));
  }

  function handlePointerMove(event: PointerEvent): void {
    if (activePointer !== event.pointerId) return;
    event.preventDefault();
    onchange(point(event));
  }

  function release(event: PointerEvent): void {
    if (activePointer !== event.pointerId) return;
    event.preventDefault();
    activePointer = null;
    onchange(null);
  }

  function neutralize(): void {
    activePointer = null;
  }

  onMount(() => onInputNeutralized(neutralize));
  onDestroy(neutralize);
</script>

<div class={`relative touch-none overflow-hidden rounded-lg border border-text/15 bg-crust/30 ${actualSize ? 'h-[240px] w-[320px]' : 'aspect-[4/3] w-full'}`}>
  {#if actualSize && stream !== null}
    <video bind:this={video} class="absolute inset-0 h-full w-full object-cover" muted playsinline aria-hidden="true"></video>
  {/if}
  <button
    type="button"
    class="absolute inset-0 size-full touch-none"
    aria-label={touch === null ? 'Touchscreen 320 × 240' : `Touchscreen X ${String(touch.x)} Y ${String(touch.y)}`}
    onpointercancel={release}
    onpointerdown={handlePointerDown}
    onpointermove={handlePointerMove}
    onpointerup={release}
  >
  <span class="absolute inset-0 bg-[linear-gradient(90deg,transparent_49%,rgba(255,255,255,.06)_50%,transparent_51%),linear-gradient(0deg,transparent_49%,rgba(255,255,255,.06)_50%,transparent_51%)] bg-[size:25%_25%]" aria-hidden="true"></span>
  {#if touch !== null}
    <span
      class="absolute size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-yellow shadow-[0_0_8px_rgb(224_175_104_/_0.45)]"
      style={`left: ${String((touch.x / 319) * 100)}%; top: ${String((touch.y / 239) * 100)}%`}
      aria-hidden="true"
    ></span>
  {/if}
  </button>
</div>
