<script lang="ts">
  import type { ApplicationRuntime, RuntimeView } from '../runtime';
  import OutputPanel from './OutputPanel.svelte';

  interface Props {
    runtime: ApplicationRuntime;
    showOutput1: boolean;
    showOutput2: boolean;
    splitRatio: number;
    view: RuntimeView;
  }

  let { runtime, showOutput1, showOutput2, splitRatio, view }: Props = $props();
  const output1Percent = $derived(10 + 0.8 * splitRatio);
  const showBoth = $derived(showOutput1 && showOutput2);
</script>

<div
  class={`grid min-h-0 gap-3 ${showBoth ? 'xl:grid-cols-[var(--output-one)_minmax(0,1fr)]' : 'grid-cols-1'}`}
  style={`--output-one: ${String(output1Percent)}%`}
>
  {#if showOutput1}
    <OutputPanel label="Output #1" lines={view.output1} onclear={() => runtime.clearOutput(1)} storageKey="pokecon.output1" />
  {/if}
  {#if showOutput2}
    <OutputPanel label="Output #2" lines={view.output2} onclear={() => runtime.clearOutput(2)} storageKey="pokecon.output2" />
  {/if}
</div>
