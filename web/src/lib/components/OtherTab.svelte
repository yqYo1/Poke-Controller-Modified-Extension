<script lang="ts">
  import { browser } from '$app/environment';

  import type { SettingsWriteValues } from '../api';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  type SelectSetting =
    | 'ui.stdout_destination'
    | 'ui.widget_mode'
    | 'ui.controller_position'
    | 'ui.dialog_button_position'
    | 'ui.desktop.close_behavior'
    | 'language';
  type BooleanSetting = 'auto_reload_config' | 'ui.desktop.disable_compositing';

  interface Props {
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { runtime, view }: Props = $props();
  let busy = $state(false);
  let error = $state<string | null>(null);

  const values = $derived(view.settings?.values);
  const english = $derived(values?.language === 'en');
  const desktopMode = browser && '__TAURI_INTERNALS__' in window;
  const savedCompositing = $derived(
    view.settings?.pending_restart_values['ui.desktop.disable_compositing'] ??
      values?.['ui.desktop.disable_compositing'] ??
      false
  );
  const compositingRestart = $derived(
    view.settings?.restart_required.includes('ui.desktop.disable_compositing') ?? false
  );

  const widgetModes = [
    ['all', 'All', 'すべて'],
    ['outputs', 'Outputs', '出力 #1 + #2'],
    ['output_1_controller', 'Output #1 + controller', '出力 #1 + コントローラー'],
    ['output_2_controller', 'Output #2 + controller', '出力 #2 + コントローラー'],
    ['output_1', 'Output #1', '出力 #1'],
    ['output_2', 'Output #2', '出力 #2'],
    ['controller', 'Controller', 'コントローラー']
  ] as const;

  function t(en: string, ja: string): string {
    return english ? en : ja;
  }

  function errorMessage(reason: unknown): string {
    return reason instanceof Error ? reason.message : 'Settings operation failed';
  }

  async function write(settings: SettingsWriteValues): Promise<void> {
    busy = true;
    error = null;
    try {
      await runtime.writeSettings(settings);
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = false;
    }
  }

  async function changeBoolean(event: Event, setting: BooleanSetting): Promise<void> {
    await write({ [setting]: (event.currentTarget as HTMLInputElement).checked });
  }

  async function changeSelect(event: Event, setting: SelectSetting): Promise<void> {
    const value = (event.currentTarget as HTMLSelectElement).value;
    if (setting === 'ui.stdout_destination') {
      await write({ 'ui.stdout_destination': value as 'output_1' | 'output_2' });
    } else if (setting === 'ui.widget_mode') {
      await write({
        'ui.widget_mode': value as NonNullable<SettingsWriteValues['ui.widget_mode']>
      });
    } else if (setting === 'ui.controller_position') {
      await write({ 'ui.controller_position': value as 'top' | 'bottom' });
    } else if (setting === 'ui.dialog_button_position') {
      await write({ 'ui.dialog_button_position': value as 'bottom' | 'top' | 'both' });
    } else if (setting === 'ui.desktop.close_behavior') {
      await write({
        'ui.desktop.close_behavior': value as 'ask' | 'shutdown' | 'keep_backend'
      });
    } else {
      await write({ language: value as 'ja' | 'en' });
    }
  }

  async function changeFps(event: Event): Promise<void> {
    const fps = Number((event.currentTarget as HTMLSelectElement).value);
    if (!Number.isSafeInteger(fps) || fps <= 0) {
      error = 'FPS must be a positive integer.';
      return;
    }
    await write({ 'ui.fps': fps });
  }

  async function changeSplit(event: Event): Promise<void> {
    const ratio = Number((event.currentTarget as HTMLInputElement).value);
    if (!Number.isSafeInteger(ratio) || ratio < 0 || ratio > 100) {
      error = 'Output split ratio must be between 0 and 100.';
      return;
    }
    await write({ 'ui.output_split_ratio': ratio });
  }
</script>

<div class="space-y-5">
  <div>
    <p class="text-xs font-semibold tracking-[0.2em] text-lime-300 uppercase">Other</p>
    <h2 class="mt-2 text-2xl font-semibold text-white">
      {t('Application settings', 'アプリケーション設定')}
    </h2>
    <p class="mt-2 text-sm text-slate-400">
      {t('Revision', 'リビジョン')} {view.settings?.revision ?? '—'} · {t('Profile', 'プロファイル')}
      {view.state?.active_profile ?? '—'}
    </p>
  </div>

  {#if error !== null}
    <div class="rounded-lg border border-red-400/20 bg-red-400/10 px-3 py-2 text-sm text-red-200" role="alert">{error}</div>
  {/if}

  <fieldset class="space-y-4 rounded-xl border border-white/10 bg-white/[0.025] p-4" disabled={busy}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-slate-400 uppercase">
      {t('Output and display', '出力と表示')}
    </legend>

    <label class="block">
      <span class="flex items-center justify-between text-xs font-medium text-slate-300">
        <span>{t('Output size split', '出力サイズ調整')}</span>
        <output for="output-split">{values?.['ui.output_split_ratio'] ?? 20}</output>
      </span>
      <input
        id="output-split"
        type="range"
        min="0"
        max="100"
        step="1"
        value={values?.['ui.output_split_ratio'] ?? 20}
        class="mt-2 w-full accent-cyan-300"
        onchange={(event) => void changeSplit(event)}
      />
      <span class="mt-1 flex justify-between text-[0.7rem] text-slate-500">
        <span>{t('Output #1: 10%', '出力 #1: 10%')}</span>
        <span>{t('Output #1: 90%', '出力 #1: 90%')}</span>
      </span>
    </label>

    <div class="grid gap-4 sm:grid-cols-2">
      <fieldset>
        <legend class="text-xs font-medium text-slate-300">{t('stdout destination', 'stdout 出力先')}</legend>
        <div class="mt-2 flex gap-2">
          {#each [['output_1', 'Output #1'], ['output_2', 'Output #2']] as destination (destination[0])}
            <label class="flex flex-1 items-center gap-2 rounded-lg bg-black/15 px-3 py-2 text-sm text-slate-200">
              <input
                type="radio"
                name="stdout-destination"
                value={destination[0]}
                checked={(values?.['ui.stdout_destination'] ?? 'output_1') === destination[0]}
                class="accent-cyan-300"
                onchange={(event) => void changeSelect(event, 'ui.stdout_destination')}
              />
              {destination[1]}
            </label>
          {/each}
        </div>
      </fieldset>

      <label>
        <span class="text-xs font-medium text-slate-300">FPS</span>
        <select
          aria-label="UI FPS"
          class="mt-2 w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white"
          value={values?.['ui.fps'] ?? 30}
          onchange={(event) => void changeFps(event)}
        >
          {#each values?.['ui.fps_options'] ?? [30] as fps (fps)}
            <option value={fps}>{fps} FPS</option>
          {/each}
        </select>
      </label>
    </div>

    <div class="grid gap-4 sm:grid-cols-2">
      <label>
        <span class="text-xs font-medium text-slate-300">{t('Widget mode', 'ウィジェットモード')}</span>
        <select
          class="mt-1 w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white"
          value={values?.['ui.widget_mode'] ?? 'all'}
          onchange={(event) => void changeSelect(event, 'ui.widget_mode')}
        >
          {#each widgetModes as mode (mode[0])}
            <option value={mode[0]}>{english ? mode[1] : mode[2]}</option>
          {/each}
        </select>
      </label>
      <label>
        <span class="text-xs font-medium text-slate-300">{t('Language', '表示言語')}</span>
        <select
          aria-label={t('Language', '表示言語')}
          class="mt-1 w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white"
          value={values?.language ?? 'ja'}
          onchange={(event) => void changeSelect(event, 'language')}
        >
          <option value="ja">日本語</option>
          <option value="en">English</option>
        </select>
      </label>
    </div>

    <button
      type="button"
      class="rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-200 hover:bg-white/10"
      onclick={() => runtime.clearOutputs()}
    >{t('Clear both outputs', '両方の出力をクリア')}</button>
  </fieldset>

  <fieldset class="space-y-4 rounded-xl border border-white/10 bg-white/[0.025] p-4" disabled={busy}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-slate-400 uppercase">
      {t('Layout', 'レイアウト')}
    </legend>
    <div class="grid gap-4 sm:grid-cols-2">
      <label>
        <span class="text-xs font-medium text-slate-300">{t('Controller position', 'コントローラーの位置')}</span>
        <select
          class="mt-1 w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white"
          value={values?.['ui.controller_position'] ?? 'top'}
          onchange={(event) => void changeSelect(event, 'ui.controller_position')}
        >
          <option value="top">TOP</option>
          <option value="bottom">BOTTOM</option>
        </select>
      </label>
      <label>
        <span class="text-xs font-medium text-slate-300">{t('Dialog button position', 'ダイアログボタンの位置')}</span>
        <select
          class="mt-1 w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white"
          value={values?.['ui.dialog_button_position'] ?? 'bottom'}
          onchange={(event) => void changeSelect(event, 'ui.dialog_button_position')}
        >
          <option value="top">TOP</option>
          <option value="bottom">BOTTOM</option>
          <option value="both">BOTH</option>
        </select>
      </label>
    </div>
  </fieldset>

  <fieldset class="space-y-4 rounded-xl border border-white/10 bg-white/[0.025] p-4" disabled={busy}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-slate-400 uppercase">
      {t('Runtime and desktop', 'ランタイムとデスクトップ')}
    </legend>
    <label class="flex items-start gap-3 rounded-lg bg-black/15 p-3 text-sm text-slate-200">
      <input
        type="checkbox"
        class="mt-0.5 size-4 accent-cyan-300"
        checked={values?.auto_reload_config ?? false}
        onchange={(event) => void changeBoolean(event, 'auto_reload_config')}
      />
      <span>
        <strong class="block font-medium">{t('Auto-reload dynamic configuration', '動的設定の自動リロード')}</strong>
        <span class="mt-1 block text-xs text-slate-400">init.py / init.lua</span>
      </span>
    </label>

    <label>
      <span class="text-xs font-medium text-slate-300">{t('When the final window closes', '最終ウィンドウを閉じる動作')}</span>
      <select
        class="mt-1 w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white"
        value={values?.['ui.desktop.close_behavior'] ?? 'ask'}
        onchange={(event) => void changeSelect(event, 'ui.desktop.close_behavior')}
      >
        <option value="ask">{t('Ask every time', '毎回確認')}</option>
        <option value="shutdown">{t('Shut down everything', 'すべて終了')}</option>
        <option value="keep_backend">{t('Keep backend running', 'バックエンドを継続')}</option>
      </select>
    </label>

    <label class="flex items-start gap-3 rounded-lg bg-black/15 p-3 text-sm text-slate-200">
      <input
        type="checkbox"
        class="mt-0.5 size-4 accent-cyan-300"
        checked={savedCompositing}
        onchange={(event) => void changeBoolean(event, 'ui.desktop.disable_compositing')}
      />
      <span class="min-w-0">
        <strong class="block font-medium">{t('Disable desktop compositing', 'デスクトップコンポジット無効化')}</strong>
        <span class="mt-1 block text-xs text-slate-400">
          {desktopMode
            ? t('Applied when the desktop app next starts.', 'デスクトップアプリの次回起動時に反映されます。')
            : t('Stored in Web mode but has no effect.', 'Web モードでは値だけを保持し、効果はありません。')}
        </span>
        <span class="mt-2 block font-mono text-[0.7rem] text-slate-500">
          {t('Current', '現在')}: {values?.['ui.desktop.disable_compositing'] ? 'true' : 'false'} ·
          {t('Saved', '保存')}: {savedCompositing ? 'true' : 'false'}
        </span>
        {#if compositingRestart}
          <span class="mt-2 inline-flex rounded-full bg-amber-300/10 px-2 py-1 text-[0.7rem] font-semibold text-amber-200">
            {t('Restart required', '再起動後に反映')}
          </span>
        {/if}
      </span>
    </label>
  </fieldset>

  {#if (view.settings?.restart_required.length ?? 0) > 0 || Object.keys(view.settings?.apply_failures ?? {}).length > 0}
    <section class="rounded-xl border border-amber-300/20 bg-amber-300/[0.06] p-4" aria-labelledby="settings-status">
      <h3 id="settings-status" class="text-sm font-semibold text-amber-100">{t('Settings status', '設定ステータス')}</h3>
      {#if (view.settings?.restart_required.length ?? 0) > 0}
        <p class="mt-2 text-xs text-amber-100/80">
          {t('Restart required:', '再起動が必要:')}
          <code>{view.settings?.restart_required.join(', ')}</code>
        </p>
      {/if}
      {#each Object.entries(view.settings?.apply_failures ?? {}) as [setting, message] (setting)}
        <p class="mt-2 text-xs text-red-200"><code>{setting}</code>: {message}</p>
      {/each}
    </section>
  {/if}
</div>
