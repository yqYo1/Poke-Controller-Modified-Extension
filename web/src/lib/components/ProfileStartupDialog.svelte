<script lang="ts">
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  interface Props {
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { runtime, view }: Props = $props();

  const DISMISS_KEY = 'pokecon.startup-profile-dismissed';
  let dismissed = $state(false);
  let selected = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  const profiles = $derived(view.state?.available_profiles ?? []);
  const language = $derived(view.settings?.values.language ?? 'ja');
  const shouldOpen = $derived(
    !dismissed &&
      profiles.length > 1 &&
      (view.state?.active_profile ?? 'default') === 'default'
  );

  $effect(() => {
    if (typeof window === 'undefined') return;
    dismissed = window.localStorage.getItem(DISMISS_KEY) === '1';
  });

  $effect(() => {
    if (!shouldOpen) return;
    if (!profiles.includes(selected)) {
      selected = view.state?.active_profile ?? profiles[0] ?? '';
    }
  });

  function t(english: string, japanese: string): string {
    return language === 'en' ? english : japanese;
  }

  function dismiss(): void {
    dismissed = true;
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(DISMISS_KEY, '1');
    }
  }

  async function confirm(): Promise<void> {
    if (selected.length === 0 || busy) return;
    busy = true;
    error = null;
    try {
      await runtime.writeSettings({ active_profile: selected });
      dismiss();
    } catch (reason: unknown) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      busy = false;
    }
  }
</script>

{#if shouldOpen}
  <div class="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4" role="presentation">
    <dialog
      open
      class="w-full max-w-md rounded-xl border border-text/15 bg-surface1 p-5 shadow-2xl"
      aria-modal="true"
      aria-labelledby="startup-profile-title"
    >
      <h2 id="startup-profile-title" class="text-base font-semibold text-text">
        {t('Choose a profile', 'プロファイルを選択')}
      </h2>
      <p class="mt-2 text-sm text-subtext0">
        {t(
          'Multiple profiles are available. Choose the one to use for this session.',
          '複数のプロファイルがあります。この起動で使うプロファイルを選んでください。'
        )}
      </p>
      <label class="mt-4 block text-xs text-subtext1">
        {t('Profile', 'プロファイル')}
        <select
          class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-3 py-2 text-sm text-text"
          aria-label={t('Startup profile', '起動時プロファイル')}
          value={selected}
          onchange={(event) => (selected = event.currentTarget.value)}
        >
          {#each profiles as profile (profile)}
            <option value={profile}>{profile}</option>
          {/each}
        </select>
      </label>
      {#if error !== null}
        <p class="mt-3 text-xs text-red" role="alert">{error}</p>
      {/if}
      <div class="mt-5 flex flex-wrap justify-end gap-2">
        <button
          type="button"
          class="rounded-md bg-text/5 px-3 py-1.5 text-sm text-subtext1"
          onclick={dismiss}
        >
          {t('Use current', '現在のまま開始')}
        </button>
        <button
          type="button"
          class="rounded-md bg-blue/15 px-3 py-1.5 text-sm text-blue disabled:opacity-40"
          disabled={busy || selected.length === 0}
          onclick={() => void confirm()}
        >
          {t('Switch', '切替')}
        </button>
      </div>
    </dialog>
  </div>
{/if}
