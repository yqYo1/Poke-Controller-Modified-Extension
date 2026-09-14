<script lang="ts">
  import type { ScriptUiAction, ScriptUiActionResult } from '../actions';
  import type { RuntimeView } from '../runtime';
  import ScriptDialog from './ScriptDialog.svelte';
  import ScriptTkWindow from './ScriptTkWindow.svelte';

  interface ScriptUiActions {
    scriptUiAction(request: ScriptUiAction): Promise<ScriptUiActionResult>;
  }

  interface Props {
    actions: ScriptUiActions;
    view: RuntimeView;
  }

  let { actions, view }: Props = $props();

  const generation = $derived(view.scriptUi.generation);
  const dialogPosition = $derived.by(() => {
    const value = view.settings?.values['ui.dialog_button_position'];
    return value === 'top' || value === 'both' ? value : 'bottom';
  });

  async function invoke(action: ScriptUiAction): Promise<void> {
    await actions.scriptUiAction(action);
  }
</script>

{#if generation !== null}
  {#if view.scriptUi.dialogs.length > 0}
    <div class="pointer-events-none fixed inset-0 z-50 flex flex-col items-center justify-center gap-4 overflow-auto bg-black/55 p-4 backdrop-blur-sm">
      {#each view.scriptUi.dialogs as dialog (dialog.id)}
        <ScriptDialog {dialog} {generation} onaction={invoke} position={dialogPosition} />
      {/each}
    </div>
  {/if}

  {#if view.scriptUi.tk_windows.length > 0}
    <div class="pointer-events-none fixed top-16 right-4 z-40 flex max-h-[calc(100vh-5rem)] flex-col gap-3 overflow-auto p-1">
      {#each view.scriptUi.tk_windows as window (window.id)}
        <ScriptTkWindow {generation} onaction={invoke} {window} />
      {/each}
    </div>
  {/if}

  {#if view.scriptUi.popup_images.length > 0}
    <div class="pointer-events-none fixed inset-0 z-[60] flex flex-wrap items-center justify-center gap-4 overflow-auto bg-black/65 p-4">
      {#each view.scriptUi.popup_images as popup (popup.id)}
        <figure class="pointer-events-auto max-w-[90vw] overflow-hidden rounded-xl border border-white/15 bg-ink-900 shadow-2xl">
          <figcaption class="flex items-center justify-between gap-4 border-b border-white/10 px-3 py-2 text-sm text-white"><span>{popup.title}</span><button type="button" class="rounded px-2 text-slate-400 hover:bg-white/10 hover:text-white" aria-label="Close image" onclick={() => void invoke({ action: 'popup_closed', generation, popup_id: popup.id })}>×</button></figcaption>
          <img class="max-h-[80vh] max-w-[90vw] object-contain" src={`data:${popup.content_type};base64,${popup.encoded_base64}`} alt={popup.title} />
        </figure>
      {/each}
    </div>
  {/if}
{/if}
