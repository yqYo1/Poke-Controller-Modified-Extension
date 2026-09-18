<svelte:head>
  <title>PokeCon Controller</title>
  <meta name="description" content="Low-latency camera, controller, command, and serial workspace" />
</svelte:head>

<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';

  import { BackendActions } from '$lib/actions';
  import CameraTab from '$lib/components/CameraTab.svelte';
  import CommandsTab from '$lib/components/CommandsTab.svelte';
  import InputSafety from '$lib/components/InputSafety.svelte';
  import ManualTab from '$lib/components/ManualTab.svelte';
  import MainPanel from '$lib/components/MainPanel.svelte';
  import NotificationsTab from '$lib/components/NotificationsTab.svelte';
  import OtherTab from '$lib/components/OtherTab.svelte';
  import RightPanel from '$lib/components/RightPanel.svelte';
  import SerialTab from '$lib/components/SerialTab.svelte';
  import ScriptUiLayer from '$lib/components/ScriptUiLayer.svelte';
  import WorkspaceMenu from '$lib/components/WorkspaceMenu.svelte';
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
  let workspaceElement = $state<HTMLElement>();
  let panelElement = $state<HTMLElement>();
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

  function resetScroll(element: HTMLElement | undefined): void {
    if (element === undefined) return;
    element.scrollTop = 0;
    element.scrollLeft = 0;
  }

  async function resetView(): Promise<void> {
    activeTab = 'camera';
    resetScroll(workspaceElement);
    resetScroll(panelElement);
    await tick();
    resetScroll(workspaceElement);
    resetScroll(panelElement);
  }

  function navigate(target: TabId): void {
    activeTab = target;
    document.getElementById(`tab-${target}`)?.focus({ preventScroll: true });
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
  <InputSafety {runtime} {view} />
  <main bind:this={workspaceElement} class="mx-auto flex h-screen min-h-0 w-full max-w-[1920px] flex-col overflow-auto p-3 sm:p-5 lg:p-6">
    <header class="flex flex-wrap items-center justify-between gap-4 border-b border-text/10 px-1 pb-4">
      <div class="flex items-center gap-3">
        <div class="grid size-10 place-items-center rounded-lg border border-blue/30 bg-blue/10 font-bold text-blue" aria-hidden="true">PC</div>
        <div>
          <p class="text-xs font-semibold tracking-[0.12em] text-blue uppercase">PokeCon</p>
          <h1 class="text-lg font-semibold text-text">Controller workspace</h1>
        </div>
      </div>
      <div class="flex flex-wrap items-center justify-end gap-3">
        <div class="flex flex-wrap items-center justify-end gap-2 text-xs" aria-live="polite">
          <span class={`size-2 rounded-full ${view.realtime.status === 'connected' ? 'bg-green' : view.realtime.status === 'exhausted' ? 'bg-red' : 'bg-yellow'}`}></span>
          <span class="text-subtext1">{view.realtime.status}</span>
          <span class="rounded-full bg-text/5 px-2 py-1 text-subtext1">Media: {view.media.mode}</span>
          {#if view.state !== null}<span class="rounded-full bg-text/5 px-2 py-1 text-subtext1">PID {view.state.pid}</span>{/if}
          {#if view.realtime.status === 'exhausted'}
            <button type="button" class="rounded-md bg-blue/15 px-2 py-1 text-blue" onclick={() => runtime.reconnectWebSocket()}>Reconnect</button>
          {/if}
        </div>
        <WorkspaceMenu {actions} onresetview={resetView} {runtime} {view} />
      </div>
    </header>

    {#if view.actionError !== null || view.notice !== null}
      <div class={`mt-3 flex items-start justify-between gap-3 rounded-lg border px-3 py-2 text-sm ${view.actionError === null ? 'border-green/20 bg-green/10 text-green' : 'border-red/20 bg-red/10 text-red'}`} role="status">
        <span>{view.actionError ?? view.notice}</span>
        <button type="button" class="text-xs opacity-70 hover:opacity-100" aria-label="Dismiss message" onclick={() => runtime.dismissFeedback()}>Dismiss</button>
      </div>
    {/if}

    <div class="mt-4 grid min-h-0 flex-1 items-stretch gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,26rem)] xl:grid-cols-[minmax(0,1fr)_minmax(28rem,44rem)]">
      <div class="flex min-h-0 min-w-0 flex-col gap-4">
        <MainPanel {actions} onnavigate={navigate} {runtime} {view} />

        <nav class="shrink-0 overflow-x-auto" aria-label={language === 'en' ? 'Main views' : 'メイン画面'}>
          <div class="flex min-w-max gap-1" role="tablist">
            {#each tabs as tab (tab.id)}
              <button
                id={`tab-${tab.id}`}
                type="button"
                role="tab"
                aria-selected={activeTab === tab.id}
                aria-controls={`panel-${tab.id}`}
                tabindex={activeTab === tab.id ? 0 : -1}
                class={`rounded-lg border px-4 py-2 text-sm font-medium transition hover:bg-text/5 hover:text-text ${activeTab === tab.id ? 'border-blue/40 bg-blue/10 text-blue' : 'border-transparent text-subtext1'}`}
                onclick={() => (activeTab = tab.id)}
                onkeydown={handleTabKeydown}
              >{label(tab)}</button>
            {/each}
          </div>
        </nav>

        <div bind:this={panelElement} id={`panel-${activeTab}`} class="min-h-[20rem] min-w-0 flex-1 overflow-auto rounded-lg border border-text/10 bg-mantle/80 p-4 shadow-md shadow-crust/20" role="tabpanel" aria-labelledby={`tab-${activeTab}`}>
          {#key activeTab}
            {#if activeTab === 'camera'}
              <CameraTab {actions} autoLoad={autoLoadDevices} {runtime} {view} />
            {:else if activeTab === 'serial'}
              <SerialTab {actions} autoLoad={autoLoadDevices} {runtime} {view} />
            {:else if activeTab === 'manual'}
              <ManualTab {runtime} {view} />
            {:else if activeTab === 'commands'}
              <CommandsTab {actions} {runtime} {view} />
            {:else if activeTab === 'notifications'}
              <NotificationsTab {actions} {runtime} {view} />
            {:else}
              <OtherTab {runtime} {view} />
            {/if}
          {/key}
        </div>
      </div>

      <RightPanel runtime={runtime} view={view} />
    </div>
  </main>
  <ScriptUiLayer {actions} {view} />
{/if}
