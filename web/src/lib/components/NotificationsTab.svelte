<script lang="ts">
  import type {
    NotificationTestRequest,
    NotificationTestResult
  } from '../actions';
  import type { SettingsWriteValues } from '../api';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  type Channel = NotificationTestRequest['channel'];
  type BooleanSetting =
    | 'notifications.windows.on_script_start'
    | 'notifications.windows.on_script_end'
    | 'notifications.discord.on_script_start'
    | 'notifications.discord.on_script_end';
  type TextSetting =
    | 'notifications.discord.webhook_url'
    | 'notifications.discord.username'
    | 'notifications.discord.avatar_url';

  interface NotificationActions {
    testNotification(request: NotificationTestRequest): Promise<NotificationTestResult>;
  }

  interface Props {
    actions: NotificationActions;
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { actions, runtime, view }: Props = $props();
  let busy = $state<Channel | 'settings' | null>(null);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);

  const values = $derived(view.settings?.values);
  const english = $derived(values?.language === 'en');

  function t(en: string, ja: string): string {
    return english ? en : ja;
  }

  function errorMessage(reason: unknown): string {
    return reason instanceof Error ? reason.message : 'Notification operation failed';
  }

  async function write(settings: SettingsWriteValues): Promise<void> {
    busy = 'settings';
    error = null;
    notice = null;
    try {
      await runtime.writeSettings(settings);
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }

  async function changeBoolean(event: Event, setting: BooleanSetting): Promise<void> {
    await write({ [setting]: (event.currentTarget as HTMLInputElement).checked });
  }

  async function changeText(event: Event, setting: TextSetting): Promise<void> {
    await write({ [setting]: (event.currentTarget as HTMLInputElement).value });
  }

  async function testChannel(channel: Channel): Promise<void> {
    busy = channel;
    error = null;
    notice = null;
    try {
      const result = await actions.testNotification({ channel });
      notice = result.delivered
        ? t(`${channel} test notification delivered.`, `${channel} のテスト通知を送信しました。`)
        : t(
            `${channel} test notification was not delivered.`,
            `${channel} のテスト通知は送信されませんでした。`
          );
    } catch (reason: unknown) {
      error = errorMessage(reason);
    } finally {
      busy = null;
    }
  }
</script>

<div class="space-y-5">
  <div>
    <p class="text-xs font-semibold tracking-[0.2em] text-lime-300 uppercase">Notifications</p>
    <h2 class="mt-2 text-2xl font-semibold text-white">
      {t('Windows / Discord notifications', 'Windows / Discord 通知')}
    </h2>
    <p class="mt-2 text-sm text-slate-400">
      {t(
        'Notification changes apply to future script starts and finishes.',
        '設定変更は、以後のスクリプト開始・終了イベントへ反映されます。'
      )}
    </p>
  </div>

  {#if error !== null}
    <div class="rounded-lg border border-red-400/20 bg-red-400/10 px-3 py-2 text-sm text-red-200" role="alert">{error}</div>
  {:else if notice !== null}
    <div class="rounded-lg border border-lime-300/20 bg-lime-300/10 px-3 py-2 text-sm text-lime-200" role="status">{notice}</div>
  {/if}

  <section class="rounded-xl border border-white/10 bg-white/[0.025] p-4" aria-labelledby="windows-notifications">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h3 id="windows-notifications" class="font-semibold text-white">Windows</h3>
        <p class="mt-1 text-xs text-slate-400">
          {t(
            'Native delivery is available on Windows; settings are retained on other platforms.',
            'ネイティブ通知は Windows のみで送信され、他の環境でも設定値は保持されます。'
          )}
        </p>
      </div>
      <button
        type="button"
        class="rounded-lg border border-cyan-300/30 bg-cyan-300/10 px-3 py-2 text-sm font-medium text-cyan-200 disabled:opacity-40"
        disabled={busy !== null}
        onclick={() => void testChannel('windows')}
      >{busy === 'windows' ? t('Sending…', '送信中…') : t('Test Windows', 'Windows をテスト')}</button>
    </div>
    <div class="mt-4 grid gap-3 sm:grid-cols-2">
      <label class="flex items-start gap-3 rounded-lg bg-black/15 p-3 text-sm text-slate-200">
        <input
          type="checkbox"
          class="mt-0.5 size-4 accent-cyan-300"
          checked={values?.['notifications.windows.on_script_start'] ?? false}
          disabled={busy !== null}
          onchange={(event) => void changeBoolean(event, 'notifications.windows.on_script_start')}
        />
        <span>{t('Notify when a script starts', 'スクリプト開始時に通知')}</span>
      </label>
      <label class="flex items-start gap-3 rounded-lg bg-black/15 p-3 text-sm text-slate-200">
        <input
          type="checkbox"
          class="mt-0.5 size-4 accent-cyan-300"
          checked={values?.['notifications.windows.on_script_end'] ?? false}
          disabled={busy !== null}
          onchange={(event) => void changeBoolean(event, 'notifications.windows.on_script_end')}
        />
        <span>{t('Notify when a script finishes', 'スクリプト終了時に通知')}</span>
      </label>
    </div>
  </section>

  <section class="rounded-xl border border-white/10 bg-white/[0.025] p-4" aria-labelledby="discord-notifications">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h3 id="discord-notifications" class="font-semibold text-white">Discord</h3>
        <p class="mt-1 text-xs text-slate-400">
          {t(
            'The saved webhook is returned as a fixed mask and is never exposed to the browser.',
            '保存済み Webhook は固定マスクで返され、ブラウザーへ秘密値を公開しません。'
          )}
        </p>
      </div>
      <button
        type="button"
        class="rounded-lg border border-cyan-300/30 bg-cyan-300/10 px-3 py-2 text-sm font-medium text-cyan-200 disabled:opacity-40"
        disabled={busy !== null}
        onclick={() => void testChannel('discord')}
      >{busy === 'discord' ? t('Sending…', '送信中…') : t('Test Discord', 'Discord をテスト')}</button>
    </div>

    <div class="mt-4 grid gap-4 sm:grid-cols-2">
      <label class="sm:col-span-2">
        <span class="text-xs font-medium text-slate-300">Webhook URL</span>
        <input
          type="password"
          autocomplete="new-password"
          class="mt-1 w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white outline-none focus:border-cyan-300/60"
          value={values?.['notifications.discord.webhook_url'] ?? ''}
          disabled={busy !== null}
          onchange={(event) => void changeText(event, 'notifications.discord.webhook_url')}
        />
      </label>
      <label>
        <span class="text-xs font-medium text-slate-300">{t('Username', 'ユーザー名')}</span>
        <input
          type="text"
          class="mt-1 w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white outline-none focus:border-cyan-300/60"
          value={values?.['notifications.discord.username'] ?? ''}
          disabled={busy !== null}
          onchange={(event) => void changeText(event, 'notifications.discord.username')}
        />
      </label>
      <label>
        <span class="text-xs font-medium text-slate-300">{t('Avatar URL', 'アバター URL')}</span>
        <input
          type="url"
          class="mt-1 w-full rounded-lg border border-white/10 bg-ink-950 px-3 py-2 text-sm text-white outline-none focus:border-cyan-300/60"
          value={values?.['notifications.discord.avatar_url'] ?? ''}
          disabled={busy !== null}
          onchange={(event) => void changeText(event, 'notifications.discord.avatar_url')}
        />
      </label>
      <label class="flex items-start gap-3 rounded-lg bg-black/15 p-3 text-sm text-slate-200">
        <input
          type="checkbox"
          class="mt-0.5 size-4 accent-cyan-300"
          checked={values?.['notifications.discord.on_script_start'] ?? false}
          disabled={busy !== null}
          onchange={(event) => void changeBoolean(event, 'notifications.discord.on_script_start')}
        />
        <span>{t('Notify when a script starts', 'スクリプト開始時に通知')}</span>
      </label>
      <label class="flex items-start gap-3 rounded-lg bg-black/15 p-3 text-sm text-slate-200">
        <input
          type="checkbox"
          class="mt-0.5 size-4 accent-cyan-300"
          checked={values?.['notifications.discord.on_script_end'] ?? false}
          disabled={busy !== null}
          onchange={(event) => void changeBoolean(event, 'notifications.discord.on_script_end')}
        />
        <span>{t('Notify when a script finishes', 'スクリプト終了時に通知')}</span>
      </label>
    </div>
  </section>
</div>
