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
  // Preserve the historical meaning of ui.output_split_ratio: it controls the
  // vertical relative heights of Output #1 and Output #2 when both are visible.
  // 10 + 0.8 * ratio maps 0..100 to a 10%..90% share for Output #1.
  const output1Percent = $derived(10 + 0.8 * splitRatio);
  const output2Percent = $derived(100 - output1Percent);
  const showBoth = $derived(showOutput1 && showOutput2);
  const rows = $derived(
    showBoth
      ? `minmax(8rem, ${String(output1Percent)}fr) minmax(8rem, ${String(output2Percent)}fr)`
      : 'minmax(8rem, 1fr)'
  );
</script>

<div
  class="grid h-full min-h-0 grid-cols-1 gap-3"
  style={`grid-template-rows: ${rows}; --output-one: ${String(output1Percent)}%;`}
>
  {#if showOutput1}
    <OutputPanel label="Output #1" lines={view.output1} onclear={() => runtime.clearOutput(1)} storageKey="pokecon.output1" />
  {/if}
  {#if showOutput2}
    <OutputPanel label="Output #2" lines={view.output2} onclear={() => runtime.clearOutput(2)} storageKey="pokecon.output2" />
  {/if}
</div>
