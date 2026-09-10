<script lang="ts">
  import { onMount } from 'svelte';

  import type { OutputLine } from '../runtime';

  type LogLevel = OutputLine['level'];

  interface Props {
    label: string;
    lines: readonly OutputLine[];
    onclear: () => void;
    storageKey: string;
  }

  const ranks: Readonly<Record<LogLevel, number>> = {
    critical: 4,
    debug: 0,
    error: 3,
    info: 1,
    warning: 2
  };
  const levels: readonly LogLevel[] = ['debug', 'info', 'warning', 'error', 'critical'];

  let { label, lines, onclear, storageKey }: Props = $props();
  let autoScroll = $state(true);
  let copied = $state(false);
  let minimumLevel = $state<LogLevel>('debug');
  let outputElement: HTMLDivElement | undefined;
  let storageReady = false;

  const filteredLines = $derived(
    lines.filter((line) => ranks[line.level] >= ranks[minimumLevel])
  );

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

<section class="flex min-h-0 flex-col overflow-hidden rounded-xl border border-white/10 bg-black/25" aria-label={label}>
  <div class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-white/10 px-3 py-2">
    <h3 class="text-xs font-semibold tracking-[0.14em] text-slate-200 uppercase">{label}</h3>
    <div class="flex items-center gap-1">
      <label class="sr-only" for={`${storageKey}-level`}>Minimum log level</label>
      <select id={`${storageKey}-level`} class="rounded-md border border-white/10 bg-ink-800 px-2 py-1 text-[11px] text-slate-300" bind:value={minimumLevel}>
        {#each levels as level (level)}
          <option value={level}>{level.toUpperCase()}</option>
        {/each}
      </select>
      <button type="button" class={`rounded-md px-2 py-1 text-[11px] ${autoScroll ? 'bg-cyan-300/15 text-cyan-200' : 'bg-white/5 text-slate-400'}`} aria-pressed={autoScroll} onclick={() => (autoScroll = !autoScroll)}>Auto</button>
      <button type="button" class="rounded-md bg-white/5 px-2 py-1 text-[11px] text-slate-300 hover:bg-white/10" onclick={() => void copy()}>{copied ? 'Copied' : 'Copy'}</button>
      <button type="button" class="rounded-md bg-white/5 px-2 py-1 text-[11px] text-slate-300 hover:bg-white/10" onclick={onclear}>Clear</button>
    </div>
  </div>
  <div bind:this={outputElement} class="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs leading-5" aria-live="polite" aria-relevant="additions text">
    {#if filteredLines.length === 0}
      <p class="text-slate-600">No output</p>
    {:else}
      {#each filteredLines as line (line.id)}
        <div class={line.level === 'critical' || line.level === 'error' ? 'text-red-300' : line.level === 'warning' ? 'text-amber-300' : line.level === 'debug' ? 'text-slate-500' : 'text-slate-300'}>
          <span class="mr-2 select-none text-[10px] uppercase opacity-60">{line.level}</span>{line.message}
        </div>
      {/each}
    {/if}
  </div>
</section>
