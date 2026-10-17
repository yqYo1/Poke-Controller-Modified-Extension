<script lang="ts">
  import Fuse from 'fuse.js';

  import type {
    CommandControlRequest,
    OperationResult,
    ScriptCreatorPreview,
    ScriptCreatorResult,
    ScriptCreatorSnapshot,
    ScriptCreatorSpec
  } from '../actions';
  import ScriptCreatorDialog from './ScriptCreatorDialog.svelte';
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
    createScript(request: ScriptCreatorSpec): Promise<ScriptCreatorResult>;
    previewScriptCreation(request: ScriptCreatorSpec): Promise<ScriptCreatorPreview>;
    reloadCommands(): Promise<OperationResult>;
    scriptCreatorSnapshot(): Promise<ScriptCreatorSnapshot>;
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
  let creatorOpen = $state(false);
  let activeSubtab = $state<Subtab>('python');
  let assigningSlot = $state<ShortcutIndex | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let search = $state('');
  let selectedIdentity = $state<string | null>(null);
  let selectedTag = $state('-');
  let commandMenu = $state<{ x: number; y: number; command: CommandInfo } | null>(null);
  let commandMenuCopied = $state(false);
  // List ordering, mirroring the upstream Command-column sorts. Name sorts
  // are frontend derivations over the immutable display list; the sorted view
  // drops separators, like the search view.
  type CommandSort =
    | 'default'
    | 'name_asc'
    | 'name_desc'
    | 'mtime_asc'
    | 'mtime_desc'
    | 'usage_desc'
    | 'usage_asc';
  let commandSort = $state<CommandSort>('default');

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
    if (query.length === 0) {
      if (commandSort === 'default') return filtered;
      const commands = filtered.filter(
        (item): item is Extract<CommandDisplayItem, { kind: 'command' }> =>
          item.kind === 'command'
      );
      if (commandSort === 'usage_desc' || commandSort === 'usage_asc') {
        const direction = commandSort === 'usage_desc' ? -1 : 1;
        return [...commands].sort((left, right) => {
          const byUsage =
            direction * ((left.command.usage_count ?? 0) - (right.command.usage_count ?? 0));
          if (byUsage !== 0) return byUsage;
          return left.command.name
            .toLowerCase()
            .localeCompare(right.command.name.toLowerCase());
        });
      }
      if (commandSort === 'mtime_asc' || commandSort === 'mtime_desc') {
        const direction = commandSort === 'mtime_desc' ? -1 : 1;
        return [...commands].sort((left, right) => {
          const byTime = direction * ((left.command.modified_at ?? 0) - (right.command.modified_at ?? 0));
          if (byTime !== 0) return byTime;
          return left.command.name
            .toLowerCase()
            .localeCompare(right.command.name.toLowerCase());
        });
      }
      const direction = commandSort === 'name_asc' ? 1 : -1;
      return [...commands].sort(
        (left, right) =>
          direction *
          left.command.name.toLowerCase().localeCompare(right.command.name.toLowerCase())
      );
    }
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

  // Right-click menu: shows the command's source path and copies it. The
  // real-file open is Tauri-only (Web stays path/copy operations).
  function openCommandMenu(event: MouseEvent, command: CommandInfo): void {
    event.preventDefault();
    commandMenuCopied = false;
    commandMenu = { x: event.clientX, y: event.clientY, command };
  }

  async function copyCommandPath(): Promise<void> {
    if (commandMenu === null) return;
    try {
      await navigator.clipboard.writeText(commandMenu.command.module_path);
      commandMenuCopied = true;
    } catch {
      commandMenuCopied = false;
    }
  }

  function closeCommandMenu(): void {
    commandMenu = null;
    commandMenuCopied = false;
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
    <p class="text-xs font-semibold tracking-[0.12em] text-blue uppercase">Commands</p>
    <div class="mt-2 flex flex-wrap items-center justify-between gap-3">
      <h2 class="text-2xl font-semibold text-text">コマンドワークスペース</h2>
      <span class={`rounded-full px-3 py-1 text-xs ${backendState?.command_state === 'running' ? 'bg-green/15 text-green' : backendState?.command_state === 'paused' ? 'bg-yellow/15 text-yellow' : backendState?.command_state === 'error' ? 'bg-red/10 text-red' : 'bg-text/5 text-subtext1'}`}>
        {backendState?.command_state ?? 'stopped'}
      </span>
    </div>
  </div>

  {#if error !== null}
    <div class="rounded-lg border border-red/20 bg-red/10 px-3 py-2 text-sm text-red" role="alert">{error}</div>
  {:else if notice !== null}
    <div class="rounded-lg border border-green/20 bg-green/10 px-3 py-2 text-sm text-green" role="status">{notice}</div>
  {/if}

  <div class="flex gap-1 overflow-x-auto border-b border-text/10" role="tablist" aria-label="Command views">
    {#each subtabs as tab (tab.id)}
      <button
        id={`command-tab-${tab.id}`}
        type="button"
        role="tab"
        aria-selected={activeSubtab === tab.id}
        aria-controls={`command-panel-${tab.id}`}
        tabindex={activeSubtab === tab.id ? 0 : -1}
        class={`border-b-2 px-3 py-2 text-sm ${activeSubtab === tab.id ? 'border-blue text-blue' : 'border-transparent text-subtext1'}`}
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
          <div class={`rounded-lg border p-2 ${assigningSlot === index ? 'border-yellow/50 bg-yellow/10' : 'border-text/10 bg-crust/15'}`}>
            <button
              type="button"
              class="min-h-14 w-full rounded-lg px-2 py-2 text-left text-sm text-text hover:bg-text/5"
              aria-label={`Shortcut ${String(index)}: ${shortcutLabel(index)}`}
              oncontextmenu={(event) => contextShortcut(event, index)}
              onclick={(event) => beginShortcutAssignment(event, index)}
            >
              <span class="block text-[10px] text-subtext0">#{String(index)}</span>
              <span class="line-clamp-2">{shortcutLabel(index)}</span>
            </button>
            {#if assigned !== null}
              <button type="button" class="mt-1 w-full rounded-md bg-green/10 px-2 py-1 text-xs text-green" disabled={busy} onclick={() => void start(assigned)}>Run</button>
            {/if}
          </div>
        {/each}
      </div>
      <p class="text-xs text-subtext0">Click a slot, then choose a command. Shift+click or right-click clears it.</p>
      {#if assigningSlot !== null}
        <section class="rounded-lg border border-yellow/20 bg-yellow/5 p-3" aria-label={`Assign shortcut ${String(assigningSlot)}`}>
          <div class="flex items-center justify-between gap-3">
            <h3 class="text-sm font-medium text-yellow">Choose command for shortcut #{String(assigningSlot)}</h3>
            <button type="button" class="text-xs text-subtext1" onclick={() => (assigningSlot = null)}>Cancel</button>
          </div>
          <div class="mt-3 grid gap-2 sm:grid-cols-2">
            {#each backendState?.command_candidates ?? [] as command (identity(command))}
              <button type="button" class="rounded-lg bg-crust/20 px-3 py-2 text-left text-sm text-text hover:bg-text/5" aria-label={`Assign ${command.name} to shortcut ${String(assigningSlot)}`} onclick={() => void assignShortcut(command)}>
                <span class="block font-medium">{command.name}</span><span class="block truncate text-xs text-subtext0">{command.module_path}</span>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    </div>
  {:else}
    <div id={`command-panel-${activeSubtab}`} role="tabpanel" aria-labelledby={`command-tab-${activeSubtab}`} class="space-y-3">
      <div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto_auto]">
        <label>
          <span class="sr-only">Search commands</span>
          <input class="w-full rounded-lg border border-text/10 bg-surface1 px-3 py-2 text-sm text-text" type="search" bind:value={search} placeholder="Search commands" />
        </label>
        <label>
          <span class="sr-only">Command order</span>
          <select class="w-full rounded-lg border border-text/10 bg-surface1 px-3 py-2 text-sm text-text" bind:value={commandSort} aria-label="Command order">
            <option value="default">Default order</option>
            <option value="name_asc">Name (A-Z)</option>
            <option value="name_desc">Name (Z-A)</option>
            <option value="mtime_desc">Updated (newest first)</option>
            <option value="mtime_asc">Updated (oldest first)</option>
            <option value="usage_desc">Usage (most first)</option>
            <option value="usage_asc">Usage (least first)</option>
          </select>
        </label>
        <label>
          <span class="sr-only">Command tag</span>
          <select class="w-full rounded-lg border border-text/10 bg-surface1 px-3 py-2 text-sm text-text" bind:value={selectedTag}>
            {#each orderedTags as tag (tag)}<option value={tag}>{tag === '-' ? 'All tags' : tag}</option>{/each}
          </select>
        </label>
        <label>
          <span class="sr-only">Tag match mode</span>
          <select class="w-full rounded-lg border border-text/10 bg-surface1 px-3 py-2 text-sm text-text" value={settings?.['commands.tag_match_mode'] ?? 'exact'} onchange={(event) => void changeMatchMode(event)}>
            <option value="exact">Exact</option><option value="partial">Partial</option><option value="prefix">Prefix</option><option value="suffix">Suffix</option>
          </select>
        </label>
      </div>

      <div class="max-h-[26rem] overflow-auto rounded-lg border border-text/10 bg-crust/20 p-2" aria-busy={backendState?.command_display_cache_loading ?? false}>
        {#if backendState?.command_display_cache_loading && visibleItems.length === 0}
          <p class="p-4 text-sm text-subtext0">Building command display cache…</p>
        {:else if visibleItems.length === 0}
          <p class="p-4 text-sm text-subtext0">No matching commands</p>
        {:else}
          {#each visibleItems as item, index (`${item.kind}-${String(index)}`)}
            {@const command = displayCommand(item)}
            {#if command === null}
              <div class="my-2 flex items-center gap-2 text-[10px] tracking-[0.14em] text-subtext0 uppercase" role="separator"><span class="h-px flex-1 bg-text/10"></span>{separatorLabel(item)}<span class="h-px flex-1 bg-text/10"></span></div>
            {:else}
              <button
                type="button"
                class={`mb-1 w-full rounded-lg border px-3 py-2 text-left ${selectedIdentity === identity(command) ? 'border-blue/40 bg-blue/10' : 'border-transparent bg-text/[0.025] hover:bg-text/5'}`}
                aria-label={`Select ${command.name}`}
                onclick={() => selectCommand(command)}
                ondblclick={() => void start(command)}
                oncontextmenu={(event) => openCommandMenu(event, command)}
              >
                <span class="flex items-start justify-between gap-3"><span class="font-medium text-text">{command.name}</span><span class="text-[10px] text-subtext0">{command.class_name}</span></span>
                <span class="mt-1 block truncate text-xs text-subtext0">{command.module_path}</span>
                {#if command.tags.length > 0}<span class="mt-2 flex flex-wrap gap-1">{#each command.tags as tag (tag)}<span class="rounded bg-text/5 px-1.5 py-0.5 text-[10px] text-subtext1">{tag}</span>{/each}</span>{/if}
              </button>
            {/if}
          {/each}
        {/if}
      </div>
    </div>
  {/if}

  <div class="flex flex-wrap items-center gap-2 rounded-lg border border-text/10 bg-crust/20 p-3">
    <button type="button" class="rounded-lg bg-green/15 px-3 py-2 text-sm font-medium text-green disabled:opacity-40" disabled={busy || selectedCommand === null || backendState?.command_state === 'running' || backendState?.command_state === 'paused'} onclick={() => void start()}>Start</button>
    <button type="button" class="rounded-lg bg-yellow/10 px-3 py-2 text-sm text-yellow disabled:opacity-40" disabled={busy || backendState?.command_state !== 'running'} onclick={() => void control({ action: 'pause' })}>Pause</button>
    <button type="button" class="rounded-lg bg-blue/10 px-3 py-2 text-sm text-blue disabled:opacity-40" disabled={busy || backendState?.command_state !== 'paused'} onclick={() => void control({ action: 'resume' })}>Resume</button>
    <button type="button" class="rounded-lg bg-red/10 px-3 py-2 text-sm text-red disabled:opacity-40" disabled={busy || (backendState?.command_state !== 'running' && backendState?.command_state !== 'paused')} onclick={() => void control({ action: 'stop' })}>Stop</button>
    <button type="button" class="rounded-lg bg-text/5 px-3 py-2 text-sm text-subtext1 disabled:opacity-40" disabled={busy} onclick={() => void reload()}>Reload</button>
    <button type="button" class="rounded-lg bg-text/5 px-3 py-2 text-sm text-subtext1" onclick={() => (creatorOpen = true)}>New script</button>
    <span class="ml-auto text-xs text-subtext0">{selectedCommand?.name ?? backendState?.current_command ?? 'No command selected'}</span>
  </div>

  {#if commandMenu !== null}
    <div
      class="fixed inset-0 z-40"
      role="presentation"
      onclick={(event) => { if (event.target === event.currentTarget) closeCommandMenu(); }}
      onkeydown={(event) => { if (event.key === 'Escape') closeCommandMenu(); }}
      oncontextmenu={(event) => { event.preventDefault(); closeCommandMenu(); }}
    >
      <div
        class="absolute z-50 w-72 rounded-lg border border-text/15 bg-surface1 p-3 shadow-lg"
        style={`left: ${String(Math.min(commandMenu.x, 800))}px; top: ${String(Math.min(commandMenu.y, 500))}px;`}
        role="menu"
        tabindex="-1"
        aria-label={`Actions for ${commandMenu.command.name}`}
      >
        <p class="text-xs font-semibold text-text">{commandMenu.command.name}</p>
        <p class="mt-1 break-all text-[11px] text-subtext0" aria-label="Command path">{commandMenu.command.module_path}</p>
        <div class="mt-2 flex items-center gap-2">
          <button type="button" class="rounded-md bg-blue/15 px-2 py-1 text-[11px] font-medium text-blue" onclick={() => void copyCommandPath()}>Copy path</button>
          {#if commandMenuCopied}<span class="text-[11px] text-green">Copied</span>{/if}
          <button type="button" class="ml-auto rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1" onclick={closeCommandMenu}>Close</button>
        </div>
      </div>
    </div>
  {/if}

  {#if creatorOpen}
    <ScriptCreatorDialog actions={actions} onclose={() => (creatorOpen = false)} />
  {/if}
</div>
