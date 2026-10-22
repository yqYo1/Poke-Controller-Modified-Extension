<script lang="ts">
  import { onMount } from 'svelte';

  import type { OutputLine } from '../runtime';

  type LogLevel = OutputLine['level'];

  interface Props {
    label: string;
    lines: readonly OutputLine[];
    onclear: () => void;
    storageKey: string;
    profile: string;
  }

  const ranks: Readonly<Record<LogLevel, number>> = {
    critical: 4,
    debug: 0,
    error: 3,
    info: 1,
    warning: 2
  };
  const levels: readonly LogLevel[] = ['debug', 'info', 'warning', 'error', 'critical'];

  let { label, lines, onclear, storageKey, profile }: Props = $props();
  let activeTab = $state<'log' | 'note'>('log');
  let note = $state('');
  let noteLoadedKey = '';
  let autoScroll = $state(true);
  let copied = $state(false);
  let minimumLevel = $state<LogLevel>('debug');
  let outputElement = $state<HTMLDivElement>();
  let storageReady = false;
  const noteKey = $derived(`${storageKey}.note.${profile}`);

  const filteredLines = $derived(
    lines.filter((line) => ranks[line.level] >= ranks[minimumLevel])
  );

  $effect(() => {
    if (typeof localStorage === 'undefined') return;
    const key = noteKey;
    if (key === noteLoadedKey) return;
    try {
      note = localStorage.getItem(key) ?? '';
    } catch {
      note = '';
    }
    noteLoadedKey = key;
  });

  $effect(() => {
    if (typeof localStorage === 'undefined' || noteLoadedKey !== noteKey) return;
    try {
      localStorage.setItem(noteKey, note);
    } catch {
      // Browser storage is an optional client-only convenience.
    }
  });

  onMount(() => {
    try {
      autoScroll = localStorage.getItem(`${storageKey}.auto-scroll`) !== 'false';
      const storedLevel = localStorage.getItem(`${storageKey}.minimum-level`) as LogLevel | null;
      if (storedLevel !== null && levels.includes(storedLevel)) minimumLevel = storedLevel;
    } catch {
      // Browser storage is an optional client-only convenience.
    }
    storageReady = true;
  });

  $effect(() => {
    if (!storageReady) return;
    try {
      localStorage.setItem(`${storageKey}.auto-scroll`, String(autoScroll));
      localStorage.setItem(`${storageKey}.minimum-level`, minimumLevel);
    } catch {
      // Private browsing modes may make storage unavailable.
    }
  });

  $effect(() => {
    if (!autoScroll || outputElement === undefined || filteredLines.length === 0) return;
    const element = outputElement;
    queueMicrotask(() => {
      element.scrollTop = element.scrollHeight;
    });
  });

  async function copy(): Promise<void> {
    const clipboard: unknown = Reflect.get(navigator, 'clipboard');
    if (typeof clipboard !== 'object' || clipboard === null) return;
    const writeText: unknown = Reflect.get(clipboard, 'writeText');
    if (typeof writeText !== 'function') return;
    try {
      await Reflect.apply(writeText, clipboard, [
        filteredLines.map((line) => line.message).join('')
      ]);
    } catch {
      return;
    }
    copied = true;
    setTimeout(() => (copied = false), 1_500);
  }
</script>

<section class="flex min-h-0 flex-col overflow-hidden rounded-lg border border-text/10 bg-crust/25" aria-label={label}>
  <div class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-text/10 px-3 py-2">
    <h3 class="text-xs font-semibold tracking-[0.14em] text-text uppercase">{label}</h3>
    <div class="flex gap-1" role="tablist" aria-label={`${label} view`}>
      <button type="button" role="tab" aria-selected={activeTab === 'log'} class={`rounded-md px-2 py-1 text-[11px] ${activeTab === 'log' ? 'bg-blue/15 text-blue' : 'bg-text/5 text-subtext1'}`} onclick={() => (activeTab = 'log')}>Log</button>
      <button type="button" role="tab" aria-selected={activeTab === 'note'} class={`rounded-md px-2 py-1 text-[11px] ${activeTab === 'note' ? 'bg-blue/15 text-blue' : 'bg-text/5 text-subtext1'}`} onclick={() => (activeTab = 'note')}>Note</button>
    </div>
    {#if activeTab === 'log'}
    <div class="flex items-center gap-1">
      <label class="sr-only" for={`${storageKey}-level`}>Minimum log level</label>
      <select id={`${storageKey}-level`} class="rounded-md border border-text/10 bg-surface1 px-2 py-1 text-[11px] text-subtext1" bind:value={minimumLevel}>
        {#each levels as level (level)}
          <option value={level}>{level.toUpperCase()}</option>
        {/each}
      </select>
      <button type="button" class={`rounded-md px-2 py-1 text-[11px] ${autoScroll ? 'bg-blue/15 text-blue' : 'bg-text/5 text-subtext1'}`} aria-pressed={autoScroll} onclick={() => (autoScroll = !autoScroll)}>Auto</button>
      <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1 hover:bg-text/10" onclick={() => void copy()}>{copied ? 'Copied' : 'Copy'}</button>
      <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1 hover:bg-text/10" onclick={onclear}>Clear</button>
    </div>
    {/if}
  </div>
  {#if activeTab === 'log'}
  <div bind:this={outputElement} class="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs leading-5" aria-live="polite" aria-relevant="additions text">
    {#if filteredLines.length === 0}
      <p class="text-subtext0">No output</p>
    {:else}
      {#each filteredLines as line (line.id)}
        <div class={line.level === 'critical' || line.level === 'error' ? 'text-red' : line.level === 'warning' ? 'text-yellow' : line.level === 'debug' ? 'text-subtext0' : 'text-subtext1'}>
          <span class="mr-2 select-none text-[10px] uppercase opacity-60">{line.level}</span>{line.message}
        </div>
      {/each}
    {/if}
  </div>
  {:else}
  <textarea class="min-h-0 flex-1 resize-none bg-transparent p-3 font-mono text-xs leading-5 text-text outline-none" aria-label={`${label} note`} placeholder="Note for this profile" bind:value={note}></textarea>
  {/if}
</section>
