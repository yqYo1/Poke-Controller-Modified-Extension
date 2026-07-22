<svelte:head>
  <title>PokeCon Controller</title>
  <meta
    name="description"
    content="Low-latency camera, controller, command, and serial workspace"
  />
</svelte:head>

<script lang="ts">
  const tabs = ['Controller', 'Commands', 'Serial', 'Settings', 'Profiles', 'About'] as const;
  let activeTab = $state<(typeof tabs)[number]>('Controller');
</script>

<main class="mx-auto flex min-h-screen w-full max-w-[1600px] flex-col p-3 sm:p-5 lg:p-7">
  <header
    class="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 px-1 pb-5"
  >
    <div class="flex items-center gap-3">
      <div
        class="grid size-10 place-items-center rounded-xl border border-cyan-300/30 bg-cyan-300/10 font-black text-cyan-300"
        aria-hidden="true"
      >
        PC
      </div>
      <div>
        <p class="text-xs font-semibold tracking-[0.22em] text-cyan-300 uppercase">PokeCon</p>
        <h1 class="text-lg font-semibold text-white">Controller workspace</h1>
      </div>
    </div>
    <div class="flex items-center gap-2 text-sm text-slate-300" aria-live="polite">
      <span class="size-2 rounded-full bg-slate-500"></span>
      接続待機中
    </div>
  </header>

  <nav class="mt-4 overflow-x-auto" aria-label="メイン画面">
    <div class="flex min-w-max gap-1" role="tablist">
      {#each tabs as tab (tab)}
        <button
          type="button"
          role="tab"
          aria-selected={activeTab === tab}
          aria-controls="main-panel"
          tabindex={activeTab === tab ? 0 : -1}
          class={`rounded-lg border px-4 py-2 text-sm font-medium transition hover:bg-white/5 hover:text-white ${
            activeTab === tab
              ? 'border-cyan-300/40 bg-cyan-300/10 text-cyan-200'
              : 'border-transparent text-slate-400'
          }`}
          onclick={() => (activeTab = tab)}
        >{tab}</button>
      {/each}
    </div>
  </nav>

  <div
    id="main-panel"
    class="mt-4 grid flex-1 place-items-center rounded-2xl border border-white/10 bg-ink-900/80 p-8 shadow-2xl shadow-black/20"
    role="tabpanel"
    aria-label={activeTab}
  >
    <div class="max-w-xl text-center">
      <p class="text-xs font-semibold tracking-[0.2em] text-lime-300 uppercase">Phase 11</p>
      <h2 class="mt-3 text-3xl font-semibold tracking-tight text-white sm:text-4xl">
        {activeTab}
      </h2>
      <p class="mt-4 text-pretty leading-7 text-slate-400">
        生成 OpenAPI 契約と静的 SPA の土台が読み込まれました。各リアルタイム画面をこの shell
        に接続します。
      </p>
    </div>
  </div>
</main>
