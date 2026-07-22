<script lang="ts">
  import type { ScriptUiAction } from '../actions';
  import type { components } from '../generated/api';

  type Dialog = components['schemas']['ScriptDialog'];
  type DialogValue = components['schemas']['ScriptDialogValue'];
  type Widget = components['schemas']['ScriptDialogWidget'];

  interface Props {
    dialog: Dialog;
    generation: string;
    onaction: (action: ScriptUiAction) => Promise<void>;
    position: 'bottom' | 'top' | 'both';
  }

  let { dialog, generation, onaction, position }: Props = $props();
  let busy = $state(false);
  let error = $state<string | null>(null);
  let initializedDialog = $state('');
  let values = $state<DialogValue[]>([]);

  $effect(() => {
    if (initializedDialog === dialog.id) return;
    initializedDialog = dialog.id;
    values = dialog.widgets.map((widget) => structuredClone(widget.value));
  });

  const columns = $derived.by(() => {
    const result: { index: number; widget: Widget }[][] = [[]];
    for (const [index, widget] of dialog.widgets.entries()) {
      if (widget.kind === 'next') {
        result.push([]);
      } else {
        result.at(-1)?.push({ index, widget });
      }
    }
    return result;
  });

  function message(reason: unknown): string {
    return reason instanceof Error ? reason.message : 'Dialog operation failed';
  }

  function optionIndex(widget: Widget, value: DialogValue): number {
    return widget.options.findIndex((option) => JSON.stringify(option) === JSON.stringify(value));
  }

  function textValue(value: DialogValue): string {
    return value.type === 'string' ? value.value : '';
  }

  function boolValue(value: DialogValue): boolean {
    return value.type === 'bool' && value.value;
  }

  function numberValue(value: DialogValue): number {
    return value.type === 'integer' || value.type === 'float' ? value.value : 0;
  }

  async function invoke(action: ScriptUiAction): Promise<void> {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await onaction(action);
    } catch (reason: unknown) {
      error = message(reason);
      busy = false;
    }
  }

  function confirm(): void {
    void invoke({
      action: 'dialog_confirm',
      dialog_id: dialog.id,
      generation,
      values
    });
  }

  function abort(reason: 'close' | 'escape'): void {
    void invoke({
      action: 'dialog_abort',
      dialog_id: dialog.id,
      generation,
      reason
    });
  }

  function keydown(event: KeyboardEvent): void {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    abort('escape');
  }
</script>

<dialog
  open
  class="pointer-events-auto relative m-0 w-[min(94vw,58rem)] rounded-2xl border border-white/15 bg-ink-900 p-4 text-left shadow-2xl shadow-black/70"
  aria-modal="true"
  aria-labelledby={`script-dialog-${dialog.id}`}
  onkeydown={keydown}
>
  <header class="flex items-start justify-between gap-4 border-b border-white/10 pb-3">
    <div>
      <h2 id={`script-dialog-${dialog.id}`} class="text-lg font-semibold text-white">{dialog.title}</h2>
      {#if dialog.description !== null}<p class="mt-1 whitespace-pre-wrap text-sm text-slate-400">{dialog.description}</p>{/if}
    </div>
    <button type="button" class="rounded-md px-2 py-1 text-slate-400 hover:bg-white/10 hover:text-white" aria-label="Close dialog and stop script" disabled={busy} onclick={() => abort('close')}>×</button>
  </header>

  {#snippet buttons()}
    <div class="flex justify-end gap-2 border-white/10 py-3">
      <button type="button" class="rounded-lg bg-cyan-300/15 px-5 py-2 text-sm font-semibold text-cyan-100 hover:bg-cyan-300/25 disabled:opacity-50" disabled={busy} onclick={confirm}>OK</button>
    </div>
  {/snippet}

  {#if position === 'top' || position === 'both'}{@render buttons()}{/if}

  <div class="grid gap-5 overflow-auto py-3" style={`grid-template-columns: repeat(${String(Math.max(1, columns.length))}, minmax(0, 1fr));`}>
    {#each columns as column, columnIndex (`${dialog.id}-${String(columnIndex)}`)}
      <div class="space-y-3">
        {#each column as item (item.index)}
          {@const widget = item.widget}
          {@const value = values[item.index] ?? widget.value}
          {#if widget.kind === 'entry'}
            <label class="block"><span class="text-xs text-slate-400">{widget.label}</span><input class="mt-1 w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" value={textValue(value)} disabled={busy} oninput={(event) => (values[item.index] = { type: 'string', value: event.currentTarget.value })} /></label>
          {:else if widget.kind === 'check'}
            <label class="flex items-center gap-2 rounded-lg border border-white/10 px-3 py-2 text-sm text-slate-200"><input type="checkbox" checked={boolValue(value)} disabled={busy} onchange={(event) => (values[item.index] = { type: 'bool', value: event.currentTarget.checked })} />{widget.label}</label>
          {:else if widget.kind === 'combo'}
            <label class="block"><span class="text-xs text-slate-400">{widget.label}</span><select class="mt-1 w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" value={optionIndex(widget, value)} disabled={busy} onchange={(event) => { const option = widget.options[Number(event.currentTarget.value)]; if (option !== undefined) values[item.index] = structuredClone(option); }}>{#each widget.options as option, optionNumber (optionNumber)}<option value={optionNumber}>{option.type === 'none' ? '' : String(option.value)}</option>{/each}</select></label>
          {:else if widget.kind === 'radio'}
            <fieldset class="rounded-lg border border-white/10 p-3"><legend class="px-1 text-xs text-slate-400">{widget.label}</legend>{#each widget.options as option, optionNumber (optionNumber)}<label class="mr-3 inline-flex items-center gap-1 text-sm text-slate-200"><input type="radio" name={`dialog-${dialog.id}-${String(item.index)}`} checked={optionIndex(widget, value) === optionNumber} disabled={busy} onchange={() => (values[item.index] = structuredClone(option))} />{option.type === 'none' ? '' : String(option.value)}</label>{/each}</fieldset>
          {:else if widget.kind === 'spin'}
            <label class="block"><span class="text-xs text-slate-400">{widget.label}</span><input class="mt-1 w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" type="number" min={widget.minimum ?? undefined} max={widget.maximum ?? undefined} step="1" value={numberValue(value)} disabled={busy} onchange={(event) => (values[item.index] = { type: 'integer', value: Number.parseInt(event.currentTarget.value, 10) })} /></label>
          {:else if widget.kind === 'scale'}
            <label class="block"><span class="flex justify-between text-xs text-slate-400"><span>{widget.label}</span><output>{numberValue(value).toFixed(widget.precision ?? 2)}</output></span><input class="mt-2 w-full accent-cyan-300" type="range" min={widget.minimum ?? 0} max={widget.maximum ?? 100} step={10 ** -(widget.precision ?? 2)} value={numberValue(value)} disabled={busy} oninput={(event) => (values[item.index] = { type: 'float', value: Number(event.currentTarget.value) })} /></label>
          {/if}
        {/each}
      </div>
    {/each}
  </div>

  {#if position === 'bottom' || position === 'both'}{@render buttons()}{/if}
  {#if error !== null}<p class="mt-2 text-sm text-red-300" role="alert">{error}</p>{/if}
</dialog>
