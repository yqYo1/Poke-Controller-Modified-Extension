<svelte:head>
  <title>PokeCon Controller</title>
  <meta name="description" content="Low-latency camera, controller, command, and serial workspace" />
</svelte:head>

<script lang="ts">
  import { onDestroy, onMount } from 'svelte';

  import { BackendActions } from '$lib/actions';
  import CameraTab from '$lib/components/CameraTab.svelte';
  import CommandsTab from '$lib/components/CommandsTab.svelte';
  import ManualTab from '$lib/components/ManualTab.svelte';
  import RightPanel from '$lib/components/RightPanel.svelte';
  import SerialTab from '$lib/components/SerialTab.svelte';
  import { ApplicationRuntime, type RuntimeView } from '$lib/runtime';

  type TabId = 'camera' | 'serial' | 'manual' | 'commands' | 'notifications' | 'other';

  const actions = new BackendActions();
  const autoLoadDevices = import.meta.env.MODE !== 'test';
  const runtime = new ApplicationRuntime();
  const tabs: readonly { id: TabId; en: string; ja: string }[] = [
    { en: 'Camera', id: 'camera', ja: 'カメラ' },
    { en: 'Serial', id: 'serial', ja: 'シリアル' },
    { en: 'Manual Control', id: 'manual', ja: '手動制御' },
    { en: 'Commands', id: 'commands', ja: 'コマンド' },
    { en: 'Notifications', id: 'notifications', ja: '通知' },
    { en: 'Other', id: 'other', ja: 'その他' }
  ];
  let activeTab = $state<TabId>('camera');
  let view = $state<RuntimeView>();
  const unsubscribe = runtime.subscribe((next) => {
    view = next;
  });
  const language = $derived(view?.settings?.values.language ?? 'ja');

  function label(tab: (typeof tabs)[number]): string {
    return language === 'en' ? tab.en : tab.ja;
  }

  function handleTabKeydown(event: KeyboardEvent): void {
    const current = tabs.findIndex((tab) => tab.id === activeTab);
    const next =
      event.key === 'ArrowRight'
        ? (current + 1) % tabs.length
        : event.key === 'ArrowLeft'
          ? (current - 1 + tabs.length) % tabs.length
          : event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? tabs.length - 1
              : null;
    if (next === null) return;
    event.preventDefault();
    const tab = tabs[next];
    if (tab === undefined) return;
    activeTab = tab.id;
    const buttons = (event.currentTarget as HTMLElement)
      .closest('[role="tablist"]')
      ?.querySelectorAll<HTMLElement>('[role="tab"]');
    buttons?.[next]?.focus();
  }

  onMount(() => {
    if (import.meta.env.MODE === 'test') return;
    runtime.start();
    const stop = () => runtime.stop();
    window.addEventListener('pagehide', stop);
    return () => {
      window.removeEventListener('pagehide', stop);
      runtime.stop();
    };
  });

  onDestroy(unsubscribe);
</script>

{#if view !== undefined}
  <main class="mx-auto flex min-h-screen w-full max-w-[1920px] flex-col p-3 sm:p-5 lg:p-6">
    <header class="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 px-1 pb-4">
      <div class="flex items-center gap-3">
        <div class="grid size-10 place-items-center rounded-xl border border-cyan-300/30 bg-cyan-300/10 font-black text-cyan-300" aria-hidden="true">PC</div>
        <div>
          <p class="text-xs font-semibold tracking-[0.22em] text-cyan-300 uppercase">PokeCon</p>
          <h1 class="text-lg font-semibold text-white">Controller workspace</h1>
        </div>
      </div>
      <div class="flex flex-wrap items-center justify-end gap-2 text-xs" aria-live="polite">
        <span class={`size-2 rounded-full ${view.realtime.status === 'connected' ? 'bg-lime-300' : view.realtime.status === 'exhausted' ? 'bg-red-400' : 'bg-amber-300'}`}></span>
        <span class="text-slate-300">{view.realtime.status}</span>
        <span class="rounded-full bg-white/5 px-2 py-1 text-slate-400">Media: {view.media.mode}</span>
        {#if view.state !== null}<span class="rounded-full bg-white/5 px-2 py-1 text-slate-400">PID {view.state.pid}</span>{/if}
        {#if view.realtime.status === 'exhausted'}
          <button type="button" class="rounded-md bg-cyan-300/15 px-2 py-1 text-cyan-200" onclick={() => runtime.reconnectWebSocket()}>Reconnect</button>
        {/if}
      </div>
    </header>

    {#if view.actionError !== null || view.notice !== null}
      <div class={`mt-3 flex items-start justify-between gap-3 rounded-lg border px-3 py-2 text-sm ${view.actionError === null ? 'border-lime-300/20 bg-lime-300/10 text-lime-200' : 'border-red-400/20 bg-red-400/10 text-red-200'}`} role="status">
        <span>{view.actionError ?? view.notice}</span>
        <button type="button" class="text-xs opacity-70 hover:opacity-100" aria-label="Dismiss message" onclick={() => runtime.dismissFeedback()}>Dismiss</button>
      </div>
    {/if}

    <nav class="mt-4 overflow-x-auto" aria-label={language === 'en' ? 'Main views' : 'メイン画面'}>
      <div class="flex min-w-max gap-1" role="tablist">
        {#each tabs as tab (tab.id)}
          <button
            id={`tab-${tab.id}`}
            type="button"
            role="tab"
            aria-selected={activeTab === tab.id}
            aria-controls={`panel-${tab.id}`}
            tabindex={activeTab === tab.id ? 0 : -1}
            class={`rounded-lg border px-4 py-2 text-sm font-medium transition hover:bg-white/5 hover:text-white ${activeTab === tab.id ? 'border-cyan-300/40 bg-cyan-300/10 text-cyan-200' : 'border-transparent text-slate-400'}`}
            onclick={() => (activeTab = tab.id)}
            onkeydown={handleTabKeydown}
          >{label(tab)}</button>
        {/each}
      </div>
    </nav>

    <div class="mt-4 grid flex-1 items-start gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(28rem,44rem)]">
      <div id={`panel-${activeTab}`} class="min-h-[34rem] rounded-2xl border border-white/10 bg-ink-900/80 p-4 shadow-2xl shadow-black/20" role="tabpanel" aria-labelledby={`tab-${activeTab}`}>
        {#if activeTab === 'camera'}
          <CameraTab {actions} autoLoad={autoLoadDevices} {runtime} {view} />
        {:else if activeTab === 'serial'}
          <SerialTab {actions} autoLoad={autoLoadDevices} {runtime} {view} />
        {:else if activeTab === 'manual'}
          <ManualTab {runtime} {view} />
        {:else if activeTab === 'commands'}
          <CommandsTab {actions} {runtime} {view} />
        {:else if activeTab === 'notifications'}
          <p class="text-xs font-semibold tracking-[0.2em] text-lime-300 uppercase">Notifications</p>
          <h2 class="mt-2 text-2xl font-semibold text-white">Windows / Discord</h2>
        {:else}
          <p class="text-xs font-semibold tracking-[0.2em] text-lime-300 uppercase">Other</p>
          <h2 class="mt-2 text-2xl font-semibold text-white">{language === 'en' ? 'Application settings' : 'アプリケーション設定'}</h2>
          <p class="mt-4 text-slate-400">Revision {view.settings?.revision ?? '—'} · Profile {view.state?.active_profile ?? '—'}</p>
        {/if}
      </div>

      <RightPanel runtime={runtime} view={view} />
    </div>
  </main>
{/if}
