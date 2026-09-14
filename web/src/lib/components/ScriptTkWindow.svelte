<script lang="ts">
  import type { ScriptUiAction } from '../actions';
  import type { components } from '../api/openapi';

  type TkWindow = components['schemas']['ScriptTkWindow'];

  interface Props {
    generation: string;
    onaction: (action: ScriptUiAction) => Promise<void>;
    window: TkWindow;
  }

  let { generation, onaction, window }: Props = $props();
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function invoke(action: ScriptUiAction): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await onaction(action);
    } catch (reason: unknown) {
      error = reason instanceof Error ? reason.message : 'Tk operation failed';
    } finally {
      busy = false;
    }
  }
</script>

<section class="pointer-events-auto w-80 overflow-hidden rounded-xl border border-white/15 bg-ink-900 shadow-2xl shadow-black/60" aria-label={window.title || 'Script window'}>
  <header class="flex items-center justify-between border-b border-white/10 bg-white/5 px-3 py-2">
    <h2 class="truncate text-sm font-semibold text-white">{window.title || 'Script window'}</h2>
    <button type="button" class="rounded px-2 text-slate-400 hover:bg-white/10 hover:text-white" aria-label="Close script window" disabled={busy} onclick={() => void invoke({ action: 'tk_window_closed', generation, window_id: window.id })}>×</button>
  </header>
  <div class="max-h-[70vh] overflow-auto p-3">
    {#each window.widgets as widget (widget.id)}
      <div style={`padding-block: ${String(Math.max(0, widget.pady ?? 0))}px`}>
        {#if widget.kind === 'scale'}
          <label class="block text-xs text-slate-300"><span class="flex justify-between"><span>{widget.label ?? ''}</span><output>{widget.value.toFixed(2)}</output></span><input class="mt-1 w-full accent-cyan-300" type="range" min={widget.from_value} max={widget.to_value} step="any" value={widget.value} disabled={busy} onchange={(event) => void invoke({ action: 'tk_scale_changed', generation, value: Number(event.currentTarget.value), widget_id: widget.id })} /></label>
        {:else if widget.kind === 'button'}
          <button type="button" class="w-full rounded-lg bg-cyan-300/15 px-3 py-2 text-sm text-cyan-100 hover:bg-cyan-300/25" disabled={busy} onclick={() => void invoke({ action: 'tk_button_invoked', generation, widget_id: widget.id })}>{widget.text}</button>
        {:else}
          <div class="rounded px-2 py-1 text-center text-sm text-slate-200" style:background-color={widget.background ?? 'transparent'}>{widget.text}</div>
        {/if}
      </div>
    {/each}
    {#if error !== null}<p class="mt-2 text-xs text-red-300" role="alert">{error}</p>{/if}
  </div>
</section>
