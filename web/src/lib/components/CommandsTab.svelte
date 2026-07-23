<script lang="ts">
  import Fuse from 'fuse.js';

  import type { CommandControlRequest, OperationResult } from '../actions';
  import type { SettingsWriteValues } from '../api';
  import type { components } from '../api/openapi';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  type CommandDisplayItem = components['schemas']['CommandDisplayItem'];
  type CommandInfo = components['schemas']['CommandInfo'];
  type CommandIdentity = components['schemas']['CommandIdentity'];
  type ShortcutIndex = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10;
  type ShortcutSetting = `shortcuts.button_${ShortcutIndex}`;
  type Subtab = 'python' | 'mcu' | 'shortcuts';

  interface CommandActions {
    controlCommand(request: CommandControlRequest): Promise<OperationResult>;
    reloadCommands(): Promise<OperationResult>;
  }

  interface Props {
    actions: CommandActions;
    runtime: ApplicationRuntime;
    view: RuntimeView;
  }

  const shortcutIndexes: readonly ShortcutIndex[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
  const subtabs: readonly { readonly id: Subtab; readonly label: string }[] = [
    { id: 'python', label: 'Python Command' },
    { id: 'mcu', label: 'Mcu Command' },
    { id: 'shortcuts', label: 'Shortcut' }
  ];

  let { actions, runtime, view }: Props = $props();
  let activeSubtab = $state<Subtab>('python');
  let assigningSlot = $state<ShortcutIndex | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let search = $state('');
  let selectedIdentity = $state<string | null>(null);
  let selectedTag = $state('-');

  const backendState = $derived(view.state);
  const settings = $derived(view.settings?.values);
  const orderedTags = $derived.by(() => [
    '-',
    ...[...(backendState?.tags ?? [])].sort((left, right) => {
      const leftAutomatic = left.startsWith('@');
      const rightAutomatic = right.startsWith('@');
      return leftAutomatic === rightAutomatic
        ? left.localeCompare(right)
        : leftAutomatic
          ? 1
          : -1;
    })
  ]);
  const baseItems = $derived(
    backendState?.command_display_lists[selectedTag] ??
      backendState?.command_display_lists['-'] ??
      []
  );
  const selectedCommand = $derived(
    backendState?.command_candidates.find(
      (command) => identity(command) === selectedIdentity
    ) ?? null
  );
  const visibleItems = $derived.by<readonly CommandDisplayItem[]>(() => {
    const filtered = filterForSubtab(baseItems, activeSubtab);
    const query = search.trim();
    if (query.length === 0) return filtered;
    const commands = filtered.filter(
      (item): item is Extract<CommandDisplayItem, { kind: 'command' }> =>
        item.kind === 'command'
    );
    return new Fuse(commands, {
      includeScore: true,
      keys: ['command.name', 'command.module_path', 'command.class_name', 'command.tags'],
      threshold: 0.4
    })
      .search(query)
      .map((result) => result.item);
  });

  $effect(() => {
    if (!orderedTags.includes(selectedTag)) selectedTag = '-';
  });

  function identity(command: CommandInfo): string {
    return `${command.module_path}\u0000${command.class_name}`;
  }

  function commandIdentity(command: CommandInfo): CommandIdentity {
    return { class_name: command.class_name, module_path: command.module_path };
  }

  function displayCommand(item: CommandDisplayItem): CommandInfo | null {
    return item.kind === 'command' ? item.command : null;
  }

  function separatorLabel(item: CommandDisplayItem): string {
    return item.kind === 'separator' ? (item.label ?? '') : '';
  }

  function detectedMcu(command: CommandInfo): boolean {
    const searchable = `${command.module_path}.${command.class_name}`.toLowerCase();
    return searchable.includes('mcu');
  }

  function filterForSubtab(
    items: readonly CommandDisplayItem[],
    subtab: Subtab
  ): readonly CommandDisplayItem[] {
    if (subtab === 'shortcuts') return [];
    const retained = items.filter(
      (item) =>
        item.kind === 'separator' ||
        (subtab === 'mcu' ? detectedMcu(item.command) : !detectedMcu(item.command))
    );
    const normalized: CommandDisplayItem[] = [];
    for (const item of retained) {
      if (item.kind === 'separator') {
        if (normalized.length === 0 || normalized.at(-1)?.kind === 'separator') continue;
      }
      normalized.push(item);
    }
    if (normalized.at(-1)?.kind === 'separator') normalized.pop();
    return normalized;
  }

  function selectCommand(command: CommandInfo): void {
    selectedIdentity = identity(command);
  }

  function shortcutSetting(index: ShortcutIndex): ShortcutSetting {
    return `shortcuts.button_${String(index)}` as ShortcutSetting;
  }

  function shortcutModule(index: ShortcutIndex): string {
    return settings?.[shortcutSetting(index)] ?? '';
  }

  function shortcutCommand(index: ShortcutIndex): CommandInfo | null {
    const modulePath = shortcutModule(index);
    return (
      backendState?.command_candidates.find((command) => command.module_path === modulePath) ?? null
    );
  }

  function shortcutLabel(index: ShortcutIndex): string {
    const assigned = shortcutCommand(index);
    const configured = shortcutModule(index);
    return assigned?.name ?? (configured.length === 0 ? 'unassigned' : configured);
  }

  async function write(values: SettingsWriteValues): Promise<void> {
    busy = true;
    error = null;
    notice = null;
    try {
      await runtime.writeSettings(values);
    } catch (reason: unknown) {
      error = reason instanceof Error ? reason.message : 'Command setting update failed';
    } finally {
      busy = false;
    }
  }

  async function changeMatchMode(event: Event): Promise<void> {
    const mode = (event.currentTarget as HTMLSelectElement).value as SettingsWriteValues['commands.tag_match_mode'];
    await write({ 'commands.tag_match_mode': mode });
  }

  async function control(request: CommandControlRequest): Promise<void> {
    busy = true;
    error = null;
    notice = null;
    try {
      const result = await actions.controlCommand(request);
      notice = result.changed ? 'Command state updated.' : 'Command state was already current.';
    } catch (reason: unknown) {
      error = reason instanceof Error ? reason.message : 'Command operation failed';
    } finally {
      busy = false;
    }
  }

  async function start(command: CommandInfo | null = selectedCommand): Promise<void> {
    if (command === null) {
      error = 'Select a command first.';
      return;
    }
    selectCommand(command);
    await control({ action: 'start', command: commandIdentity(command) });
  }

  async function reload(): Promise<void> {
    busy = true;
    error = null;
    notice = null;
    try {
      const result = await actions.reloadCommands();
      notice = result.changed ? 'Command list reloaded.' : 'Command list did not change.';
    } catch (reason: unknown) {
      error = reason instanceof Error ? reason.message : 'Command reload failed';
    } finally {
      busy = false;
    }
  }

  function beginShortcutAssignment(event: MouseEvent, index: ShortcutIndex): void {
    event.preventDefault();
    if (event.shiftKey) {
      void clearShortcut(index);
      return;
    }
    assigningSlot = index;
  }

  function contextShortcut(event: MouseEvent, index: ShortcutIndex): void {
    event.preventDefault();
    void clearShortcut(index);
  }

  async function clearShortcut(index: ShortcutIndex): Promise<void> {
    const values: SettingsWriteValues = {};
    values[shortcutSetting(index)] = '';
    await write(values);
    if (assigningSlot === index) assigningSlot = null;
  }

  async function assignShortcut(command: CommandInfo): Promise<void> {
    if (assigningSlot === null) return;
    const index = assigningSlot;
    const values: SettingsWriteValues = {};
    values[shortcutSetting(index)] = command.module_path;
    await write(values);
    assigningSlot = null;
    selectCommand(command);
  }

  function subtabKeydown(event: KeyboardEvent): void {
    const current = subtabs.findIndex((tab) => tab.id === activeSubtab);
    const next =
      event.key === 'ArrowRight'
        ? (current + 1) % subtabs.length
        : event.key === 'ArrowLeft'
          ? (current - 1 + subtabs.length) % subtabs.length
          : event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? subtabs.length - 1
              : null;
    if (next === null) return;
    event.preventDefault();
    const tab = subtabs[next];
    if (tab === undefined) return;
    activeSubtab = tab.id;
    const buttons = (event.currentTarget as HTMLElement)
      .closest('[role="tablist"]')
      ?.querySelectorAll<HTMLElement>('[role="tab"]');
    buttons?.[next]?.focus();
  }
</script>

<div class="space-y-4">
  <div>
    <p class="text-xs font-semibold tracking-[0.2em] text-lime-300 uppercase">Commands</p>
    <div class="mt-2 flex flex-wrap items-center justify-between gap-3">
      <h2 class="text-2xl font-semibold text-white">コマンドワークスペース</h2>
      <span class={`rounded-full px-3 py-1 text-xs ${backendState?.command_state === 'running' ? 'bg-lime-300/15 text-lime-300' : backendState?.command_state === 'paused' ? 'bg-amber-300/15 text-amber-200' : backendState?.command_state === 'error' ? 'bg-red-400/10 text-red-200' : 'bg-white/5 text-slate-400'}`}>
        {backendState?.command_state ?? 'stopped'}
      </span>
    </div>
  </div>

  {#if error !== null}
    <div class="rounded-lg border border-red-400/20 bg-red-400/10 px-3 py-2 text-sm text-red-200" role="alert">{error}</div>
  {:else if notice !== null}
    <div class="rounded-lg border border-lime-300/20 bg-lime-300/10 px-3 py-2 text-sm text-lime-200" role="status">{notice}</div>
  {/if}

  <div class="flex gap-1 overflow-x-auto border-b border-white/10" role="tablist" aria-label="Command views">
    {#each subtabs as tab (tab.id)}
      <button
        id={`command-tab-${tab.id}`}
        type="button"
        role="tab"
        aria-selected={activeSubtab === tab.id}
        aria-controls={`command-panel-${tab.id}`}
        tabindex={activeSubtab === tab.id ? 0 : -1}
        class={`border-b-2 px-3 py-2 text-sm ${activeSubtab === tab.id ? 'border-cyan-300 text-cyan-200' : 'border-transparent text-slate-400'}`}
        onclick={() => (activeSubtab = tab.id)}
        onkeydown={subtabKeydown}
      >{tab.label}</button>
    {/each}
  </div>

  {#if activeSubtab === 'shortcuts'}
    <div id="command-panel-shortcuts" role="tabpanel" aria-labelledby="command-tab-shortcuts" class="space-y-4">
      <div class="grid gap-2 sm:grid-cols-2 xl:grid-cols-5">
        {#each shortcutIndexes as index (index)}
          {@const assigned = shortcutCommand(index)}
          <div class={`rounded-xl border p-2 ${assigningSlot === index ? 'border-yellow-300/50 bg-yellow-300/10' : 'border-white/10 bg-black/15'}`}>
            <button
              type="button"
              class="min-h-14 w-full rounded-lg px-2 py-2 text-left text-sm text-slate-200 hover:bg-white/5"
              aria-label={`Shortcut ${String(index)}: ${shortcutLabel(index)}`}
              oncontextmenu={(event) => contextShortcut(event, index)}
              onclick={(event) => beginShortcutAssignment(event, index)}
            >
              <span class="block text-[10px] text-slate-500">#{String(index)}</span>
              <span class="line-clamp-2">{shortcutLabel(index)}</span>
            </button>
            {#if assigned !== null}
              <button type="button" class="mt-1 w-full rounded-md bg-lime-300/10 px-2 py-1 text-xs text-lime-200" disabled={busy} onclick={() => void start(assigned)}>Run</button>
            {/if}
          </div>
        {/each}
      </div>
      <p class="text-xs text-slate-500">Click a slot, then choose a command. Shift+click or right-click clears it.</p>
      {#if assigningSlot !== null}
        <section class="rounded-xl border border-yellow-300/20 bg-yellow-300/5 p-3" aria-label={`Assign shortcut ${String(assigningSlot)}`}>
          <div class="flex items-center justify-between gap-3">
            <h3 class="text-sm font-medium text-yellow-100">Choose command for shortcut #{String(assigningSlot)}</h3>
            <button type="button" class="text-xs text-slate-400" onclick={() => (assigningSlot = null)}>Cancel</button>
          </div>
          <div class="mt-3 grid gap-2 sm:grid-cols-2">
            {#each backendState?.command_candidates ?? [] as command (identity(command))}
              <button type="button" class="rounded-lg bg-black/20 px-3 py-2 text-left text-sm text-slate-200 hover:bg-white/5" aria-label={`Assign ${command.name} to shortcut ${String(assigningSlot)}`} onclick={() => void assignShortcut(command)}>
                <span class="block font-medium">{command.name}</span><span class="block truncate text-xs text-slate-500">{command.module_path}</span>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    </div>
  {:else}
    <div id={`command-panel-${activeSubtab}`} role="tabpanel" aria-labelledby={`command-tab-${activeSubtab}`} class="space-y-3">
      <div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto]">
        <label>
          <span class="sr-only">Search commands</span>
          <input class="w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" type="search" bind:value={search} placeholder="Search commands" />
        </label>
        <label>
          <span class="sr-only">Command tag</span>
          <select class="w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" bind:value={selectedTag}>
            {#each orderedTags as tag (tag)}<option value={tag}>{tag === '-' ? 'All tags' : tag}</option>{/each}
          </select>
        </label>
        <label>
          <span class="sr-only">Tag match mode</span>
          <select class="w-full rounded-lg border border-white/10 bg-ink-800 px-3 py-2 text-sm text-white" value={settings?.['commands.tag_match_mode'] ?? 'exact'} onchange={(event) => void changeMatchMode(event)}>
            <option value="exact">Exact</option><option value="partial">Partial</option><option value="prefix">Prefix</option><option value="suffix">Suffix</option>
          </select>
        </label>
      </div>

      <div class="max-h-[26rem] overflow-auto rounded-xl border border-white/10 bg-black/20 p-2" aria-busy={backendState?.command_display_cache_loading ?? false}>
        {#if backendState?.command_display_cache_loading && visibleItems.length === 0}
          <p class="p-4 text-sm text-slate-500">Building command display cache…</p>
        {:else if visibleItems.length === 0}
          <p class="p-4 text-sm text-slate-500">No matching commands</p>
        {:else}
          {#each visibleItems as item, index (`${item.kind}-${String(index)}`)}
            {@const command = displayCommand(item)}
            {#if command === null}
              <div class="my-2 flex items-center gap-2 text-[10px] tracking-[0.14em] text-slate-600 uppercase" role="separator"><span class="h-px flex-1 bg-white/10"></span>{separatorLabel(item)}<span class="h-px flex-1 bg-white/10"></span></div>
            {:else}
              <button
                type="button"
                class={`mb-1 w-full rounded-lg border px-3 py-2 text-left ${selectedIdentity === identity(command) ? 'border-cyan-300/40 bg-cyan-300/10' : 'border-transparent bg-white/[0.025] hover:bg-white/5'}`}
                aria-label={`Select ${command.name}`}
                onclick={() => selectCommand(command)}
                ondblclick={() => void start(command)}
              >
                <span class="flex items-start justify-between gap-3"><span class="font-medium text-slate-200">{command.name}</span><span class="text-[10px] text-slate-500">{command.class_name}</span></span>
                <span class="mt-1 block truncate text-xs text-slate-500">{command.module_path}</span>
                {#if command.tags.length > 0}<span class="mt-2 flex flex-wrap gap-1">{#each command.tags as tag (tag)}<span class="rounded bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-400">{tag}</span>{/each}</span>{/if}
              </button>
            {/if}
          {/each}
        {/if}
      </div>
    </div>
  {/if}

  <div class="flex flex-wrap items-center gap-2 rounded-xl border border-white/10 bg-black/20 p-3">
    <button type="button" class="rounded-lg bg-lime-300/15 px-3 py-2 text-sm font-medium text-lime-200 disabled:opacity-40" disabled={busy || selectedCommand === null || backendState?.command_state === 'running' || backendState?.command_state === 'paused'} onclick={() => void start()}>Start</button>
    <button type="button" class="rounded-lg bg-amber-300/10 px-3 py-2 text-sm text-amber-200 disabled:opacity-40" disabled={busy || backendState?.command_state !== 'running'} onclick={() => void control({ action: 'pause' })}>Pause</button>
    <button type="button" class="rounded-lg bg-cyan-300/10 px-3 py-2 text-sm text-cyan-200 disabled:opacity-40" disabled={busy || backendState?.command_state !== 'paused'} onclick={() => void control({ action: 'resume' })}>Resume</button>
    <button type="button" class="rounded-lg bg-red-400/10 px-3 py-2 text-sm text-red-200 disabled:opacity-40" disabled={busy || (backendState?.command_state !== 'running' && backendState?.command_state !== 'paused')} onclick={() => void control({ action: 'stop' })}>Stop</button>
    <button type="button" class="rounded-lg bg-white/5 px-3 py-2 text-sm text-slate-300 disabled:opacity-40" disabled={busy} onclick={() => void reload()}>Reload</button>
    <span class="ml-auto text-xs text-slate-500">{selectedCommand?.name ?? backendState?.current_command ?? 'No command selected'}</span>
  </div>
</div>
