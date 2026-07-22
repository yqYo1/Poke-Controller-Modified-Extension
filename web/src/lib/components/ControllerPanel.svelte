<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';

  import type { components } from '../generated/api';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';
  import AnalogStick from './AnalogStick.svelte';
  import TouchPad from './TouchPad.svelte';

  type ButtonState = components['schemas']['ButtonState'];
  type GamepadButton = components['schemas']['GamepadButton'];
  type GamepadHat = components['schemas']['GamepadHat'];

  interface Props {
    compact?: boolean;
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { compact = false, runtime, view }: Props = $props();
  let visuallyReleased = new SvelteSet<GamepadButton>();
  let hatVisuallyReleased = $state(false);

  const buttonFields: Readonly<Record<GamepadButton, keyof ButtonState>> = {
    A: 'a',
    B: 'b',
    CAPTURE: 'capture',
    HOME: 'home',
    L: 'l',
    LCLICK: 'lclick',
    MINUS: 'minus',
    PLUS: 'plus',
    R: 'r',
    RCLICK: 'rclick',
    X: 'x',
    Y: 'y',
    ZL: 'zl',
    ZR: 'zr'
  };

  const leftButtons: readonly GamepadButton[] = ['ZL', 'L', 'MINUS', 'CAPTURE'];
  const rightButtons: readonly GamepadButton[] = ['ZR', 'R', 'PLUS', 'HOME'];
  const faceButtons: readonly GamepadButton[] = ['X', 'Y', 'A', 'B'];
  const hats: readonly { label: string; value: GamepadHat }[] = [
    { label: '↑', value: 'UP' },
    { label: '←', value: 'LEFT' },
    { label: '→', value: 'RIGHT' },
    { label: '↓', value: 'DOWN' }
  ];

  function active(button: GamepadButton): boolean {
    return view.input.snapshot.buttons[buttonFields[button]] && !visuallyReleased.has(button);
  }

  function setVisuallyReleased(button: GamepadButton, released: boolean): void {
    if (released) visuallyReleased.add(button);
    else visuallyReleased.delete(button);
  }

  function press(event: PointerEvent, button: GamepadButton): void {
    if (event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    setVisuallyReleased(button, false);
    runtime.setGamepadButton(button, true);
  }

  function release(event: PointerEvent, button: GamepadButton): void {
    event.preventDefault();
    if (event.shiftKey) {
      setVisuallyReleased(button, true);
      return;
    }
    setVisuallyReleased(button, false);
    runtime.setGamepadButton(button, false);
  }

  function cancel(event: PointerEvent, button: GamepadButton): void {
    event.preventDefault();
    setVisuallyReleased(button, false);
    runtime.setGamepadButton(button, false);
  }

  function hatActive(hat: GamepadHat): boolean {
    if (hatVisuallyReleased) return false;
    const current = view.input.snapshot.hat;
    return (
      (hat === 'UP' && ['up', 'up_left', 'up_right'].includes(current)) ||
      (hat === 'DOWN' && ['down', 'down_left', 'down_right'].includes(current)) ||
      (hat === 'LEFT' && ['left', 'up_left', 'down_left'].includes(current)) ||
      (hat === 'RIGHT' && ['right', 'up_right', 'down_right'].includes(current))
    );
  }

  function pressHat(event: PointerEvent, hat: GamepadHat): void {
    if (event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    hatVisuallyReleased = false;
    runtime.setGamepadHat(hat);
  }

  function releaseHat(event: PointerEvent): void {
    event.preventDefault();
    if (event.shiftKey) {
      hatVisuallyReleased = true;
      return;
    }
    hatVisuallyReleased = false;
    runtime.setGamepadHat('CENTER');
  }

  function cancelHat(event: PointerEvent): void {
    event.preventDefault();
    hatVisuallyReleased = false;
    runtime.setGamepadHat('CENTER');
  }

  function hatPosition(hat: GamepadHat): string {
    switch (hat) {
      case 'UP':
        return 'col-start-2 row-start-1';
      case 'LEFT':
        return 'col-start-1 row-start-2';
      case 'RIGHT':
        return 'col-start-3 row-start-2';
      case 'DOWN':
        return 'col-start-2 row-start-3';
      default:
        return '';
    }
  }

  function facePosition(index: number): string {
    return [
      'col-start-2 row-start-1',
      'col-start-1 row-start-2',
      'col-start-3 row-start-2',
      'col-start-2 row-start-3'
    ][index] ?? '';
  }

  function changeStick(stick: components['schemas']['StickName'], x: number, y: number): void {
    runtime.setGamepadStick(stick, x, y);
  }

  function changeTouch(touch: components['schemas']['TouchPoint'] | null): void {
    runtime.setGamepadTouch(touch);
  }
</script>

<section class="overflow-hidden rounded-2xl border border-white/10 bg-ink-900/90" aria-label="Software controller">
  <div class="flex items-center justify-between border-b border-white/10 px-4 py-3">
    <div>
      <p class="text-xs font-semibold tracking-[0.16em] text-cyan-300 uppercase">Joy-Con</p>
      <h2 class="text-sm font-semibold text-white">Software Controller</h2>
    </div>
    <span class={`rounded-full px-2 py-1 text-[11px] font-medium ${view.input.ready ? 'bg-lime-300/15 text-lime-300' : 'bg-white/5 text-slate-400'}`}>
      {view.input.ready ? view.input.route : 'standby'}
    </span>
  </div>

  <div class={`grid gap-3 p-3 ${compact ? 'grid-cols-[1fr_1.35fr_1fr]' : 'grid-cols-1 sm:grid-cols-[1fr_1.4fr_1fr]'}`}>
    <div class="rounded-[1.7rem] bg-[#56CCF2] p-2 text-slate-950 shadow-lg shadow-cyan-950/30">
      <div class="grid grid-cols-2 gap-1">
        {#each leftButtons as button (button)}
          <button
            type="button"
            class={`touch-none rounded-lg border border-black/15 px-1 py-2 text-xs font-black transition ${active(button) ? 'bg-[#FFD800]' : 'bg-white/55 hover:bg-white/75'}`}
            aria-pressed={active(button)}
            onpointercancel={(event) => cancel(event, button)}
            onpointerdown={(event) => press(event, button)}
            onpointerup={(event) => release(event, button)}
          >{button}</button>
        {/each}
      </div>
      <div class="mt-3 grid place-items-center">
        <AnalogStick
          label="Left stick"
          onchange={changeStick}
          position={view.input.snapshot.left_stick}
          stick="LSTICK"
        />
        <button
          type="button"
          class={`mt-2 rounded-full px-3 py-1 text-[10px] font-black ${active('LCLICK') ? 'bg-[#FFD800]' : 'bg-white/60'}`}
          aria-pressed={active('LCLICK')}
          onpointercancel={(event) => cancel(event, 'LCLICK')}
          onpointerdown={(event) => press(event, 'LCLICK')}
          onpointerup={(event) => release(event, 'LCLICK')}
        >LCLICK</button>
      </div>
      <div class="mx-auto mt-3 grid w-24 grid-cols-3 grid-rows-3 gap-1">
        {#each hats as hat (hat.value)}
          <button
            type="button"
            class={`touch-none rounded-md py-1 text-sm font-black ${hatPosition(hat.value)} ${hatActive(hat.value) ? 'bg-[#FFD800]' : 'bg-slate-800 text-white'}`}
            aria-pressed={hatActive(hat.value)}
            aria-label={`D-pad ${hat.value.toLowerCase()}`}
            onpointercancel={cancelHat}
            onpointerdown={(event) => pressHat(event, hat.value)}
            onpointerup={releaseHat}
          >{hat.label}</button>
        {/each}
      </div>
    </div>

    <div class="flex min-w-0 flex-col justify-center gap-2 rounded-2xl border border-white/10 bg-black/20 p-2">
      <TouchPad
        onchange={changeTouch}
        touch={view.input.snapshot.touch}
      />
      <p class="text-center text-[10px] tracking-[0.12em] text-slate-500 uppercase">320 × 240 touch</p>
    </div>

    <div class="rounded-[1.7rem] bg-[#E9514E] p-2 text-slate-950 shadow-lg shadow-red-950/30">
      <div class="grid grid-cols-2 gap-1">
        {#each rightButtons as button (button)}
          <button
            type="button"
            class={`touch-none rounded-lg border border-black/15 px-1 py-2 text-xs font-black transition ${active(button) ? 'bg-[#FFD800]' : 'bg-white/55 hover:bg-white/75'}`}
            aria-pressed={active(button)}
            onpointercancel={(event) => cancel(event, button)}
            onpointerdown={(event) => press(event, button)}
            onpointerup={(event) => release(event, button)}
          >{button}</button>
        {/each}
      </div>
      <div class="mx-auto mt-3 grid w-24 grid-cols-3 grid-rows-3 gap-1">
        {#each faceButtons as button, index (button)}
          <button
            type="button"
            class={`touch-none rounded-full py-1 text-xs font-black ${facePosition(index)} ${active(button) ? 'bg-[#FFD800]' : 'bg-slate-800 text-white'}`}
            aria-pressed={active(button)}
            onpointercancel={(event) => cancel(event, button)}
            onpointerdown={(event) => press(event, button)}
            onpointerup={(event) => release(event, button)}
          >{button}</button>
        {/each}
      </div>
      <div class="mt-3 grid place-items-center">
        <AnalogStick
          label="Right stick"
          onchange={changeStick}
          position={view.input.snapshot.right_stick}
          stick="RSTICK"
        />
        <button
          type="button"
          class={`mt-2 rounded-full px-3 py-1 text-[10px] font-black ${active('RCLICK') ? 'bg-[#FFD800]' : 'bg-white/60'}`}
          aria-pressed={active('RCLICK')}
          onpointercancel={(event) => cancel(event, 'RCLICK')}
          onpointerdown={(event) => press(event, 'RCLICK')}
          onpointerup={(event) => release(event, 'RCLICK')}
        >RCLICK</button>
      </div>
    </div>
  </div>
</section>
