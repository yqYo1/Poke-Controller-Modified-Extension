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
  let container = $state<HTMLElement>();
  let dragRatio = $state<number | null>(null);
  // Preserve the historical meaning of ui.output_split_ratio: it controls the
  // vertical relative heights of Output #1 and Output #2 when both are visible.
  // 10 + 0.8 * ratio maps 0..100 to a 10%..90% share for Output #1.
  const effectiveRatio = $derived(dragRatio ?? splitRatio);
  const output1Percent = $derived(10 + 0.8 * effectiveRatio);
  const output2Percent = $derived(100 - output1Percent);
  const showBoth = $derived(showOutput1 && showOutput2);
  const rows = $derived(
    showBoth
      ? `minmax(8rem, ${String(output1Percent)}fr) 6px minmax(8rem, ${String(output2Percent)}fr)`
      : 'minmax(8rem, 1fr)'
  );

  function ratioAt(clientY: number): number {
    const bounds = container?.getBoundingClientRect();
    if (bounds === undefined || bounds.height <= 0) return effectiveRatio;
    const percent = ((clientY - bounds.top) / bounds.height) * 100;
    return Math.min(100, Math.max(0, (percent - 10) / 0.8));
  }

  function startDrag(event: PointerEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    dragRatio = ratioAt(event.clientY);
  }

  function moveDrag(event: PointerEvent): void {
    if (dragRatio === null) return;
    event.preventDefault();
    dragRatio = ratioAt(event.clientY);
  }

  function endDrag(): void {
    if (dragRatio === null) return;
    const value = Math.round(dragRatio);
    dragRatio = null;
    void runtime.writeSettings({ 'ui.output_split_ratio': value });
  }

  function resetSplit(): void {
    dragRatio = null;
    void runtime.writeSettings({ 'ui.output_split_ratio': 50 });
  }
</script>

<div
  bind:this={container}
  class="grid h-full min-h-0 grid-cols-1 gap-3"
  style={`grid-template-rows: ${rows}; --output-one: ${String(output1Percent)}%;`}
>
  {#if showOutput1}
    <OutputPanel label="Output #1" lines={view.output1} onclear={() => runtime.clearOutput(1)} storageKey="pokecon.output1" />
  {/if}
  {#if showBoth}
    <div
      role="separator"
      aria-orientation="horizontal"
      aria-label="Output split"
      aria-valuemin="0"
      aria-valuemax="100"
      aria-valuenow={Math.round(effectiveRatio)}
      class="cursor-row-resize rounded-full bg-text/10 hover:bg-blue/40"
      onpointerdown={startDrag}
      onpointermove={moveDrag}
      onpointerup={endDrag}
      onpointercancel={endDrag}
      ondblclick={resetSplit}
    ></div>
  {/if}
  {#if showOutput2}
    <OutputPanel label="Output #2" lines={view.output2} onclear={() => runtime.clearOutput(2)} storageKey="pokecon.output2" />
  {/if}
</div>
