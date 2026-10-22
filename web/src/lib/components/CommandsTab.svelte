<script lang="ts">
  import Fuse from 'fuse.js';

  import type {
    CommandControlRequest,
    CommandHistoryControlRequest,
    CommandHistoryEntry,
    CommandHistorySnapshot,
    CommandPauseSnapshot,
    CommandTagRequest,
    InputRecordControlRequest,
    InputRecordResult,
    InputRecordSnapshot,
    CommandTagResult,
    CommandTagSnapshot,
    OperationResult,
    RunListSaveRequest,
    RunListSnapshot,
    RunScheduleControlRequest,
    RunScheduleSnapshot,
    ScriptCreatorPreview,
    ScriptCreatorResult,
    ScriptCreatorSnapshot,
    ScriptCreatorSpec
  } from '../actions';
  import RunListEditorDialog from './RunListEditorDialog.svelte';
  import TagEditorDialog from './TagEditorDialog.svelte';
  import ScriptCreatorDialog from './ScriptCreatorDialog.svelte';
  import type { SettingsWriteValues } from '../api';
  import type { components } from '../api/openapi';
  import type { ApplicationRuntime, RuntimeView } from '../runtime';

  type CommandDisplayItem = components['schemas']['CommandDisplayItem'];
  type CommandInfo = components['schemas']['CommandInfo'];
  type CommandIdentity = components['schemas']['CommandIdentity'];
  type ShortcutIndex = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10;
  type ShortcutSetting = `shortcuts.button_${ShortcutIndex}`;
  type Subtab = 'python' | 'mcu' | 'shortcuts' | 'history';

  interface CommandActions {
    commandHistory(): Promise<CommandHistorySnapshot>;
    commandPause(): Promise<CommandPauseSnapshot>;
    inputRecord(): Promise<InputRecordSnapshot>;
    controlInputRecord(request: InputRecordControlRequest): Promise<InputRecordResult>;
    commandTags(): Promise<CommandTagSnapshot>;
    applyCommandTags(request: CommandTagRequest): Promise<CommandTagResult>;
    controlCommand(request: CommandControlRequest): Promise<OperationResult>;
    controlCommandHistory(
      request: CommandHistoryControlRequest
    ): Promise<CommandHistorySnapshot>;
    createScript(request: ScriptCreatorSpec): Promise<ScriptCreatorResult>;
    previewScriptCreation(request: ScriptCreatorSpec): Promise<ScriptCreatorPreview>;
    reloadCommands(): Promise<OperationResult>;
    runLists(): Promise<RunListSnapshot>;
    runSchedule(): Promise<RunScheduleSnapshot>;
    controlRunSchedule(request: RunScheduleControlRequest): Promise<RunScheduleSnapshot>;
    saveRunLists(request: RunListSaveRequest): Promise<RunListSnapshot>;
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
    { id: 'shortcuts', label: 'Shortcut' },
    { id: 'history', label: 'History' }
  ];

  let { actions, runtime, view }: Props = $props();
  let creatorOpen = $state(false);
  let runListOpen = $state(false);
  let tagOpen = $state(false);
  let pauseInfo = $state<CommandPauseSnapshot | null>(null);
  let recordInfo = $state<InputRecordSnapshot | null>(null);
  let recordName = $state('record');
  let recordBusy = $state(false);
  let recordMessage = $state<string | null>(null);
  let recordSavedPath = $state<string | null>(null);
  let recordScriptPath = $state<string | null>(null);
  let copiedPauseFile = $state<string | null>(null);
  let activeSubtab = $state<Subtab>('python');
  let assigningSlot = $state<ShortcutIndex | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let search = $state('');
  let selectedIdentity = $state<string | null>(null);
  let selectedTag = $state('-');
  let commandMenu = $state<{ x: number; y: number; command: CommandInfo } | null>(null);
  let historyEntries = $state<CommandHistoryEntry[]>([]);
  let historyDescending = $state(true);
  let historySelected = $state<string | null>(null);
  let historyBusy = $state(false);
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
  const runListSelection = $derived(
    selectedCommand === null
      ? null
      : {
          name: selectedCommand.name,
          type: detectedMcu(selectedCommand) ? 'Mcu' : 'Python',
          module_path: selectedCommand.module_path,
          class_name: selectedCommand.class_name
        }
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

  const sortedHistory = $derived.by(() => {
    const rows = [...historyEntries];
    rows.sort((left, right) =>
      historyDescending
        ? right.last_run.localeCompare(left.last_run)
        : left.last_run.localeCompare(right.last_run)
    );
    return rows;
  });

  $effect(() => {
    if (activeSubtab !== 'history') return;
    void refreshHistory();
  });

  async function refreshHistory(): Promise<void> {
    historyBusy = true;
    try {
      historyEntries = (await actions.commandHistory()).entries;
    } catch (reason: unknown) {
      error = reason instanceof Error ? reason.message : 'Command history load failed';
    } finally {
      historyBusy = false;
    }
  }

  function commandForHistory(entry: CommandHistoryEntry): CommandInfo | null {
    const candidates = backendState?.command_candidates ?? [];
    return (
      candidates.find(
        (command) => command.name === entry.name && command.class_name === entry.name
      ) ??
      candidates.find((command) => command.name === entry.name) ??
      null
    );
  }

  async function applyHistory(
    request: CommandHistoryControlRequest
  ): Promise<void> {
    historyBusy = true;
    try {
      historyEntries = (await actions.controlCommandHistory(request)).entries;
      historySelected = null;
    } catch (reason: unknown) {
      error = reason instanceof Error ? reason.message : 'Command history update failed';
    } finally {
      historyBusy = false;
    }
  }

  async function runHistoryEntry(entry: CommandHistoryEntry): Promise<void> {
    const command = commandForHistory(entry);
    if (command === null) {
      error = 'Command is no longer available.';
      return;
    }
    await start(command);
    await refreshHistory();
  }

  function historyKey(entry: CommandHistoryEntry): string {
    return entry.type + '\u0000' + entry.name;
  }

  async function removeSelectedHistory(): Promise<void> {
    if (historySelected === null) return;
    const entry = historyEntries.find(
      (candidate) => historyKey(candidate) === historySelected
    );
    if (entry === undefined) return;
    await applyHistory({ action: 'remove', type: entry.type, name: entry.name });
  }

  async function clearHistory(): Promise<void> {
    if (historyEntries.length === 0) return;
    if (!window.confirm(`すべての履歴(${String(historyEntries.length)}件)を削除しますか？`)) return;
    await applyHistory({ action: 'clear' });
  }

  async function selectHistoryShortcut(entry: CommandHistoryEntry, event: Event): Promise<void> {
    const select = event.currentTarget as HTMLSelectElement;
    const value = select.value;
    select.value = '';
    if (value.length === 0) return;
    const command = commandForHistory(entry);
    if (command === null) {
      error = 'Command is no longer available.';
      return;
    }
    assigningSlot = Number(value) as ShortcutIndex;
    await assignShortcut(command);
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
  $effect(() => {
    if (backendState?.command_state !== 'paused') {
      pauseInfo = null;
      return;
    }
    let active = true;
    const load = async (): Promise<void> => {
      try {
        const snapshot = await actions.commandPause();
        if (active) pauseInfo = snapshot;
      } catch {
        // The command can resume between the state update and this read;
        // keep the panel hidden rather than surfacing a transient error.
      }
    };
    void load();
    const timer = setInterval(() => void load(), 1000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  });

  $effect(() => {
    if (!recordInfo?.recording) {
      return;
    }
    let active = true;
    const load = async (): Promise<void> => {
      try {
        const snapshot = await actions.inputRecord();
        if (active) recordInfo = snapshot;
      } catch {
        // The recording may be discarded by another client; polling resumes
        // on the next explicit action.
      }
    };
    void load();
    const timer = setInterval(() => void load(), 1000);
    return () => {
      active = false;
      clearInterval(timer);
    };
  });

  async function startRecording(): Promise<void> {
    recordBusy = true;
    recordMessage = null;
    recordSavedPath = null;
    recordScriptPath = null;
    try {
      const result = await actions.controlInputRecord({ action: 'start' });
      recordInfo = result.snapshot;
    } catch (failure) {
      recordMessage = failure instanceof Error ? failure.message : '記録を開始できませんでした';
    } finally {
      recordBusy = false;
    }
  }

  async function stopRecording(): Promise<void> {
    recordBusy = true;
    try {
      const result = await actions.controlInputRecord({ action: 'stop' });
      recordInfo = result.snapshot;
    } catch (failure) {
      recordMessage = failure instanceof Error ? failure.message : '記録を停止できませんでした';
    } finally {
      recordBusy = false;
    }
  }

  async function saveRecording(): Promise<void> {
    recordBusy = true;
    recordMessage = null;
    try {
      const result = await actions.controlInputRecord({ action: 'save', name: recordName });
      recordInfo = result.snapshot;
      recordSavedPath = result.saved_path ?? null;
      recordMessage = 'CSVを保存しました';
    } catch (failure) {
      recordMessage = failure instanceof Error ? failure.message : 'CSVを保存できませんでした';
    } finally {
      recordBusy = false;
    }
  }

  async function generateRecording(): Promise<void> {
    recordBusy = true;
    recordMessage = null;
    try {
      const result = await actions.controlInputRecord({ action: 'generate', name: recordName });
      recordInfo = result.snapshot;
      recordScriptPath = result.script_path ?? null;
      recordMessage = 'スクリプトを生成しました';
      void reload();
    } catch (failure) {
      recordMessage = failure instanceof Error ? failure.message : 'スクリプトを生成できませんでした';
    } finally {
      recordBusy = false;
    }
  }

  async function discardRecording(): Promise<void> {
    recordBusy = true;
    recordMessage = null;
    try {
      const result = await actions.controlInputRecord({ action: 'discard' });
      recordInfo = result.snapshot;
      recordSavedPath = null;
      recordScriptPath = null;
      recordMessage = '記録を破棄しました';
    } catch (failure) {
      recordMessage = failure instanceof Error ? failure.message : '記録を破棄できませんでした';
    } finally {
      recordBusy = false;
    }
  }

  async function copyPauseFile(file: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(file);
      copiedPauseFile = file;
    } catch {
      copiedPauseFile = null;
    }
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

  {#if recordInfo?.recording}
    <div class="rounded-lg border border-red/20 bg-red/10 px-3 py-2 text-xs text-red" role="status">
      入力記録中: {String(recordInfo.samples)} 件 / {String(Math.floor(recordInfo.elapsed_ms / 1000))} 秒（上限 {String(recordInfo.limit)} 件）
    </div>
  {/if}

  {#if recordInfo && !recordInfo.recording && recordInfo.samples > 0}
    <section class="rounded-lg border border-blue/20 bg-blue/10 p-3" aria-label="Recorded input">
      <h3 class="text-xs font-semibold text-blue">入力記録</h3>
      <p class="mt-1 text-[11px] text-subtext0">{String(recordInfo.samples)} 件を記録しました。CSV保存またはスクリプト生成を選べます。</p>
      <div class="mt-2 flex flex-wrap items-end gap-2">
        <label class="text-xs text-subtext1">
          Name
          <input class="mt-1 w-48 rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label="Record name" value={recordName} oninput={(event) => (recordName = event.currentTarget.value)} />
        </label>
        <button type="button" class="rounded-md bg-blue/15 px-2 py-1 text-[11px] text-blue disabled:opacity-40" disabled={recordBusy || recordName.trim().length === 0} onclick={() => void saveRecording()}>Save CSV</button>
        <button type="button" class="rounded-md bg-green/15 px-2 py-1 text-[11px] text-green disabled:opacity-40" disabled={recordBusy || recordName.trim().length === 0} onclick={() => void generateRecording()}>Generate script</button>
        <button type="button" class="rounded-md bg-red/10 px-2 py-1 text-[11px] text-red disabled:opacity-40" disabled={recordBusy} onclick={() => void discardRecording()}>Discard</button>
      </div>
      {#if recordMessage !== null}
        <p class="mt-2 text-[11px] text-subtext1" role="status">{recordMessage}</p>
      {/if}
      {#if recordSavedPath !== null}
        <p class="mt-1 break-all text-[11px] text-subtext0">CSV: {recordSavedPath}</p>
      {/if}
      {#if recordScriptPath !== null}
        <p class="mt-1 break-all text-[11px] text-subtext0">Script: {recordScriptPath}</p>
      {/if}
    </section>
  {/if}

  {#if pauseInfo?.paused}
    <section class="rounded-lg border border-yellow/25 bg-yellow/10 p-3" aria-label="Pause information">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <h3 class="text-xs font-semibold text-yellow">停止位置</h3>
        <span class="text-[11px] text-subtext0">呼び出し順（外側 → 内側）</span>
      </div>
      <ol class="mt-2 space-y-1">
        {#each pauseInfo.frames as frame, index (frame.file + ':' + String(frame.line) + ':' + frame.function)}
          <li class="flex flex-wrap items-center gap-2 text-[11px] text-subtext0">
            <span class="text-text">{String(index + 1)}. {frame.function}</span>
            <span class="break-all">{frame.file}:{String(frame.line)}</span>
            <button type="button" class="rounded bg-text/5 px-1.5 py-0.5 text-[10px] text-subtext1" aria-label={`Copy pause path ${frame.file}`} onclick={() => void copyPauseFile(frame.file)}>Copy path</button>
            {#if copiedPauseFile === frame.file}<span class="text-[10px] text-green">Copied</span>{/if}
          </li>
        {/each}
        {#if pauseInfo.frames.length === 0}
          <li class="text-[11px] text-subtext0">停止位置を取得できませんでした。</li>
        {/if}
      </ol>
      <h3 class="mt-3 text-xs font-semibold text-yellow">変数</h3>
      {#if pauseInfo.variables.length > 0}
        <dl class="mt-1 grid gap-x-4 gap-y-1 sm:grid-cols-[minmax(0,auto)_minmax(0,1fr)]">
          {#each pauseInfo.variables as variable (variable.name)}
            <dt class="break-all text-[11px] text-text">{variable.name}</dt>
            <dd class="break-all text-[11px] text-subtext0">{variable.value}</dd>
          {/each}
        </dl>
      {:else}
        <p class="mt-1 text-[11px] text-subtext0">表示できる変数はありません。</p>
      {/if}
    </section>
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
  {:else if activeSubtab === 'history'}
    <div id="command-panel-history" role="tabpanel" aria-labelledby="command-tab-history" class="space-y-3">
      <div class="flex flex-wrap items-center gap-2">
        <button type="button" class="rounded-lg bg-text/5 px-3 py-1 text-xs text-subtext1" onclick={() => (historyDescending = !historyDescending)}>
          Last Run {historyDescending ? '▼' : '▲'}
        </button>
        <button type="button" class="rounded-lg bg-red/10 px-3 py-1 text-xs text-red disabled:opacity-40" disabled={historyBusy || historySelected === null} onclick={() => void removeSelectedHistory()}>Delete</button>
        <button type="button" class="rounded-lg bg-red/10 px-3 py-1 text-xs text-red disabled:opacity-40" disabled={historyBusy || historyEntries.length === 0} onclick={() => void clearHistory()}>Delete all</button>
        <span class="ml-auto text-xs text-subtext0">{historyEntries.length} / 10</span>
      </div>
      <div class="max-h-[26rem] space-y-1 overflow-auto rounded-lg border border-text/10 bg-crust/20 p-2">
        {#if sortedHistory.length === 0}
          <p class="p-2 text-xs text-subtext0">No command runs recorded yet.</p>
        {/if}
        {#each sortedHistory as entry (historyKey(entry))}
          {@const key = historyKey(entry)}
          <div class={`flex flex-wrap items-center gap-2 rounded-lg border px-2 py-1 ${historySelected === key ? 'border-blue/40 bg-blue/10' : 'border-transparent bg-text/[0.025]'}`}>
            <button type="button" class="min-w-0 flex-1 text-left" aria-label={`Run ${entry.name}`} onclick={() => void runHistoryEntry(entry)}>
              <span class="block truncate text-sm font-medium text-text">{entry.name}</span>
              <span class="block text-[11px] text-subtext0">{entry.last_run} · {entry.type}</span>
            </button>
            <select class="rounded-md border border-text/15 bg-crust/40 px-1 py-0.5 text-[11px] text-subtext1" aria-label={`Shortcut for ${entry.name}`} value="" onchange={(event) => void selectHistoryShortcut(entry, event)}>
              <option value="">Shortcut…</option>
              {#each shortcutIndexes as index (index)}<option value={String(index)}>#{String(index)}</option>{/each}
            </select>
            <button type="button" class="rounded-md bg-text/5 px-2 py-0.5 text-[11px] text-subtext1" aria-label={`Select ${entry.name} in history`} onclick={() => (historySelected = key)}>Select</button>
          </div>
        {/each}
      </div>
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
    <button type="button" class="rounded-lg bg-green/15 px-3 py-2 text-sm font-medium text-green disabled:opacity-40" disabled={busy || selectedCommand === null || activeSubtab === 'shortcuts' || activeSubtab === 'history' || backendState?.command_state === 'running' || backendState?.command_state === 'paused'} onclick={() => void start()}>Start</button>
    <button type="button" class="rounded-lg bg-yellow/10 px-3 py-2 text-sm text-yellow disabled:opacity-40" disabled={busy || backendState?.command_state !== 'running'} onclick={() => void control({ action: 'pause' })}>Pause</button>
    <button type="button" class="rounded-lg bg-blue/10 px-3 py-2 text-sm text-blue disabled:opacity-40" disabled={busy || backendState?.command_state !== 'paused'} onclick={() => void control({ action: 'resume' })}>Resume</button>
    <button type="button" class="rounded-lg bg-red/10 px-3 py-2 text-sm text-red disabled:opacity-40" disabled={busy || (backendState?.command_state !== 'running' && backendState?.command_state !== 'paused')} onclick={() => void control({ action: 'stop' })}>Stop</button>
    <button type="button" class="rounded-lg bg-text/5 px-3 py-2 text-sm text-subtext1 disabled:opacity-40" disabled={busy} onclick={() => void reload()}>Reload</button>
    <button type="button" class="rounded-lg bg-text/5 px-3 py-2 text-sm text-subtext1" onclick={() => (creatorOpen = true)}>New script</button>
    <button type="button" class="rounded-lg bg-text/5 px-3 py-2 text-sm text-subtext1" onclick={() => (runListOpen = true)}>Run lists</button>
    <button type="button" class="rounded-lg bg-text/5 px-3 py-2 text-sm text-subtext1" onclick={() => (tagOpen = true)}>Tags</button>
    <button type="button" class={`rounded-lg px-3 py-2 text-sm ${recordInfo?.recording ? 'bg-red/10 text-red' : 'bg-text/5 text-subtext1'}`} disabled={recordBusy} onclick={() => (recordInfo?.recording ? void stopRecording() : void startRecording())}>{recordInfo?.recording ? `Stop recording (${String(recordInfo.samples)})` : 'Record'}</button>
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

  {#if runListOpen}
    <RunListEditorDialog
      actions={actions}
      selected={runListSelection}
      onclose={() => (runListOpen = false)}
      onsaved={() => void reload()}
    />
  {/if}

  {#if tagOpen}
    <TagEditorDialog
      actions={actions}
      selected={selectedCommand}
      onclose={() => (tagOpen = false)}
      onsaved={() => void reload()}
    />
  {/if}
</div>
