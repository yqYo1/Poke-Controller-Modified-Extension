<script lang="ts">
  import type { SettingsWriteValues } from '../api';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';
  import SoftwareKeyboard from './SoftwareKeyboard.svelte';

  interface Props {
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { runtime, view }: Props = $props();
  let error = $state<string | null>(null);
  let saving = $state(false);
  const keyboardEnabled = $derived(view.settings?.values['input.keyboard_enabled'] ?? true);
  const language = $derived(view.settings?.values.language ?? 'ja');
  const savedTouchArea = $derived(
    view.settings?.values['input.touchscreen_area'] ?? {
      left: 0,
      top: 0,
      right: 1,
      bottom: 1
    }
  );
  let touchLeft = $state(0);
  let touchTop = $state(0);
  let touchRight = $state(1);
  let touchBottom = $state(1);

  $effect(() => {
    touchLeft = savedTouchArea.left;
    touchTop = savedTouchArea.top;
    touchRight = savedTouchArea.right;
    touchBottom = savedTouchArea.bottom;
  });

  function neutralize(): void {
    runtime.neutralizeInput();
  }

  function focusController(): void {
    const panel = document.getElementById('software-controller');
    if (panel === null) return;
    panel.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    panel.querySelector<HTMLElement>('button')?.focus({ preventScroll: true });
  }

  async function write(values: SettingsWriteValues): Promise<void> {
    saving = true;
    error = null;
    try {
      await runtime.writeSettings(values);
    } catch (reason: unknown) {
      error = reason instanceof Error ? reason.message : 'Input setting update failed';
    } finally {
      saving = false;
    }
  }

  function changeToggle(
    event: Event,
    setting:
      | 'input.keyboard_enabled'
      | 'input.left_stick_mouse_enabled'
      | 'input.right_stick_mouse_enabled'
  ): void {
    void write({ [setting]: (event.currentTarget as HTMLInputElement).checked });
  }

  function applyTouchArea(): void {
    const area = [touchLeft, touchTop, touchRight, touchBottom];
    if (area.some((value) => !Number.isFinite(value) || value < 0 || value > 1)) {
      error = 'Touch area values must be between 0 and 1';
      return;
    }
    if (touchLeft >= touchRight || touchTop >= touchBottom) {
      error = 'Touch area must keep left < right and top < bottom';
      return;
    }
    void write({
      'input.touchscreen_area': {
        left: touchLeft,
        top: touchTop,
        right: touchRight,
        bottom: touchBottom
      }
    });
  }
</script>

<div class="space-y-4">
  <div>
    <p class="text-xs font-semibold tracking-[0.12em] text-blue uppercase">Manual Control</p>
    <h2 class="mt-2 text-2xl font-semibold text-text">入力制御</h2>
    <p class="mt-2 text-sm text-subtext1">ブラウザのキーコードとポインター操作を、現在の {view.input.route ?? 'standby'} input generation へ送信します。</p>
  </div>

  {#if error !== null}
    <div class="rounded-lg border border-red/20 bg-red/10 px-3 py-2 text-sm text-red" role="alert">{error}</div>
  {/if}

  <fieldset class="grid gap-3 rounded-lg border border-text/10 bg-text/[0.025] p-4 sm:grid-cols-3" disabled={saving}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-subtext1 uppercase">Software input sources</legend>
    <label class="flex items-start gap-3 rounded-lg bg-crust/15 p-3">
      <input class="mt-1" type="checkbox" checked={keyboardEnabled} onchange={(event) => changeToggle(event, 'input.keyboard_enabled')} />
      <span><span class="block text-sm font-medium text-text">Keyboard</span><span class="text-xs text-subtext0">Tab とフォーム入力、Ctrl/Alt/Meta shortcut は除外</span></span>
    </label>
    <label class="flex items-start gap-3 rounded-lg bg-crust/15 p-3">
      <input class="mt-1" type="checkbox" checked={view.settings?.values['input.left_stick_mouse_enabled'] ?? false} onchange={(event) => changeToggle(event, 'input.left_stick_mouse_enabled')} />
      <span><span class="block text-sm font-medium text-text">L-stick mouse</span><span class="text-xs text-subtext0">Camera canvas の左ドラッグ</span></span>
    </label>
    <label class="flex items-start gap-3 rounded-lg bg-crust/15 p-3">
      <input class="mt-1" type="checkbox" checked={view.settings?.values['input.right_stick_mouse_enabled'] ?? false} onchange={(event) => changeToggle(event, 'input.right_stick_mouse_enabled')} />
      <span><span class="block text-sm font-medium text-text">R-stick mouse</span><span class="text-xs text-subtext0">Camera canvas の右ドラッグ</span></span>
    </label>
  </fieldset>

  <div class="flex flex-wrap items-center gap-2">
    <SoftwareKeyboard {runtime} {language} />
  </div>

  <div class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-text/10 bg-crust/20 px-4 py-3 text-xs">
    <span class="text-subtext1">Active keyboard codes: {view.input.snapshot.keyboard_keys.length === 0 ? 'none' : view.input.snapshot.keyboard_keys.join(', ')}</span>
    <div class="flex flex-wrap items-center gap-2">
      <button type="button" class="rounded-lg bg-text/5 px-3 py-2 font-medium text-text hover:bg-text/10" onclick={focusController}>Focus software controller</button>
      <button type="button" class="rounded-lg bg-red/10 px-3 py-2 font-medium text-red" onclick={neutralize}>Release all input</button>
    </div>
  </div>

  <section class="rounded-lg border border-text/10 bg-crust/20 p-4" aria-labelledby="touch-area-heading">
    <h3 id="touch-area-heading" class="text-xs font-semibold tracking-[0.14em] text-subtext1 uppercase">Touch screen area</h3>
    <p class="mt-1 text-xs text-subtext0">正規化した矩形（0〜1）。left &lt; right、top &lt; bottom を保ちます。</p>
    <div class="mt-3 grid gap-2 sm:grid-cols-4">
      <label class="text-xs text-subtext1">Left<input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" type="number" min="0" max="1" step="0.01" aria-label="Touch area left" bind:value={touchLeft} /></label>
      <label class="text-xs text-subtext1">Top<input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" type="number" min="0" max="1" step="0.01" aria-label="Touch area top" bind:value={touchTop} /></label>
      <label class="text-xs text-subtext1">Right<input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" type="number" min="0" max="1" step="0.01" aria-label="Touch area right" bind:value={touchRight} /></label>
      <label class="text-xs text-subtext1">Bottom<input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" type="number" min="0" max="1" step="0.01" aria-label="Touch area bottom" bind:value={touchBottom} /></label>
    </div>
    <button type="button" class="mt-3 rounded-lg bg-blue/15 px-3 py-2 text-xs font-medium text-blue disabled:opacity-40" disabled={saving} onclick={applyTouchArea}>Apply touch area</button>
  </section>

  <p class="text-xs text-subtext0">Live controller input uses the single Software-Controller in the right column.</p>
</div>
