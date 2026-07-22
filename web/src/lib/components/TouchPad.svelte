<script lang="ts">
  import type { components } from '../generated/api';

  type TouchPoint = components['schemas']['TouchPoint'];

  interface Props {
    onchange: (touch: TouchPoint | null) => void;
    touch: TouchPoint | null;
  }

  let { onchange, touch }: Props = $props();
  let activePointer: number | null = null;

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
</script>

<button
  type="button"
  class="relative aspect-[4/3] w-full touch-none overflow-hidden rounded-lg border border-white/15 bg-black/30"
  aria-label={touch === null ? 'Touchscreen 320 × 240' : `Touchscreen X ${String(touch.x)} Y ${String(touch.y)}`}
  onpointercancel={release}
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={release}
>
  <span class="absolute inset-0 bg-[linear-gradient(90deg,transparent_49%,rgba(255,255,255,.06)_50%,transparent_51%),linear-gradient(0deg,transparent_49%,rgba(255,255,255,.06)_50%,transparent_51%)] bg-[size:25%_25%]" aria-hidden="true"></span>
  {#if touch !== null}
    <span
      class="absolute size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-yellow-300 shadow-[0_0_16px_rgba(253,224,71,.8)]"
      style={`left: ${String((touch.x / 319) * 100)}%; top: ${String((touch.y / 239) * 100)}%`}
      aria-hidden="true"
    ></span>
  {/if}
</button>
