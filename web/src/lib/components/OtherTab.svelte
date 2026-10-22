<script lang="ts">
  import { browser } from '$app/environment';

  import type { SettingsWriteValues } from '../api';
  import DiagnosticsDialog from './DiagnosticsDialog.svelte';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  type SelectSetting =
    | 'ui.stdout_destination'
    | 'ui.widget_mode'
    | 'ui.right_panel_position'
    | 'ui.controller_layout'
    | 'ui.controller_position'
    | 'ui.controller_body_color'
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
    } else if (setting === 'ui.controller_layout') {
      await write({
        'ui.controller_layout': value as NonNullable<
          SettingsWriteValues['ui.controller_layout']
        >
      });
    } else if (setting === 'ui.right_panel_position') {
      await write({
        'ui.right_panel_position': value as NonNullable<
          SettingsWriteValues['ui.right_panel_position']
        >
      });
    } else if (setting === 'ui.controller_position') {
      await write({ 'ui.controller_position': value as 'top' | 'bottom' });
    } else if (setting === 'ui.controller_body_color') {
      await write({
        'ui.controller_body_color': value as NonNullable<
          SettingsWriteValues['ui.controller_body_color']
        >
      });
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

  function exportColorPreset(): void {
    const body = JSON.stringify(
      {
        ui: {
          controller_body_color: values?.['ui.controller_body_color'] ?? 'classic'
        }
      },
      null,
      2
    );
    const blob = new Blob([body], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = 'pokecon-controller-color.json';
    anchor.click();
    URL.revokeObjectURL(url);
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
  interface PackageRow {
    name: string;
    version: string;
    extras: string;
  }

  let packageRows = $state<PackageRow[]>([]);
  let packageDirty = $state(false);
  let packageError = $state<string | null>(null);
  let packageMessage = $state<string | null>(null);

  const savedPackages = $derived(
    (values?.['python.script.packages.list'] ?? []) as {
      name: string;
      version?: string;
      extras?: string[];
    }[]
  );

  $effect(() => {
    if (!packageDirty) {
      packageRows = savedPackages.map((entry) => ({
        name: entry.name,
        version: entry.version ?? '',
        extras: (entry.extras ?? []).join(', ')
      }));
    }
  });

  function addPackage(): void {
    packageRows = [...packageRows, { name: '', version: '', extras: '' }];
    packageDirty = true;
    packageError = null;
  }

  function removePackage(index: number): void {
    packageRows = packageRows.filter((_row, rowIndex) => rowIndex !== index);
    packageDirty = true;
    packageError = null;
  }

  function updatePackage(index: number, field: keyof PackageRow, value: string): void {
    packageRows = packageRows.map((row, rowIndex) =>
      rowIndex === index ? { ...row, [field]: value } : row
    );
    packageDirty = true;
    packageError = null;
  }

  async function savePackages(): Promise<void> {
    const names: string[] = [];
    const output: { name: string; version?: string; extras?: string[] }[] = [];
    for (const row of packageRows) {
      const name = row.name.trim();
      if (name.length === 0) {
        packageError = t('Every library needs a name.', 'ライブラリ名を入力してください。');
        return;
      }
      if (names.includes(name)) {
        packageError = t('Library names must be unique.', 'ライブラリ名が重複しています。');
        return;
      }
      names.push(name);
      const extras = row.extras
        .split(',')
        .map((extra) => extra.trim())
        .filter((extra) => extra.length > 0);
      const entry: { name: string; version?: string; extras?: string[] } = { name };
      const version = row.version.trim();
      if (version.length > 0) {
        entry.version = version;
      }
      if (extras.length > 0) {
        entry.extras = extras;
      }
      output.push(entry);
    }
    busy = true;
    packageError = null;
    packageMessage = null;
    try {
      await runtime.writeSettings({
        'python.script.packages.list': output
      });
      packageDirty = false;
      packageMessage = t(
        'Saved. Libraries synchronize at the next worker generation.',
        '保存しました。次回のワーカー生成時に同期されます。'
      );
    } catch (reason: unknown) {
      packageError = errorMessage(reason);
    } finally {
      busy = false;
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
        <span class="text-xs font-medium text-subtext1">{t('Controller layout', 'コントローラーレイアウト')}</span>
        <select
          aria-label={t('Controller layout', 'コントローラーレイアウト')}
          class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text"
          value={values?.['ui.controller_layout'] ?? 'standard'}
          onchange={(event) => void changeSelect(event, 'ui.controller_layout')}
        >
          <option value="standard">{t('Standard', '標準')}</option>
          <option value="wide_center">{t('Wide center', '中央広幅')}</option>
          <option value="left_heavy">{t('Left heavy', '左広幅')}</option>
          <option value="right_heavy">{t('Right heavy', '右広幅')}</option>
          <option value="stacked">{t('Stacked', '縦積み')}</option>
        </select>
      </label>
      <label>
        <span class="text-xs font-medium text-subtext1">{t('Right panel position', '右パネル位置')}</span>
        <select
          aria-label={t('Right panel position', '右パネル位置')}
          class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text"
          value={values?.['ui.right_panel_position'] ?? 'right'}
          onchange={(event) => void changeSelect(event, 'ui.right_panel_position')}
        >
          <option value="right">{t('Right', '右')}</option>
          <option value="bottom">{t('Bottom', '下')}</option>
          <option value="hidden">{t('Hidden', '非表示')}</option>
        </select>
      </label>
      <label>
        <span class="text-xs font-medium text-subtext1">{t('Controller body color', 'コントローラー本体色')}</span>
        <div class="mt-1 flex gap-2">
          <select
            aria-label={t('Controller body color', 'コントローラー本体色')}
            class="min-w-0 flex-1 rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text"
            value={values?.['ui.controller_body_color'] ?? 'classic'}
            onchange={(event) => void changeSelect(event, 'ui.controller_body_color')}
          >
            {#each [['classic', 'Classic'], ['black', 'Black'], ['white', 'White'], ['red', 'Red'], ['blue', 'Blue'], ['green', 'Green'], ['purple', 'Purple']] as color (color[0])}
              <option value={color[0]}>{color[1]}</option>
            {/each}
          </select>
          <button type="button" class="rounded-lg bg-text/5 px-3 py-2 text-xs text-subtext1 hover:bg-text/10" onclick={exportColorPreset}>{t('Export', '書出し')}</button>
        </div>
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

  <section class="rounded-lg border border-text/10 bg-crust/20 p-4" aria-labelledby="python-libraries">
    <h3 id="python-libraries" class="text-sm font-semibold text-text">{t('Python libraries', 'Pythonライブラリ')}</h3>
    <p class="mt-1 text-[11px] text-subtext0">{t('Declared packages synchronize into the script worker environment at the next worker generation.', '宣言したパッケージは次回のワーカー生成時にスクリプト実行環境へ同期されます。')}</p>
    <div class="mt-3 space-y-2">
      {#each packageRows as row, index (index)}
        <div class="grid gap-2 sm:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_minmax(0,1.5fr)_auto]">
          <label class="text-xs text-subtext1">
            {t('Name', '名前')}
            <input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label={`Library name ${String(index + 1)}`} value={row.name} oninput={(event) => updatePackage(index, 'name', event.currentTarget.value)} />
          </label>
          <label class="text-xs text-subtext1">
            {t('Version (optional)', 'バージョン（任意）')}
            <input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label={`Library version ${String(index + 1)}`} value={row.version} oninput={(event) => updatePackage(index, 'version', event.currentTarget.value)} />
          </label>
          <label class="text-xs text-subtext1">
            {t('Extras (comma separated)', 'extras（カンマ区切り）')}
            <input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label={`Library extras ${String(index + 1)}`} value={row.extras} oninput={(event) => updatePackage(index, 'extras', event.currentTarget.value)} />
          </label>
          <button type="button" class="mt-5 rounded-md bg-red/10 px-2 py-1 text-[11px] text-red" aria-label={`Remove library ${String(index + 1)}`} onclick={() => removePackage(index)}>{t('Remove', '削除')}</button>
        </div>
      {/each}
      {#if packageRows.length === 0}
        <p class="text-xs text-subtext0">{t('No libraries are declared.', 'ライブラリは登録されていません。')}</p>
      {/if}
    </div>
    <div class="mt-3 flex flex-wrap items-center gap-2">
      <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1" onclick={addPackage}>{t('Add library', 'ライブラリを追加')}</button>
      <button type="button" class="rounded-md bg-green/15 px-2 py-1 text-[11px] text-green disabled:opacity-40" disabled={busy || !packageDirty} onclick={() => void savePackages()}>{t('Save libraries', 'ライブラリを保存')}</button>
      {#if packageMessage !== null}<span class="text-[11px] text-green" role="status">{packageMessage}</span>{/if}
    </div>
    {#if packageError !== null}
      <p class="mt-2 text-[11px] text-red" role="alert">{packageError}</p>
    {/if}
  </section>

  <DiagnosticsDialog {runtime} {view} />

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
