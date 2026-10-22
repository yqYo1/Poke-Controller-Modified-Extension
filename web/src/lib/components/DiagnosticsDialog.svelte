<script lang="ts">
  import packageJson from '../../../package.json';

  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  interface Props {
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { runtime, view }: Props = $props();
  let open = $state(false);
  let writeState = $state<'unknown' | 'checking' | 'writable' | 'failed'>('unknown');
  let copied = $state(false);

  const language = $derived(view.settings?.values.language ?? 'ja');
  const diagnostics = $derived({
    version: packageJson.version,
    profile: view.state?.active_profile ?? null,
    revision: view.settings?.revision ?? null,
    serial: {
      port: view.state?.serial_port ?? null,
      baud_rate: view.state?.serial_baud_rate ?? null,
      connected: view.state?.serial_connected ?? false
    },
    camera: {
      opened: view.state?.camera_opened ?? false,
      fps: view.state?.camera_fps ?? null,
      resolution: view.state?.camera_resolution ?? null,
      device: view.state?.camera_device ?? null
    },
    command: {
      state: view.state?.command_state ?? null,
      current: view.state?.current_command ?? null
    },
    realtime: view.realtime.status,
    media: view.media.mode,
    write_state: writeState
  });

  function t(english: string, japanese: string): string {
    return language === 'en' ? english : japanese;
  }

  async function testWrite(): Promise<void> {
    writeState = 'checking';
    try {
      await runtime.writeSettings({ language });
      writeState = 'writable';
    } catch {
      writeState = 'failed';
    }
  }

  async function copyDiagnostics(): Promise<void> {
    const text = JSON.stringify(diagnostics, null, 2);
    const clipboard: unknown = Reflect.get(navigator, 'clipboard');
    if (typeof clipboard !== 'object' || clipboard === null) return;
    const writeText: unknown = Reflect.get(clipboard, 'writeText');
    if (typeof writeText !== 'function') return;
    try {
      await Reflect.apply(writeText, clipboard, [text]);
      copied = true;
      setTimeout(() => (copied = false), 1_500);
    } catch {
      // Clipboard access is optional.
    }
  }
</script>

<button
  type="button"
  class="rounded-lg bg-text/5 px-3 py-2 text-xs text-subtext1 hover:bg-text/10"
  onclick={() => (open = true)}
>{t('Diagnostics', '環境の診断')}</button>

{#if open}
  <div class="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4" role="presentation">
    <dialog
      open
      class="max-h-[85vh] w-full max-w-2xl overflow-auto rounded-xl border border-text/15 bg-surface1 p-5 shadow-2xl"
      aria-modal="true"
      aria-labelledby="diagnostics-title"
    >
      <div class="flex items-center justify-between gap-3">
        <h2 id="diagnostics-title" class="text-base font-semibold text-text">
          {t('Diagnostics', '環境の診断')}
        </h2>
        <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-xs text-subtext1" onclick={() => (open = false)}>
          {t('Close', '閉じる')}
        </button>
      </div>

      <dl class="mt-4 grid gap-2 text-sm sm:grid-cols-2">
        <dt class="text-subtext1">{t('Version', 'バージョン')}</dt>
        <dd class="font-mono text-text">{packageJson.version}</dd>
        <dt class="text-subtext1">{t('Profile', 'プロファイル')}</dt>
        <dd class="font-mono text-text">{view.state?.active_profile ?? '-'}</dd>
        <dt class="text-subtext1">{t('Settings revision', '設定revision')}</dt>
        <dd class="font-mono text-text">{view.settings?.revision ?? '-'}</dd>
        <dt class="text-subtext1">{t('Serial', 'シリアル')}</dt>
        <dd class="font-mono text-text">
          {view.state?.serial_connected ? t('connected', '接続') : t('disconnected', '未接続')}
          <span class="mx-1">/</span>{view.state?.serial_port ?? '-'}<span class="mx-1">/</span>{String(view.state?.serial_baud_rate ?? '-')}
        </dd>
        <dt class="text-subtext1">{t('Camera', 'カメラ')}</dt>
        <dd class="font-mono text-text">
          {view.state?.camera_opened ? t('open', 'オープン') : t('closed', 'クローズ')}
          <span class="mx-1">/</span>{view.state?.camera_resolution ?? '-'}<span class="mx-1">/</span>{String(view.state?.camera_fps ?? '-')} fps
        </dd>
        <dt class="text-subtext1">{t('Command', 'コマンド')}</dt>
        <dd class="font-mono text-text">{view.state?.command_state ?? '-'}<span class="mx-1">/</span>{view.state?.current_command ?? '-'}</dd>
        <dt class="text-subtext1">{t('Realtime', 'リアルタイム')}</dt>
        <dd class="font-mono text-text">{view.realtime.status}<span class="mx-1">/</span>{view.media.mode}</dd>
        <dt class="text-subtext1">{t('Write check', '書込可否')}</dt>
        <dd class="font-mono text-text">
          {writeState === 'writable' ? t('writable', '書込可') : writeState === 'failed' ? t('failed', '失敗') : writeState === 'checking' ? t('checking', '確認中') : t('not checked', '未確認')}
        </dd>
      </dl>

      <div class="mt-5 flex flex-wrap gap-2">
        <button type="button" class="rounded-md bg-blue/15 px-3 py-1.5 text-sm text-blue disabled:opacity-40" disabled={writeState === 'checking'} onclick={() => void testWrite()}>
          {t('Test write access', '書込可否を確認')}
        </button>
        <button type="button" class="rounded-md bg-text/5 px-3 py-1.5 text-sm text-subtext1" onclick={() => void copyDiagnostics()}>
          {copied ? t('Copied', 'コピー済み') : t('Copy masked diagnostics', 'マスク済み診断をコピー')}
        </button>
      </div>
      <p class="mt-3 text-xs text-subtext0">
        {t('Copied diagnostics contain no webhook secret; secret values are already masked by the settings API.', 'コピー内容にWebhook secretは含まれません。secret値は設定APIでマスク済みです。')}
      </p>
    </dialog>
  </div>
{/if}
