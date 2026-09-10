<script lang="ts">
  import { onMount } from 'svelte';

  import type { SettingsWriteValues } from '../api';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  interface Props {
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { runtime, view }: Props = $props();
  let error = $state<string | null>(null);
  let saving = $state(false);
  const pressedKeys: string[] = [];
  const keyboardEnabled = $derived(view.settings?.values['input.keyboard_enabled'] ?? true);

  function interactiveTarget(target: EventTarget | null): boolean {
    return (
      target instanceof HTMLElement &&
      (target.isContentEditable ||
        ['BUTTON', 'INPUT', 'SELECT', 'TEXTAREA'].includes(target.tagName))
    );
  }

  function keydown(event: KeyboardEvent): void {
    if (
      !keyboardEnabled ||
      event.repeat ||
      event.code.length === 0 ||
      event.code === 'Tab' ||
      event.metaKey ||
      event.ctrlKey ||
      event.altKey ||
      interactiveTarget(event.target) ||
      pressedKeys.includes(event.code)
    ) {
      return;
    }
    event.preventDefault();
    pressedKeys.push(event.code);
    runtime.setKeyboardKey(event.code, true);
  }

  function keyup(event: KeyboardEvent): void {
    const index = pressedKeys.indexOf(event.code);
    if (index < 0) return;
    event.preventDefault();
    pressedKeys.splice(index, 1);
    runtime.setKeyboardKey(event.code, false);
  }

  function releaseKeyboard(): void {
    for (const code of pressedKeys.splice(0)) runtime.setKeyboardKey(code, false);
  }

  function neutralize(): void {
    pressedKeys.splice(0);
    runtime.neutralizeInput();
  }

  function focusController(): void {
    const panel = document.getElementById('software-controller');
    if (panel === null) return;
    panel.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    panel.querySelector<HTMLElement>('button')?.focus({ preventScroll: true });
  }

  $effect(() => {
    if (!keyboardEnabled) releaseKeyboard();
  });

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

  onMount(() => {
    const visibility = (): void => {
      if (document.visibilityState === 'hidden') neutralize();
    };
    window.addEventListener('blur', neutralize);
    window.addEventListener('keydown', keydown);
    window.addEventListener('keyup', keyup);
    document.addEventListener('visibilitychange', visibility);
    return () => {
      window.removeEventListener('blur', neutralize);
      window.removeEventListener('keydown', keydown);
      window.removeEventListener('keyup', keyup);
      document.removeEventListener('visibilitychange', visibility);
      releaseKeyboard();
    };
  });
</script>

<div class="space-y-4">
  <div>
    <p class="text-xs font-semibold tracking-[0.2em] text-lime-300 uppercase">Manual Control</p>
    <h2 class="mt-2 text-2xl font-semibold text-white">入力制御</h2>
    <p class="mt-2 text-sm text-slate-400">ブラウザのキーコードとポインター操作を、現在の {view.input.route ?? 'standby'} input generation へ送信します。</p>
  </div>

  {#if error !== null}
    <div class="rounded-lg border border-red-400/20 bg-red-400/10 px-3 py-2 text-sm text-red-200" role="alert">{error}</div>
  {/if}

  <fieldset class="grid gap-3 rounded-xl border border-white/10 bg-white/[0.025] p-4 sm:grid-cols-3" disabled={saving}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-slate-400 uppercase">Software input sources</legend>
    <label class="flex items-start gap-3 rounded-lg bg-black/15 p-3">
      <input class="mt-1" type="checkbox" checked={keyboardEnabled} onchange={(event) => changeToggle(event, 'input.keyboard_enabled')} />
      <span><span class="block text-sm font-medium text-slate-200">Keyboard</span><span class="text-xs text-slate-500">Tab とフォーム入力、Ctrl/Alt/Meta shortcut は除外</span></span>
    </label>
    <label class="flex items-start gap-3 rounded-lg bg-black/15 p-3">
      <input class="mt-1" type="checkbox" checked={view.settings?.values['input.left_stick_mouse_enabled'] ?? false} onchange={(event) => changeToggle(event, 'input.left_stick_mouse_enabled')} />
      <span><span class="block text-sm font-medium text-slate-200">L-stick mouse</span><span class="text-xs text-slate-500">Camera canvas の左ドラッグ</span></span>
    </label>
    <label class="flex items-start gap-3 rounded-lg bg-black/15 p-3">
      <input class="mt-1" type="checkbox" checked={view.settings?.values['input.right_stick_mouse_enabled'] ?? false} onchange={(event) => changeToggle(event, 'input.right_stick_mouse_enabled')} />
      <span><span class="block text-sm font-medium text-slate-200">R-stick mouse</span><span class="text-xs text-slate-500">Camera canvas の右ドラッグ</span></span>
    </label>
  </fieldset>

  <div class="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-white/10 bg-black/20 px-4 py-3 text-xs">
    <span class="text-slate-400">Active keyboard codes: {view.input.snapshot.keyboard_keys.length === 0 ? 'none' : view.input.snapshot.keyboard_keys.join(', ')}</span>
    <div class="flex flex-wrap items-center gap-2">
      <button type="button" class="rounded-lg bg-white/5 px-3 py-2 font-medium text-slate-200 hover:bg-white/10" onclick={focusController}>Focus software controller</button>
      <button type="button" class="rounded-lg bg-red-400/10 px-3 py-2 font-medium text-red-200" onclick={neutralize}>Release all input</button>
    </div>
  </div>

  <p class="text-xs text-slate-500">Live controller input uses the single Software-Controller in the right column.</p>
</div>
