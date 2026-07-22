<script lang="ts">
  import { browser } from '$app/environment';

  import type {
    DownloadResult,
    DynamicConfigControlRequest,
    DynamicConfigResult,
    GenerateLauncherRequest,
    GenerateLauncherResult,
    UpdateCheckResult
  } from '../actions';
  import {
    chooseNativeSavePath,
    isDesktopShell,
    openNativeConfigDirectory
  } from '../desktop';
  import { triggerDownload } from '../download';
  import openapi from '../generated/openapi.json';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  type DownloadLauncherRequest = GenerateLauncherRequest & {
    destination: { kind: 'download'; filename: string | null };
  };
  type PathLauncherRequest = GenerateLauncherRequest & {
    destination: { kind: 'path'; path: string };
  };
  type BusyAction = 'dynamic' | 'launcher' | 'profile' | 'update';

  interface MenuActions {
    checkUpdate(): Promise<UpdateCheckResult>;
    controlDynamicConfig(request: DynamicConfigControlRequest): Promise<DynamicConfigResult>;
    downloadLauncher(request: DownloadLauncherRequest): Promise<DownloadResult>;
    generateLauncher(request: PathLauncherRequest): Promise<GenerateLauncherResult>;
  }

  interface ClipboardWriter {
    writeText(value: string): Promise<void>;
  }

  interface Props {
    actions: MenuActions;
    confirmOverwrite?: (message: string) => boolean;
    onresetview?: () => void;
    runtime: ApplicationRuntime;
    view: RuntimeView;
    windows?: boolean;
  }

  const detectedWindows =
    browser && /Windows|Win32|Win64/iu.test(`${navigator.platform} ${navigator.userAgent}`);
  const desktopMode = isDesktopShell();
  let {
    actions,
    confirmOverwrite = (message: string) => browser && window.confirm(message),
    onresetview = () => undefined,
    runtime,
    view,
    windows = detectedWindows
  }: Props = $props();
  let busy = $state<BusyAction | null>(null);
  let copyCurrent = $state(false);
  let copied = $state(false);
  let dynamicResult = $state<DynamicConfigResult | null>(null);
  let error = $state<string | null>(null);
  let launcherProfile = $state('');
  let lastActiveProfile = $state('');
  let notice = $state<string | null>(null);
  let selectedProfile = $state('');
  let updateResult = $state<UpdateCheckResult | null>(null);

  const values = $derived(view.settings?.values);
  const english = $derived(values?.language === 'en');
  const activeProfile = $derived(
    view.state?.active_profile ?? values?.active_profile ?? 'default'
  );
  const availableProfiles = $derived(
    view.state?.available_profiles.length === 0 || view.state === null
      ? [activeProfile]
      : view.state.available_profiles
  );
  const configDirectory = $derived(
    dynamicResult === null ? null : parentDirectory(dynamicResult.display_path)
  );

  $effect(() => {
    if (activeProfile !== lastActiveProfile) {
      selectedProfile = activeProfile;
      launcherProfile = activeProfile;
      lastActiveProfile = activeProfile;
    }
  });

  function t(en: string, ja: string): string {
    return english ? en : ja;
  }

  function errorMessage(reason: unknown): string {
    return reason instanceof Error ? reason.message : 'Menu operation failed';
  }

  function isClipboardWriter(value: unknown): value is ClipboardWriter {
    return (
      typeof value === 'object' &&
      value !== null &&
      typeof Reflect.get(value, 'writeText') === 'function'
    );
  }

  function begin(action: BusyAction): void {
    busy = action;
    error = null;
    notice = null;
  }

  function parentDirectory(path: string): string {
    const separator = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
    if (separator < 0) return '.';
    if (separator === 0) return path.slice(0, 1);
    if (separator === 2 && /^[A-Za-z]:/u.test(path)) return path.slice(0, 3);
    return path.slice(0, separator);
  }

  async function switchProfile(): Promise<void> {
    if (selectedProfile === activeProfile) return;
    begin('profile');
    try {
      await runtime.writeSettings({ active_profile: selectedProfile });
      notice = t(
        `Profile switch requested: ${selectedProfile}`,
        `プロファイル切替を要求しました: ${selectedProfile}`
      );
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function loadDynamicFile(event: Event): Promise<void> {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (file === undefined) return;
    const lowerName = file.name.toLowerCase();
    const language = lowerName.endsWith('.py')
      ? 'python'
      : lowerName.endsWith('.lua')
        ? 'lua'
        : null;
    if (language === null) {
      error = t('Select a .py or .lua file.', '.py または .lua ファイルを選択してください。');
      return;
    }
    if (
      !confirmOverwrite(
        t(
          `Replace the server's canonical init.${language === 'python' ? 'py' : 'lua'} and load ${file.name}?`,
          `サーバーの正準 init.${language === 'python' ? 'py' : 'lua'} を置換して ${file.name} を読み込みますか？`
        )
      )
    ) {
      notice = t('Dynamic configuration load cancelled.', '動的設定の読み込みをキャンセルしました。');
      return;
    }

    begin('dynamic');
    try {
      const result = await actions.controlDynamicConfig({
        action: 'load_content',
        content: await file.text(),
        language
      });
      dynamicResult = result;
      notice = result.loaded
        ? t(`Loaded ${result.display_path}.`, `${result.display_path} を読み込みました。`)
        : t('The new file was not activated.', '新しいファイルは有効化されませんでした。');
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function reloadDynamicConfig(): Promise<void> {
    begin('dynamic');
    try {
      const result = await actions.controlDynamicConfig({ action: 'reload' });
      dynamicResult = result;
      notice = result.loaded
        ? t(`Reloaded ${result.display_path}.`, `${result.display_path} を再読み込みしました。`)
        : t('Dynamic configuration reload failed.', '動的設定の再読み込みに失敗しました。');
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function copyConfigDirectory(): Promise<void> {
    if (configDirectory === null) return;
    const clipboard: unknown = Reflect.get(navigator, 'clipboard');
    if (!isClipboardWriter(clipboard)) {
      error = t('Clipboard access is unavailable.', 'クリップボードを利用できません。');
      return;
    }
    try {
      await clipboard.writeText(configDirectory);
      copied = true;
      setTimeout(() => (copied = false), 1_500);
    } catch (reason: unknown) {
      error = errorMessage(reason);
    }
  }

  async function accessConfigDirectory(): Promise<void> {
    if (!desktopMode) {
      await copyConfigDirectory();
      return;
    }
    begin('dynamic');
    try {
      await openNativeConfigDirectory();
      notice = t('Opened the config directory.', '設定ディレクトリを開きました。');
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function generateLauncher(): Promise<void> {
    const profile = launcherProfile.trim();
    if (profile.length === 0) {
      error = t('Enter a profile name.', 'プロファイル名を入力してください。');
      return;
    }
    begin('launcher');
    try {
      if (desktopMode) {
        const path = await chooseNativeSavePath(`${profile}.bat`, 'bat');
        if (path === null) {
          notice = t('Launcher generation cancelled.', 'ランチャー作成をキャンセルしました。');
          return;
        }
        const result = await actions.generateLauncher({
          copy_current: copyCurrent,
          destination: { kind: 'path', path },
          profile
        });
        notice = result.launcher_created
          ? t(`Launcher created for ${profile}.`, `${profile} のランチャーを作成しました。`)
          : t(`Launcher already exists for ${profile}.`, `${profile} のランチャーは既に存在します。`);
        return;
      }
      const result = await actions.downloadLauncher({
        copy_current: copyCurrent,
        destination: { filename: `${profile}.bat`, kind: 'download' },
        profile
      });
      triggerDownload(result, `${profile}.bat`);
      notice = t(`Launcher prepared for ${profile}.`, `${profile} のランチャーを作成しました。`);
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function checkUpdate(): Promise<void> {
    begin('update');
    try {
      updateResult = await actions.checkUpdate();
      notice = updateResult.update_available
        ? t(
            `Version ${updateResult.latest_version} is available.`,
            `バージョン ${updateResult.latest_version} を利用できます。`
          )
        : t('PokeCon is up to date.', 'PokeCon は最新です。');
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  function resetView(): void {
    onresetview();
    notice = t('View state reset.', '画面表示をリセットしました。');
  }

  function openRelease(): void {
    if (updateResult === null) return;
    try {
      const url = new URL(updateResult.release_url);
      if (url.protocol !== 'https:' && url.protocol !== 'http:') {
        throw new Error('release URL must use HTTP or HTTPS');
      }
      window.open(url, '_blank', 'noopener,noreferrer');
    } catch (reason: unknown) {
      error = errorMessage(reason);
    }
  }
</script>

<div class="relative z-30 flex flex-wrap items-center justify-end gap-2">
  <details class="group relative">
    <summary class="cursor-pointer list-none rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-xs font-medium text-slate-200 hover:bg-white/10">
      {t('Menu', 'メニュー')}
    </summary>
    <div class="absolute right-0 mt-2 w-[min(92vw,28rem)] space-y-4 rounded-xl border border-white/10 bg-ink-900 p-4 shadow-2xl shadow-black/60">
      {#if error !== null}
        <div class="rounded-lg border border-red-400/20 bg-red-400/10 px-3 py-2 text-xs text-red-200" role="alert">{error}</div>
      {:else if notice !== null}
        <div class="rounded-lg border border-lime-300/20 bg-lime-300/10 px-3 py-2 text-xs text-lime-200" role="status">{notice}</div>
      {/if}

      <section aria-labelledby="profile-menu-heading">
        <h2 id="profile-menu-heading" class="text-xs font-semibold tracking-[0.14em] text-cyan-200 uppercase">
          {t('Profile', 'プロファイル')}
        </h2>
        <div class="mt-2 flex gap-2">
          <label class="min-w-0 flex-1">
            <span class="sr-only">{t('Selected profile', '選択プロファイル')}</span>
            <select
              class="w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white"
              bind:value={selectedProfile}
              disabled={busy !== null || view.state?.pending_profile !== null}
            >
              {#each availableProfiles as profile (profile)}
                <option value={profile}>{profile}</option>
              {/each}
            </select>
          </label>
          <button
            type="button"
            class="rounded-lg bg-cyan-300/15 px-3 py-2 text-sm font-medium text-cyan-200 disabled:opacity-40"
            disabled={busy !== null || selectedProfile === activeProfile || view.state?.pending_profile !== null}
            onclick={() => void switchProfile()}
          >{busy === 'profile' ? t('Switching…', '切替中…') : t('Switch', '切替')}</button>
        </div>
        {#if view.state?.pending_profile !== null && view.state?.pending_profile !== undefined}
          <p class="mt-2 text-xs text-amber-200" aria-live="polite">
            {t('Pending', '切替先')}: {view.state.pending_profile}
          </p>
        {/if}
      </section>

      <section class="border-t border-white/10 pt-4" aria-labelledby="dynamic-menu-heading">
        <h2 id="dynamic-menu-heading" class="text-xs font-semibold tracking-[0.14em] text-cyan-200 uppercase">
          {t('Dynamic configuration', '動的設定')}
        </h2>
        <div class="mt-2 flex flex-wrap gap-2">
          <label class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-xs text-slate-200 hover:bg-white/10">
            {busy === 'dynamic' ? t('Loading…', '読込中…') : t('Load .py / .lua', '.py / .lua を読み込む')}
            <input
              type="file"
              accept=".py,.lua,text/x-python,text/x-lua"
              class="sr-only"
              disabled={busy !== null}
              onchange={(event) => void loadDynamicFile(event)}
            />
          </label>
          <button
            type="button"
            class="rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-xs text-slate-200 hover:bg-white/10 disabled:opacity-40"
            disabled={busy !== null}
            onclick={() => void reloadDynamicConfig()}
          >{t('Reload', '再読み込み')}</button>
        </div>
        <div class="mt-3 rounded-lg bg-black/15 p-3 text-xs text-slate-400">
          <p class="font-medium text-slate-300">{desktopMode ? t('Config directory', '設定ディレクトリ') : t('Config directory (Web mode)', '設定ディレクトリ（Web モード）')}</p>
          {#if desktopMode}
            <button
              type="button"
              class="mt-2 rounded bg-white/5 px-2 py-1 text-slate-200"
              disabled={busy !== null}
              onclick={() => void accessConfigDirectory()}
            >{t('Open directory', 'ディレクトリを開く')}</button>
          {:else if configDirectory === null}
            <p class="mt-1">{t('Load or reload a config to resolve its display path.', '読み込みまたは再読み込み後に表示パスを確認できます。')}</p>
          {:else}
            <p class="mt-1 break-all font-mono">{configDirectory}</p>
            <button
              type="button"
              class="mt-2 rounded bg-white/5 px-2 py-1 text-slate-200"
              onclick={() => void accessConfigDirectory()}
            >{copied ? t('Copied', 'コピー済み') : t('Copy path', 'パスをコピー')}</button>
          {/if}
        </div>
      </section>

      <section class="border-t border-white/10 pt-4" aria-labelledby="launcher-menu-heading">
        <h2 id="launcher-menu-heading" class="text-xs font-semibold tracking-[0.14em] text-cyan-200 uppercase">
          {t('Windows launcher and profile', 'Windows ランチャーとプロファイル')}
        </h2>
        <div class="mt-2 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
          <label>
            <span class="sr-only">{t('Launcher profile name', 'ランチャーのプロファイル名')}</span>
            <input
              type="text"
              class="w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white"
              bind:value={launcherProfile}
              disabled={!windows || busy !== null}
            />
          </label>
          <button
            type="button"
            class="rounded-lg bg-cyan-300/15 px-3 py-2 text-xs font-medium text-cyan-200 disabled:opacity-40"
            disabled={!windows || busy !== null}
            onclick={() => void generateLauncher()}
          >{busy === 'launcher' ? t('Generating…', '作成中…') : desktopMode ? t('Save .bat', '.bat を保存') : t('Download .bat', '.bat をダウンロード')}</button>
        </div>
        <label class="mt-2 flex items-center gap-2 text-xs text-slate-300">
          <input type="checkbox" class="accent-cyan-300" bind:checked={copyCurrent} disabled={!windows || busy !== null} />
          {t('Copy the current profile when creating a new profile', '新規プロファイル作成時に現在の内容をコピー')}
        </label>
        {#if !windows}
          <p class="mt-2 text-xs text-amber-200">
            {t('Launcher generation is available only on Windows.', 'ランチャー生成は Windows でのみ利用できます。')}
          </p>
        {/if}
      </section>

      <button
        type="button"
        class="w-full rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-left text-xs text-slate-200 hover:bg-white/10"
        onclick={resetView}
      >{t('Reset view size and position', '画面サイズと位置をリセット')}</button>
    </div>
  </details>

  <details class="group relative">
    <summary class="cursor-pointer list-none rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-xs font-medium text-slate-200 hover:bg-white/10">
      {t('Help', 'ヘルプ')}
    </summary>
    <div class="absolute right-0 mt-2 w-72 space-y-2 rounded-xl border border-white/10 bg-ink-900 p-3 text-xs shadow-2xl shadow-black/60">
      <p class="px-2 py-1 text-slate-400">PokeCon {openapi.info.version}</p>
      <a class="block rounded-lg px-2 py-2 text-slate-200 hover:bg-white/5" href="https://github.com/yqYo1/Poke-Controller-Modified-Extension" target="_blank" rel="noreferrer">GitHub</a>
      <a class="block rounded-lg px-2 py-2 text-slate-200 hover:bg-white/5" href="https://github.com/KawaSwitch/Poke-Controller/wiki" target="_blank" rel="noreferrer">Poke-Controller Guide</a>
      <a class="block rounded-lg px-2 py-2 text-slate-200 hover:bg-white/5" href="https://github.com/yqYo1/Poke-Controller-Modified-Extension/issues/new/choose" target="_blank" rel="noreferrer">{t('Question template', '質問テンプレート')}</a>
      <a class="block rounded-lg px-2 py-2 text-slate-200 hover:bg-white/5" href="https://github.com/yqYo1/Poke-Controller-Modified-Extension/blob/refactor/rust-core/changelog.txt" target="_blank" rel="noreferrer">{t('Change log', '更新履歴')}</a>
      <a class="block rounded-lg px-2 py-2 text-slate-200 hover:bg-white/5" href="https://github.com/yqYo1/Poke-Controller-Modified-Extension/blob/refactor/rust-core/LICENSE" target="_blank" rel="noreferrer">LICENSE</a>
      <button
        type="button"
        class="w-full rounded-lg bg-cyan-300/15 px-2 py-2 text-left text-cyan-200 disabled:opacity-40"
        disabled={busy !== null}
        onclick={() => void checkUpdate()}
      >{busy === 'update' ? t('Checking…', '確認中…') : t('Check for updates', 'アップデート確認')}</button>
      {#if updateResult !== null}
        <div class="rounded-lg bg-black/15 p-2 text-slate-400" aria-live="polite">
          <p>{updateResult.current_version} → {updateResult.latest_version}</p>
          {#if updateResult.update_available}
            <button type="button" class="mt-1 text-cyan-200 underline" onclick={openRelease}>{t('Open release', 'リリースを開く')}</button>
          {/if}
        </div>
      {/if}
    </div>
  </details>
</div>
