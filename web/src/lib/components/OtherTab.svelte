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
  let copied = $state<string | null>(null);
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
  const savedWebDirectory = $derived(
    view.settings?.pending_restart_values['server.web_dir'] ??
      values?.['server.web_dir'] ??
      ''
  );
  const savedPort = $derived(
    view.settings?.pending_restart_values['server.port'] ?? values?.['server.port'] ?? 8020
  );
  const savedBindAddress = $derived(
    view.settings?.pending_restart_values['server.bind_address'] ??
      values?.['server.bind_address'] ??
      '127.0.0.1'
  );
  const lanExposed = $derived(!isLoopback(savedBindAddress));

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
    const value = (event.currentTarget as HTMLInputElement | HTMLSelectElement).value;
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

  async function changeServerText(
    event: Event,
    setting: 'server.web_dir' | 'server.bind_address' | 'stun_server'
  ): Promise<void> {
    const value = (event.currentTarget as HTMLInputElement).value;
    await write({ [setting]: value });
  }

  async function changePort(event: Event): Promise<void> {
    const port = Number((event.currentTarget as HTMLInputElement).value);
    if (!Number.isSafeInteger(port) || port < 1 || port > 65_535) {
      error = 'Server port must be an integer between 1 and 65535.';
      return;
    }
    await write({ 'server.port': port });
  }

  function isLoopback(address: string): boolean {
    if (address === '::1') return true;
    const first = Number(address.split('.')[0]);
    return Number.isSafeInteger(first) && first === 127;
  }

  async function copyValue(label: string, value: string): Promise<void> {
    error = null;
    copied = null;
    try {
      await navigator.clipboard.writeText(value);
      copied = label;
    } catch (reason: unknown) {
      error = errorMessage(reason);
    }
  }
</script>

<div class="space-y-5">
  <div>
    <p class="text-xs font-semibold tracking-[0.12em] text-blue uppercase">Other</p>
    <h2 class="mt-2 text-2xl font-semibold text-text">
      {t('Application settings', 'アプリケーション設定')}
    </h2>
    <p class="mt-2 text-sm text-subtext1">
      {t('Revision', 'リビジョン')} {view.settings?.revision ?? '—'} · {t('Profile', 'プロファイル')}
      {view.state?.active_profile ?? '—'}
    </p>
  </div>

  {#if error !== null}
    <div class="rounded-lg border border-red/20 bg-red/10 px-3 py-2 text-sm text-red" role="alert">{error}</div>
  {/if}

  <fieldset class="space-y-4 rounded-lg border border-text/10 bg-text/[0.025] p-4" disabled={busy}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-subtext1 uppercase">
      {t('Output and display', '出力と表示')}
    </legend>

    <label class="block">
      <span class="flex items-center justify-between text-xs font-medium text-subtext1">
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
        class="mt-2 w-full accent-blue"
        onchange={(event) => void changeSplit(event)}
      />
      <span class="mt-1 flex justify-between text-[0.7rem] text-subtext0">
        <span>{t('Output #1: 10%', '出力 #1: 10%')}</span>
        <span>{t('Output #1: 90%', '出力 #1: 90%')}</span>
      </span>
    </label>

    <div class="grid gap-4 sm:grid-cols-2">
      <fieldset>
        <legend class="text-xs font-medium text-subtext1">{t('stdout destination', 'stdout 出力先')}</legend>
        <div class="mt-2 flex gap-2">
          {#each [['output_1', 'Output #1'], ['output_2', 'Output #2']] as destination (destination[0])}
            <label class="flex flex-1 items-center gap-2 rounded-lg bg-crust/15 px-3 py-2 text-sm text-text">
              <input
                type="radio"
                name="stdout-destination"
                value={destination[0]}
                checked={(values?.['ui.stdout_destination'] ?? 'output_1') === destination[0]}
                class="accent-blue"
                onchange={(event) => void changeSelect(event, 'ui.stdout_destination')}
              />
              {destination[1]}
            </label>
          {/each}
        </div>
      </fieldset>

      <label>
        <span class="text-xs font-medium text-subtext1">FPS</span>
        <select
          aria-label="UI FPS"
          class="mt-2 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text"
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
        <span class="text-xs font-medium text-subtext1">{t('Widget mode', 'ウィジェットモード')}</span>
        <select
          class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text"
          value={values?.['ui.widget_mode'] ?? 'all'}
          onchange={(event) => void changeSelect(event, 'ui.widget_mode')}
        >
          {#each widgetModes as mode (mode[0])}
            <option value={mode[0]}>{english ? mode[1] : mode[2]}</option>
          {/each}
        </select>
      </label>
      <label>
        <span class="text-xs font-medium text-subtext1">{t('Language', '表示言語')}</span>
        <select
          aria-label={t('Language', '表示言語')}
          class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text"
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
      class="rounded-lg border border-text/10 bg-text/5 px-3 py-2 text-sm text-text hover:bg-text/10"
      onclick={() => runtime.clearOutputs()}
    >{t('Clear both outputs', '両方の出力をクリア')}</button>
  </fieldset>

  <fieldset class="space-y-4 rounded-lg border border-text/10 bg-text/[0.025] p-4" disabled={busy}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-subtext1 uppercase">
      {t('Layout', 'レイアウト')}
    </legend>
    <div class="grid gap-4 sm:grid-cols-2">
      <fieldset>
        <legend class="text-xs font-medium text-subtext1">{t('Controller position', 'コントローラーの位置')}</legend>
        <div class="mt-2 flex gap-2">
          {#each [['top', 'TOP'], ['bottom', 'BOTTOM']] as position (position[0])}
            <label class="flex flex-1 items-center gap-2 rounded-lg bg-crust/15 px-3 py-2 text-sm text-text">
              <input
                type="radio"
                name="controller-position"
                value={position[0]}
                checked={(values?.['ui.controller_position'] ?? 'bottom') === position[0]}
                class="accent-blue"
                onchange={(event) => void changeSelect(event, 'ui.controller_position')}
              />
              {position[1]}
            </label>
          {/each}
        </div>
      </fieldset>
      <fieldset>
        <legend class="text-xs font-medium text-subtext1">{t('Dialog button position', 'ダイアログボタンの位置')}</legend>
        <div class="mt-2 flex gap-2">
          {#each [['top', 'TOP'], ['bottom', 'BOTTOM'], ['both', 'BOTH']] as position (position[0])}
            <label class="flex flex-1 items-center gap-2 rounded-lg bg-crust/15 px-3 py-2 text-sm text-text">
              <input
                type="radio"
                name="dialog-button-position"
                value={position[0]}
                checked={(values?.['ui.dialog_button_position'] ?? 'bottom') === position[0]}
                class="accent-blue"
                onchange={(event) => void changeSelect(event, 'ui.dialog_button_position')}
              />
              {position[1]}
            </label>
          {/each}
        </div>
      </fieldset>
    </div>
  </fieldset>

  <fieldset class="space-y-4 rounded-lg border border-text/10 bg-text/[0.025] p-4" disabled={busy}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-subtext1 uppercase">
      {t('Runtime and desktop', 'ランタイムとデスクトップ')}
    </legend>
    <label class="flex items-start gap-3 rounded-lg bg-crust/15 p-3 text-sm text-text">
      <input
        type="checkbox"
        class="mt-0.5 size-4 accent-blue"
        checked={values?.auto_reload_config ?? false}
        onchange={(event) => void changeBoolean(event, 'auto_reload_config')}
      />
      <span>
        <strong class="block font-medium">{t('Auto-reload dynamic configuration', '動的設定の自動リロード')}</strong>
        <span class="mt-1 block text-xs text-subtext1">init.py / init.lua</span>
      </span>
    </label>

    <label>
      <span class="text-xs font-medium text-subtext1">{t('When the final window closes', '最終ウィンドウを閉じる動作')}</span>
      <select
        class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text"
        value={values?.['ui.desktop.close_behavior'] ?? 'ask'}
        onchange={(event) => void changeSelect(event, 'ui.desktop.close_behavior')}
      >
        <option value="ask">{t('Ask every time', '毎回確認')}</option>
        <option value="shutdown">{t('Shut down everything', 'すべて終了')}</option>
        <option value="keep_backend">{t('Keep backend running', 'バックエンドを継続')}</option>
      </select>
    </label>

    <label class="flex items-start gap-3 rounded-lg bg-crust/15 p-3 text-sm text-text">
      <input
        type="checkbox"
        class="mt-0.5 size-4 accent-blue"
        checked={savedCompositing}
        onchange={(event) => void changeBoolean(event, 'ui.desktop.disable_compositing')}
      />
      <span class="min-w-0">
        <strong class="block font-medium">{t('Disable desktop compositing', 'デスクトップコンポジット無効化')}</strong>
        <span class="mt-1 block text-xs text-subtext1">
          {desktopMode
            ? t('Applied when the desktop app next starts.', 'デスクトップアプリの次回起動時に反映されます。')
            : t('Stored in Web mode but has no effect.', 'Web モードでは値だけを保持し、効果はありません。')}
        </span>
        <span class="mt-2 block font-mono text-[0.7rem] text-subtext0">
          {t('Current', '現在')}: {values?.['ui.desktop.disable_compositing'] ? 'true' : 'false'} ·
          {t('Saved', '保存')}: {savedCompositing ? 'true' : 'false'}
        </span>
        {#if compositingRestart}
          <span class="mt-2 inline-flex rounded-full bg-yellow/10 px-2 py-1 text-[0.7rem] font-semibold text-yellow">
            {t('Restart required', '再起動後に反映')}
          </span>
        {/if}
      </span>
    </label>
  </fieldset>

  <fieldset class="space-y-4 rounded-lg border border-text/10 bg-text/[0.025] p-4" disabled={busy}>
    <legend class="px-2 text-xs font-semibold tracking-[0.14em] text-subtext1 uppercase">
      {t('Server and network', 'サーバーとネットワーク')}
    </legend>
    <p class="text-xs text-subtext1">
      {t(
        'Server root, port, and bind address are saved now and applied after restart.',
        'Web UI ディレクトリ、ポート、待受アドレスは保存後、再起動時に反映されます。'
      )}
    </p>

    <label class="block">
      <span class="text-xs font-medium text-subtext1">{t('Saved Web UI directory', '保存する Web UI ディレクトリ')}</span>
      <input
        type="text"
        value={savedWebDirectory}
        class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 font-mono text-sm text-text outline-none focus:border-blue/60"
        onchange={(event) => void changeServerText(event, 'server.web_dir')}
      />
      <span class="mt-2 flex flex-wrap items-center gap-2 text-[0.7rem] text-subtext0">
        <span class="min-w-0 break-all">{t('Current', '現在')}: {values?.['server.web_dir'] ?? '—'}</span>
        <button
          type="button"
          class="rounded bg-text/5 px-2 py-1 text-subtext1"
          onclick={() => void copyValue('server.web_dir', values?.['server.web_dir'] ?? '')}
        >{copied === 'server.web_dir' ? t('Copied', 'コピー済み') : t('Copy', 'コピー')}</button>
      </span>
    </label>

    <div class="grid gap-4 sm:grid-cols-2">
      <label>
        <span class="text-xs font-medium text-subtext1">{t('Saved server port', '保存するサーバーポート')}</span>
        <input
          type="number"
          min="1"
          max="65535"
          step="1"
          value={savedPort}
          class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text outline-none focus:border-blue/60"
          onchange={(event) => void changePort(event)}
        />
        <span class="mt-1 block text-[0.7rem] text-subtext0">
          {t('Current', '現在')}: {values?.['server.port'] ?? '—'}
        </span>
      </label>
      <label>
        <span class="text-xs font-medium text-subtext1">{t('Saved bind address', '保存するバインドアドレス')}</span>
        <input
          type="text"
          inputmode="decimal"
          value={savedBindAddress}
          class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 font-mono text-sm text-text outline-none focus:border-blue/60"
          onchange={(event) => void changeServerText(event, 'server.bind_address')}
        />
        <span class="mt-1 block text-[0.7rem] text-subtext0">
          {t('Current', '現在')}: {values?.['server.bind_address'] ?? '—'}
        </span>
      </label>
    </div>

    {#if lanExposed}
      <div class="rounded-lg border border-red/30 bg-red/10 px-3 py-2 text-xs text-red" role="alert">
        {t(
          'This address exposes every REST and WebSocket operation to trusted LAN clients without authentication, including dynamic Python/Lua loading.',
          'このアドレスは、動的 Python/Lua 読み込みを含む全 REST／WebSocket 操作を、認証なしで信頼済み LAN クライアントへ公開します。'
        )}
      </div>
    {/if}

    <label class="block">
      <span class="text-xs font-medium text-subtext1">{t('WebRTC STUN URI', 'WebRTC STUN URI')}</span>
      <input
        type="text"
        placeholder="stun:stun.example.com:3478"
        value={values?.stun_server ?? ''}
        class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 font-mono text-sm text-text outline-none focus:border-blue/60"
        onchange={(event) => void changeServerText(event, 'stun_server')}
      />
      <span class="mt-1 block text-[0.7rem] text-subtext0">
        {t(
          'Empty disables STUN. Changes affect future WebRTC connections.',
          '空文字で無効。変更は以後の WebRTC 接続から使用されます。'
        )}
      </span>
    </label>
  </fieldset>

  {#if (view.settings?.restart_required.length ?? 0) > 0 || Object.keys(view.settings?.apply_failures ?? {}).length > 0}
    <section class="rounded-lg border border-yellow/20 bg-yellow/[0.06] p-4" aria-labelledby="settings-status">
      <h3 id="settings-status" class="text-sm font-semibold text-yellow">{t('Settings status', '設定ステータス')}</h3>
      {#if (view.settings?.restart_required.length ?? 0) > 0}
        <p class="mt-2 text-xs text-yellow/80">
          {t('Restart required:', '再起動が必要:')}
          <code>{view.settings?.restart_required.join(', ')}</code>
        </p>
      {/if}
      {#each Object.entries(view.settings?.apply_failures ?? {}) as [setting, message] (setting)}
        <p class="mt-2 text-xs text-red"><code>{setting}</code>: {message}</p>
      {/each}
    </section>
  {/if}
</div>
