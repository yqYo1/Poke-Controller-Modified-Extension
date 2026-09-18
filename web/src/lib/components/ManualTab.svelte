<script lang="ts">
  import type { SettingsWriteValues } from '../api';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  interface Props {
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { runtime, view }: Props = $props();
  let error = $state<string | null>(null);
  let saving = $state(false);
  const keyboardEnabled = $derived(view.settings?.values['input.keyboard_enabled'] ?? true);

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
</script>

<div class="space-y-4">
  <div>
    <p class="text-xs font-semibold tracking-[0.12em] text-green uppercase">Manual Control</p>
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

  <div class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-text/10 bg-crust/20 px-4 py-3 text-xs">
    <span class="text-subtext1">Active keyboard codes: {view.input.snapshot.keyboard_keys.length === 0 ? 'none' : view.input.snapshot.keyboard_keys.join(', ')}</span>
    <div class="flex flex-wrap items-center gap-2">
      <button type="button" class="rounded-lg bg-text/5 px-3 py-2 font-medium text-text hover:bg-text/10" onclick={focusController}>Focus software controller</button>
      <button type="button" class="rounded-lg bg-red/10 px-3 py-2 font-medium text-red" onclick={neutralize}>Release all input</button>
    </div>
  </div>

  <p class="text-xs text-subtext0">Live controller input uses the single Software-Controller in the right column.</p>
</div>
