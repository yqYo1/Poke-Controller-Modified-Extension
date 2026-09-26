<script lang="ts">
  import { tick } from 'svelte';

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

  async function write(settings: SettingsWriteValues): Promise<boolean> {
    busy = 'settings';
    error = null;
    notice = null;
    try {
      await runtime.writeSettings(settings);
      return true;
    } catch (reason: unknown) {
      error = errorMessage(reason);
      return false;
    } finally {
      busy = null;
    }
  }

  let webhookDraft = $state('');
  const webhookConfigured = $derived(
    values?.['notifications.discord.webhook_url']?.configured === true
  );

  async function changeBoolean(event: Event, setting: BooleanSetting): Promise<void> {
    await write({ [setting]: (event.currentTarget as HTMLInputElement).checked });
  }

  async function changeText(event: Event, setting: TextSetting): Promise<void> {
    await write({ [setting]: (event.currentTarget as HTMLInputElement).value });
  }

  async function changeWebhook(event: Event): Promise<void> {
    const value = (event.currentTarget as HTMLInputElement).value;
    webhookDraft = value;
    if (await write({ 'notifications.discord.webhook_url': value })) {
      // Never keep the secret in component state after a successful write. The
      // next settings snapshot contains only `{ configured: boolean }`.
      webhookDraft = '';
      await tick();
    }
  }

  async function clearWebhook(): Promise<void> {
    if (await write({ 'notifications.discord.webhook_url': '' })) {
      webhookDraft = '';
      await tick();
    }
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
    <p class="text-xs font-semibold tracking-[0.12em] text-blue uppercase">Notifications</p>
    <h2 class="mt-2 text-2xl font-semibold text-text">
      {t('Windows / Discord notifications', 'Windows / Discord 通知')}
    </h2>
    <p class="mt-2 text-sm text-subtext1">
      {t(
        'Notification changes apply to future script starts and finishes.',
        '設定変更は、以後のスクリプト開始・終了イベントへ反映されます。'
      )}
    </p>
  </div>

  {#if error !== null}
    <div class="rounded-lg border border-red/20 bg-red/10 px-3 py-2 text-sm text-red" role="alert">{error}</div>
  {:else if notice !== null}
    <div class="rounded-lg border border-green/20 bg-green/10 px-3 py-2 text-sm text-green" role="status">{notice}</div>
  {/if}

  <section class="rounded-lg border border-text/10 bg-text/[0.025] p-4" aria-labelledby="windows-notifications">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h3 id="windows-notifications" class="font-semibold text-text">Windows</h3>
        <p class="mt-1 text-xs text-subtext1">
          {t(
            'Native delivery is available on Windows; settings are retained on other platforms.',
            'ネイティブ通知は Windows のみで送信され、他の環境でも設定値は保持されます。'
          )}
        </p>
      </div>
      <button
        type="button"
        class="rounded-lg border border-blue/30 bg-blue/10 px-3 py-2 text-sm font-medium text-blue disabled:opacity-40"
        disabled={busy !== null}
        onclick={() => void testChannel('windows')}
      >{busy === 'windows' ? t('Sending…', '送信中…') : t('Test Windows', 'Windows をテスト')}</button>
    </div>
    <div class="mt-4 grid gap-3 sm:grid-cols-2">
      <label class="flex items-start gap-3 rounded-lg bg-crust/15 p-3 text-sm text-text">
        <input
          type="checkbox"
          class="mt-0.5 size-4 accent-blue"
          checked={values?.['notifications.windows.on_script_start'] ?? false}
          disabled={busy !== null}
          onchange={(event) => void changeBoolean(event, 'notifications.windows.on_script_start')}
        />
        <span>{t('Notify when a script starts', 'スクリプト開始時に通知')}</span>
      </label>
      <label class="flex items-start gap-3 rounded-lg bg-crust/15 p-3 text-sm text-text">
        <input
          type="checkbox"
          class="mt-0.5 size-4 accent-blue"
          checked={values?.['notifications.windows.on_script_end'] ?? false}
          disabled={busy !== null}
          onchange={(event) => void changeBoolean(event, 'notifications.windows.on_script_end')}
        />
        <span>{t('Notify when a script finishes', 'スクリプト終了時に通知')}</span>
      </label>
    </div>
  </section>

  <section class="rounded-lg border border-text/10 bg-text/[0.025] p-4" aria-labelledby="discord-notifications">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h3 id="discord-notifications" class="font-semibold text-text">Discord</h3>
        <p class="mt-1 text-xs text-subtext1">
          {t(
            'The saved webhook is returned as a fixed mask and is never exposed to the browser.',
            '保存済み Webhook は固定マスクで返され、ブラウザーへ秘密値を公開しません。'
          )}
        </p>
      </div>
      <button
        type="button"
        class="rounded-lg border border-blue/30 bg-blue/10 px-3 py-2 text-sm font-medium text-blue disabled:opacity-40"
        disabled={busy !== null}
        onclick={() => void testChannel('discord')}
      >{busy === 'discord' ? t('Sending…', '送信中…') : t('Test Discord', 'Discord をテスト')}</button>
    </div>

    <div class="mt-4 grid gap-4 sm:grid-cols-2">
      <label class="sm:col-span-2" for="notifications-discord-webhook-url">
        <span class="text-xs font-medium text-subtext1">Webhook URL</span>
        <div class="mt-1 flex gap-2">
          <input
            id="notifications-discord-webhook-url"
            type="password"
            aria-label="Webhook URL"
            autocomplete="new-password"
            class="min-w-0 flex-1 rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text outline-none focus:border-blue/60"
            placeholder={webhookConfigured ? t('Saved webhook (enter to replace)', '保存済み（置き換える場合に入力）') : t('Enter webhook URL', 'Webhook URL を入力')}
            value={webhookDraft}
            disabled={busy !== null}
            onchange={(event) => void changeWebhook(event)}
          />
          {#if webhookConfigured}
            <button
              type="button"
              class="rounded-lg border border-red/30 bg-red/10 px-3 py-2 text-xs text-red disabled:opacity-40"
              disabled={busy !== null}
              onclick={() => void clearWebhook()}
            >{t('Clear', '消去')}</button>
          {/if}
        </div>
        <span class="mt-1 block text-xs text-subtext0">
          {webhookConfigured
            ? t('A saved value is not readable. Enter a new value only to replace it.', '保存済みの値は読み出せません。置き換える場合だけ新しい値を入力してください。')
            : t('The value is stored without being returned to the browser.', '値は保存されますが、ブラウザーへ返されません。')}
        </span>
      </label>
      <label>
        <span class="text-xs font-medium text-subtext1">{t('Username', 'ユーザー名')}</span>
        <input
          type="text"
          class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text outline-none focus:border-blue/60"
          value={values?.['notifications.discord.username'] ?? ''}
          disabled={busy !== null}
          onchange={(event) => void changeText(event, 'notifications.discord.username')}
        />
      </label>
      <label>
        <span class="text-xs font-medium text-subtext1">{t('Avatar URL', 'アバター URL')}</span>
        <input
          type="url"
          class="mt-1 w-full rounded-lg border border-text/10 bg-crust px-3 py-2 text-sm text-text outline-none focus:border-blue/60"
          value={values?.['notifications.discord.avatar_url'] ?? ''}
          disabled={busy !== null}
          onchange={(event) => void changeText(event, 'notifications.discord.avatar_url')}
        />
      </label>
      <label class="flex items-start gap-3 rounded-lg bg-crust/15 p-3 text-sm text-text">
        <input
          type="checkbox"
          class="mt-0.5 size-4 accent-blue"
          checked={values?.['notifications.discord.on_script_start'] ?? false}
          disabled={busy !== null}
          onchange={(event) => void changeBoolean(event, 'notifications.discord.on_script_start')}
        />
        <span>{t('Notify when a script starts', 'スクリプト開始時に通知')}</span>
      </label>
      <label class="flex items-start gap-3 rounded-lg bg-crust/15 p-3 text-sm text-text">
        <input
          type="checkbox"
          class="mt-0.5 size-4 accent-blue"
          checked={values?.['notifications.discord.on_script_end'] ?? false}
          disabled={busy !== null}
          onchange={(event) => void changeBoolean(event, 'notifications.discord.on_script_end')}
        />
        <span>{t('Notify when a script finishes', 'スクリプト終了時に通知')}</span>
      </label>
    </div>
  </section>
</div>
