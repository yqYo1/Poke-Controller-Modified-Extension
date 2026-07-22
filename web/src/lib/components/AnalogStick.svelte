<script lang="ts">
  import { onDestroy } from 'svelte';

  import type { components } from '../generated/api';

  type StickName = components['schemas']['StickName'];
  type StickPosition = components['schemas']['StickPosition'];

  interface Props {
    label: string;
    onchange: (stick: StickName, x: number, y: number) => void;
    position: StickPosition;
    stick: StickName;
  }

  let { label, onchange, position, stick }: Props = $props();
  let activePointer: number | null = null;
  let animationFrame: number | null = null;
  let lastSentAt = 0;
  let pendingPosition: StickPosition | null = null;

  function pointerPosition(event: PointerEvent): StickPosition {
    const target = event.currentTarget as HTMLElement;
    const bounds = target.getBoundingClientRect();
    const radius = Math.max(1, Math.min(bounds.width, bounds.height) / 2);
    const dx = event.clientX - (bounds.left + bounds.width / 2);
    const dy = event.clientY - (bounds.top + bounds.height / 2);
    const distance = Math.hypot(dx, dy);
    const scale = distance > radius ? radius / distance : 1;
    return {
      x: Math.round(128 + (dx * scale * 127) / radius),
      y: Math.round(128 + (dy * scale * 127) / radius)
    };
  }

  function flush(timestamp: number): void {
    animationFrame = null;
    if (pendingPosition === null || activePointer === null) return;
    if (timestamp - lastSentAt < 16) {
      animationFrame = requestAnimationFrame(flush);
      return;
    }
    const next = pendingPosition;
    pendingPosition = null;
    lastSentAt = timestamp;
    onchange(stick, next.x, next.y);
  }

  function schedule(position: StickPosition): void {
    pendingPosition = position;
    animationFrame ??= requestAnimationFrame(flush);
  }

  function handlePointerDown(event: PointerEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    activePointer = event.pointerId;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    schedule(pointerPosition(event));
  }

  function handlePointerMove(event: PointerEvent): void {
    if (activePointer !== event.pointerId) return;
    event.preventDefault();
    schedule(pointerPosition(event));
  }

  function release(event: PointerEvent): void {
    if (activePointer !== event.pointerId) return;
    event.preventDefault();
    activePointer = null;
    pendingPosition = null;
    if (animationFrame !== null) cancelAnimationFrame(animationFrame);
    animationFrame = null;
    onchange(stick, 128, 128);
  }

  function handleKeydown(event: KeyboardEvent): void {
    const step = event.shiftKey ? 32 : 8;
    let { x, y } = position;
    switch (event.key) {
      case 'ArrowDown':
        y = Math.min(255, y + step);
        break;
      case 'ArrowLeft':
        x = Math.max(0, x - step);
        break;
      case 'ArrowRight':
        x = Math.min(255, x + step);
        break;
      case 'ArrowUp':
        y = Math.max(0, y - step);
        break;
      case 'Home':
        x = 128;
        y = 128;
        break;
      default:
        return;
    }
    event.preventDefault();
    onchange(stick, x, y);
  }

  onDestroy(() => {
    if (animationFrame !== null) cancelAnimationFrame(animationFrame);
  });
</script>

<button
  type="button"
  class="relative aspect-square w-full max-w-24 touch-none rounded-full border border-white/15 bg-black/25 shadow-inner shadow-black/50"
  aria-label={`${label} X ${String(position.x)} Y ${String(position.y)}`}
  onkeydown={handleKeydown}
  onpointercancel={release}
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={release}
>
  <span
    class="absolute size-10 -translate-x-1/2 -translate-y-1/2 rounded-full border border-white/20 bg-slate-600 shadow-lg transition-[left,top] duration-75"
    style={`left: ${String((position.x / 255) * 100)}%; top: ${String((position.y / 255) * 100)}%`}
    aria-hidden="true"
  ></span>
  <span class="sr-only">{label}</span>
</button>
