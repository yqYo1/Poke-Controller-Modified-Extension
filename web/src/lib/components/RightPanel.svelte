<script lang="ts">
  import type { ApplicationRuntime, RuntimeView } from '../runtime';
  import ControllerPanel from './ControllerPanel.svelte';
  import OutputGroup from './OutputGroup.svelte';

  interface Props {
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  let { runtime, view }: Props = $props();
  const mode = $derived(view.settings?.values['ui.widget_mode'] ?? 'all');
  const controllerPosition = $derived(
    view.settings?.values['ui.controller_position'] ?? 'top'
  );
  const splitRatio = $derived(view.settings?.values['ui.output_split_ratio'] ?? 20);
  const showController = $derived(
    ['all', 'output_1_controller', 'output_2_controller', 'controller'].includes(mode)
  );
  const showOutput1 = $derived(
    ['all', 'outputs', 'output_1_controller', 'output_1'].includes(mode)
  );
  const showOutput2 = $derived(
    ['all', 'outputs', 'output_2_controller', 'output_2'].includes(mode)
  );
</script>

<aside class="flex min-h-0 flex-col gap-3" aria-label="Controller and output panel">
  {#if controllerPosition === 'top'}
    {#if showController}<ControllerPanel compact runtime={runtime} view={view} />{/if}
    <OutputGroup {runtime} {showOutput1} {showOutput2} {splitRatio} {view} />
  {:else}
    <OutputGroup {runtime} {showOutput1} {showOutput2} {splitRatio} {view} />
    {#if showController}<ControllerPanel compact runtime={runtime} view={view} />{/if}
  {/if}
</aside>
