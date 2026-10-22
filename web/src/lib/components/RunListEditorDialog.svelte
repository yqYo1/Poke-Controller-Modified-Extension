<script lang="ts">
  import type {
    RunListItem,
    RunListSnapshot,
    RunListSpec,
    RunScheduleControlRequest,
    RunScheduleSnapshot
  } from '../actions';

  interface RunListActions {
    controlRunSchedule(request: RunScheduleControlRequest): Promise<RunScheduleSnapshot>;
    runLists(): Promise<RunListSnapshot>;
    runSchedule(): Promise<RunScheduleSnapshot>;
    saveRunLists(request: { lists: RunListSpec[] }): Promise<RunListSnapshot>;
  }

  interface Selection {
    readonly name: string;
    readonly type: string;
    readonly module_path: string;
    readonly class_name: string;
  }

  interface Props {
    actions: RunListActions;
    selected: Selection | null;
    onclose: () => void;
    onsaved?: () => void;
  }

  let { actions, selected, onclose, onsaved }: Props = $props();

  const NAME_SUFFIX = ' (実行リスト)';

  let lists = $state<RunListSpec[]>([]);
  let draft = $state<RunListSpec | null>(null);
  let editingIndex = $state<number | null>(null);
  let selectedRow = $state<number | null>(null);
  let dirty = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let activeTab = $state<'lists' | 'schedule'>('lists');
  let schedule = $state<RunScheduleSnapshot | null>(null);
  let scheduleError = $state<string | null>(null);
  let scheduleMessage = $state<string | null>(null);
  let scheduleBusy = $state(false);
  let startMode = $state<'clock' | 'minutes'>('clock');
  let startClock = $state('');
  let startMinutes = $state('');
  let stopMode = $state<'clock' | 'minutes'>('clock');
  let stopClock = $state('');
  let stopMinutes = $state('');

  const rows = $derived(draft?.items ?? []);
  const selectedItem = $derived(
    draft !== null && selectedRow !== null ? (draft.items?.[selectedRow] ?? null) : null
  );

  $effect(() => {
    void load();
  });

  $effect(() => {
    if (activeTab !== 'schedule') return;
    void refreshSchedule();
    const timer = setInterval(() => void refreshSchedule(), 1000);
    return () => clearInterval(timer);
  });

  async function load(): Promise<void> {
    try {
      const snapshot = await actions.runLists();
      lists = [...snapshot.lists];
      if (lists.length > 0) {
        const first = lists[0];
        if (first !== undefined) beginEdit(0, first);
      } else {
        draft = {
          name: uniqueName('New list'),
          items: [],
          interval_seconds: 1,
          on_error: 'stop',
          loops: 1
        };
        editingIndex = null;
        selectedRow = null;
        dirty = true;
        message = 'Select a command in the Commands tab, then press "Add selected command".';
      }
    } catch (reason: unknown) {
      error = messageOf(reason, 'Run list load failed');
    }
  }

  function messageOf(reason: unknown, fallback: string): string {
    return reason instanceof Error ? reason.message : fallback;
  }

  function clone(list: RunListSpec): RunListSpec {
    return {
      name: list.name,
      items: (list.items ?? []).map((item) => ({
        type: item.type,
        name: item.name,
        repeat: item.repeat ?? 1,
        limit_minutes: item.limit_minutes ?? null
      })),
      interval_seconds: list.interval_seconds ?? 1,
      on_error: list.on_error ?? 'stop',
      loops: list.loops ?? 1
    };
  }

  function beginEdit(index: number, list: RunListSpec): void {
    draft = clone(list);
    editingIndex = index;
    selectedRow = null;
    dirty = false;
    message = null;
  }

  function uniqueName(base: string): string {
    const existing = new Set(lists.map((list) => list.name));
    if (!existing.has(base)) return base;
    let index = 2;
    while (existing.has(`${base} ${String(index)}`)) index += 1;
    return `${base} ${String(index)}`;
  }

  function confirmDiscard(): boolean {
    return !dirty || window.confirm('Discard the unsaved changes?');
  }

  function selectList(index: number): void {
    const list = lists[index];
    if (list === undefined || index === editingIndex) return;
    if (!confirmDiscard()) return;
    beginEdit(index, list);
  }

  function newList(): void {
    if (!confirmDiscard()) return;
    draft = {
      name: uniqueName('New list'),
      items: [],
      interval_seconds: 1,
      on_error: 'stop',
      loops: 1
    };
    editingIndex = null;
    selectedRow = null;
    dirty = true;
    message = 'Select a command in the Commands tab, then press "Add selected command".';
  }

  function duplicate(): void {
    if (draft === null || !confirmDiscard()) return;
    draft = { ...clone(draft), name: uniqueName(`${draft.name} copy`) };
    editingIndex = null;
    selectedRow = null;
    dirty = true;
    message = 'Press "Save" to keep the copied run list.';
  }

  async function deleteList(): Promise<void> {
    if (draft === null) return;
    if (editingIndex === null) {
      if (!window.confirm('Discard the unsaved run list?')) return;
      const first = lists[0];
      if (first === undefined) {
        draft = null;
      } else {
        beginEdit(0, first);
      }
      return;
    }
    const name = draft.name;
    if (!window.confirm(`Delete the run list "${name}"?`)) return;
    const next = lists.filter((_, index) => index !== editingIndex);
    busy = true;
    error = null;
    try {
      const snapshot = await actions.saveRunLists({ lists: next });
      lists = [...snapshot.lists];
      onsaved?.();
      const first = lists[0];
      if (first === undefined) {
        draft = null;
        editingIndex = null;
        selectedRow = null;
      } else {
        beginEdit(0, first);
      }
      message = `Deleted "${name}".`;
    } catch (reason: unknown) {
      message = messageOf(reason, 'Run list delete failed');
    } finally {
      busy = false;
    }
  }

  function addSelected(): void {
    if (draft === null) return;
    if (selected === null) {
      message = 'Select a command in the Commands tab first.';
      return;
    }
    const item: RunListItem = {
      type: selected.type,
      name: selected.name,
      repeat: 1,
      limit_minutes: null
    };
    const items = [...(draft.items ?? [])];
    const insertAt = selectedRow === null ? items.length : selectedRow + 1;
    items.splice(insertAt, 0, item);
    draft.items = items;
    selectedRow = insertAt;
    dirty = true;
    message = selected.type === 'Mcu' ? `Enter the limit minutes for "${selected.name}".` : null;
  }

  function moveRow(offset: number): void {
    if (draft === null || selectedRow === null) return;
    const items = [...(draft.items ?? [])];
    const item = items[selectedRow];
    const target = selectedRow + offset;
    if (item === undefined || target < 0 || target >= items.length) return;
    items.splice(selectedRow, 1);
    items.splice(target, 0, item);
    draft.items = items;
    selectedRow = target;
    dirty = true;
  }

  function removeRow(): void {
    if (draft === null || selectedRow === null) return;
    const items = [...(draft.items ?? [])];
    if (items[selectedRow] === undefined) return;
    items.splice(selectedRow, 1);
    draft.items = items;
    selectedRow = items.length === 0 ? null : Math.min(selectedRow, items.length - 1);
    dirty = true;
  }

  function updateRepeat(value: string): void {
    if (selectedItem === null) return;
    const trimmed = value.trim();
    selectedItem.repeat = trimmed.length === 0 ? 1 : Number(value);
    dirty = true;
  }

  function updateLimit(value: string): void {
    if (selectedItem === null) return;
    const trimmed = value.trim();
    selectedItem.limit_minutes = trimmed.length === 0 ? null : Number(value);
    dirty = true;
  }

  function updateName(value: string): void {
    if (draft === null) return;
    draft.name = value;
    dirty = true;
  }

  function updateInterval(value: string): void {
    if (draft === null) return;
    const trimmed = value.trim();
    draft.interval_seconds = trimmed.length === 0 ? undefined : Number(value);
    dirty = true;
  }

  function updateLoops(value: string): void {
    if (draft === null) return;
    const trimmed = value.trim();
    draft.loops = trimmed.length === 0 ? undefined : Number(value);
    dirty = true;
  }

  function updateOnError(value: string): void {
    if (draft === null) return;
    draft.on_error = value;
    dirty = true;
  }

  function validate(): string | null {
    if (draft === null) return 'Select or create a run list first.';
    if (draft.name.trim().length === 0) return 'Enter the run list name.';
    const items = draft.items ?? [];
    if (items.length === 0) return 'Add at least one command.';
    const interval = draft.interval_seconds;
    if (interval === undefined || !Number.isFinite(interval) || interval < 0 || interval > 3600) {
      return 'The interval must be a number between 0 and 3600 seconds.';
    }
    const loops = draft.loops;
    if (loops === undefined || !Number.isInteger(loops) || loops < 0 || loops > 9999) {
      return 'The loops must be an integer between 0 and 9999.';
    }
    for (const [index, item] of items.entries()) {
      const row = String(index + 1);
      const repeat = item.repeat ?? 1;
      if (!Number.isInteger(repeat) || repeat < 1 || repeat > 9999) {
        return `Row ${row}: the repeat must be an integer between 1 and 9999.`;
      }
      const limit = item.limit_minutes ?? null;
      if (item.type === 'Mcu' && limit === null) {
        return `Row ${row}: Mcu commands need the limit minutes.`;
      }
      if (limit !== null && (!Number.isFinite(limit) || limit <= 0)) {
        return `Row ${row}: the limit must be a positive number.`;
      }
      if (limit !== null && limit > 10080) {
        return `Row ${row}: the limit must be 7 days (10080 minutes) or less.`;
      }
    }
    return null;
  }

  async function save(): Promise<void> {
    if (draft === null) return;
    const problem = validate();
    if (problem !== null) {
      message = problem;
      return;
    }
    const spec: RunListSpec = {
      name: draft.name.trim(),
      items: (draft.items ?? []).map((item) => ({
        type: item.type,
        name: item.name,
        repeat: item.repeat ?? 1,
        limit_minutes: item.limit_minutes ?? null
      })),
      interval_seconds: draft.interval_seconds ?? 1,
      on_error: draft.on_error ?? 'stop',
      loops: draft.loops ?? 1
    };
    const next = [...lists];
    if (editingIndex === null) {
      next.push(spec);
    } else {
      next[editingIndex] = spec;
    }
    busy = true;
    error = null;
    try {
      const snapshot = await actions.saveRunLists({ lists: next });
      lists = [...snapshot.lists];
      const savedIndex = lists.findIndex((list) => list.name === spec.name);
      if (savedIndex >= 0) {
        const saved = lists[savedIndex];
        if (saved !== undefined) beginEdit(savedIndex, saved);
      } else {
        draft = null;
        editingIndex = null;
        selectedRow = null;
      }
      message = `Saved. Start "${spec.name}${NAME_SUFFIX}" from the Commands tab.`;
      onsaved?.();
    } catch (reason: unknown) {
      message = messageOf(reason, 'Run list save failed');
    } finally {
      busy = false;
    }
  }

  async function refreshSchedule(): Promise<void> {
    try {
      schedule = await actions.runSchedule();
      scheduleError = null;
    } catch (reason: unknown) {
      scheduleError = messageOf(reason, 'Run schedule load failed');
    }
  }

  function confirmSleepWarning(): boolean {
    if (schedule?.pending) return true;
    const warning = schedule?.sleep_warning ?? '';
    return window.confirm(`${warning}\n\n予約しますか?`);
  }

  async function controlSchedule(request: RunScheduleControlRequest): Promise<void> {
    scheduleBusy = true;
    scheduleMessage = null;
    try {
      schedule = await actions.controlRunSchedule(request);
      scheduleError = null;
    } catch (reason: unknown) {
      scheduleMessage = messageOf(reason, 'Run schedule control failed');
    } finally {
      scheduleBusy = false;
    }
  }

  function reserveStart(): void {
    if (selected === null) {
      scheduleMessage = 'Select a command in the Commands tab first.';
      return;
    }
    if (startMode === 'clock' && startClock.trim().length === 0) {
      scheduleMessage = 'Enter the start time as 23:30.';
      return;
    }
    if (startMode === 'minutes' && startMinutes.trim().length === 0) {
      scheduleMessage = 'Enter the minutes as a number.';
      return;
    }
    const target = {
      module_path: selected.module_path,
      class_name: selected.class_name,
      name: selected.name
    };
    const request: RunScheduleControlRequest =
      startMode === 'clock'
        ? { action: 'reserve_start', target, clock: startClock }
        : { action: 'reserve_start', target, after_minutes: Number(startMinutes) };
    if (!confirmSleepWarning()) return;
    void controlSchedule(request);
  }

  function reserveStop(): void {
    if (stopMode === 'clock' && stopClock.trim().length === 0) {
      scheduleMessage = 'Enter the stop time as 23:30.';
      return;
    }
    if (stopMode === 'minutes' && stopMinutes.trim().length === 0) {
      scheduleMessage = 'Enter the minutes as a number.';
      return;
    }
    const request: RunScheduleControlRequest =
      stopMode === 'clock'
        ? { action: 'reserve_stop', clock: stopClock }
        : { action: 'reserve_stop', after_minutes: Number(stopMinutes) };
    if (!confirmSleepWarning()) return;
    void controlSchedule(request);
  }
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" role="presentation">
  <section
    aria-label="Run list editor"
    class="max-h-[90vh] w-full max-w-4xl overflow-auto rounded-lg border border-text/15 bg-base p-4 shadow-xl"
  >
    <header class="mb-3 flex items-center justify-between">
      <h2 class="text-sm font-semibold text-text">Run list editor</h2>
      <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1" onclick={onclose}>Close</button>
    </header>

    <div class="mb-3 flex gap-1" role="tablist" aria-label="Run list editor tabs">
      <button type="button" role="tab" aria-selected={activeTab === 'lists'} class={`rounded-md px-3 py-1 text-xs font-medium ${activeTab === 'lists' ? 'bg-blue/15 text-blue' : 'bg-text/5 text-subtext1'}`} onclick={() => (activeTab = 'lists')}>Run lists</button>
      <button type="button" role="tab" aria-selected={activeTab === 'schedule'} class={`rounded-md px-3 py-1 text-xs font-medium ${activeTab === 'schedule' ? 'bg-blue/15 text-blue' : 'bg-text/5 text-subtext1'}`} onclick={() => (activeTab = 'schedule')}>Schedule</button>
    </div>

    {#if error !== null}
      <p class="mb-3 rounded-md bg-red/10 px-3 py-2 text-xs text-red" role="alert">{error}</p>
    {/if}

    {#if activeTab === 'lists'}
    <div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
      <div class="rounded-md border border-text/10 bg-crust/25 p-3">
        <h3 class="mb-2 text-xs font-semibold text-subtext1">Saved lists</h3>
        <div class="space-y-1">
          {#each lists as list, index (list.name)}
            <button
              type="button"
              class={`w-full rounded-md border px-2 py-1 text-left text-xs ${index === editingIndex ? 'border-blue/40 bg-blue/10 text-text' : 'border-transparent bg-text/[0.025] text-subtext1 hover:bg-text/5'}`}
              aria-label={`Edit run list ${list.name}`}
              onclick={() => selectList(index)}
            >{list.name}{NAME_SUFFIX}</button>
          {/each}
          {#if lists.length === 0}
            <p class="text-xs text-subtext0">No run lists are saved.</p>
          {/if}
        </div>
        <div class="mt-2 flex flex-wrap gap-1">
          <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1" onclick={newList}>New</button>
          <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1 disabled:opacity-40" disabled={draft === null} onclick={duplicate}>Duplicate</button>
          <button type="button" class="rounded-md bg-red/10 px-2 py-1 text-[11px] text-red disabled:opacity-40" disabled={draft === null} onclick={() => void deleteList()}>Delete</button>
        </div>
      </div>

      <div class="min-w-0 rounded-md border border-text/10 bg-crust/25 p-3">
        {#if draft !== null}
          <label class="block text-xs text-subtext1">
            Name
            <input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" value={draft.name} oninput={(event) => updateName(event.currentTarget.value)} />
          </label>
          <p class="mt-1 text-[11px] text-subtext0">Shown in the command list as "{draft.name}{NAME_SUFFIX}".</p>

          <h3 class="mt-3 text-xs font-semibold text-subtext1">Rows</h3>
          <div class="mt-1 space-y-1">
            {#each rows as item, index (index)}
              <button
                type="button"
                class={`w-full rounded-md border px-2 py-1 text-left text-xs ${index === selectedRow ? 'border-blue/40 bg-blue/10 text-text' : 'border-transparent bg-text/[0.025] text-subtext1 hover:bg-text/5'}`}
                aria-label={`Select row ${String(index + 1)}`}
                onclick={() => (selectedRow = index)}
              >{index + 1}. {item.type} · {item.name}{#if (item.repeat ?? 1) > 1} ×{item.repeat}{/if}{#if item.limit_minutes !== null && item.limit_minutes !== undefined} · {item.limit_minutes} min{/if}</button>
            {/each}
            {#if rows.length === 0}
              <p class="text-xs text-subtext0">No rows yet.</p>
            {/if}
          </div>
          <div class="mt-2 flex flex-wrap gap-1">
            <button type="button" class="rounded-md bg-blue/15 px-2 py-1 text-[11px] text-blue disabled:opacity-40" disabled={busy} onclick={addSelected}>Add selected command</button>
            <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1 disabled:opacity-40" disabled={selectedItem === null} onclick={() => moveRow(-1)}>Up</button>
            <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1 disabled:opacity-40" disabled={selectedItem === null} onclick={() => moveRow(1)}>Down</button>
            <button type="button" class="rounded-md bg-red/10 px-2 py-1 text-[11px] text-red disabled:opacity-40" disabled={selectedItem === null} onclick={removeRow}>Remove</button>
          </div>

          {#if selectedItem !== null}
            <div class="mt-3 grid gap-2 sm:grid-cols-2">
              <label class="text-xs text-subtext1">
                Repeat
                <input type="number" min="1" max="9999" class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label="Repeat" value={selectedItem.repeat ?? 1} oninput={(event) => updateRepeat(event.currentTarget.value)} />
              </label>
              <label class="text-xs text-subtext1">
                Limit (minutes, empty = wait until the command finishes)
                <input type="number" min="0" step="any" class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label="Limit (minutes)" value={selectedItem.limit_minutes ?? ''} oninput={(event) => updateLimit(event.currentTarget.value)} />
              </label>
            </div>
            {#if selectedItem.type === 'Mcu'}
              <p class="mt-1 text-[11px] text-subtext0">Mcu commands need a limit.</p>
            {/if}
          {/if}

          <h3 class="mt-3 text-xs font-semibold text-subtext1">List options</h3>
          <div class="mt-1 grid gap-2 sm:grid-cols-3">
            <label class="text-xs text-subtext1">
              Interval (seconds)
              <input type="number" min="0" max="3600" step="any" class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label="Interval (seconds)" value={draft.interval_seconds ?? ''} oninput={(event) => updateInterval(event.currentTarget.value)} />
            </label>
            <label class="text-xs text-subtext1">
              Loops (0 = until Stop)
              <input type="number" min="0" max="9999" class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label="Loops" value={draft.loops ?? ''} oninput={(event) => updateLoops(event.currentTarget.value)} />
            </label>
            <label class="text-xs text-subtext1">
              On error
              <select class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label="On error" value={draft.on_error ?? 'stop'} onchange={(event) => updateOnError(event.currentTarget.value)}>
                <option value="stop">Stop the list</option>
                <option value="next">Continue to the next command</option>
              </select>
            </label>
          </div>
        {:else}
          <p class="text-xs text-subtext0">No run list is selected. Press "New" to create one.</p>
        {/if}
      </div>
    </div>

    <div class="mt-3 flex flex-wrap items-center gap-2">
      {#if message !== null}
        <p class="text-xs text-subtext1" role="status">{message}</p>
      {/if}
      <button type="button" class="ml-auto rounded-lg bg-blue/15 px-4 py-2 text-sm font-medium text-blue disabled:opacity-40" disabled={busy || draft === null} onclick={() => void save()}>Save</button>
    </div>
    {:else}
    <div class="grid gap-3 sm:grid-cols-2">
      <div class="rounded-md border border-text/10 bg-crust/25 p-3">
        <h3 class="mb-2 text-xs font-semibold text-subtext1">Start reservation</h3>
        <p class="text-xs text-subtext0">Command: <span class="text-text">{selected === null ? '(no command selected)' : `[${selected.type}] ${selected.name}`}</span></p>
        <p class="mt-1 text-[11px] text-subtext0">Uses the command selected in the Commands tab; run lists can be selected too.</p>
        <div class="mt-2 space-y-1 text-xs text-subtext1">
          <label class="flex items-center gap-2">
            <input type="radio" name="schedule-start-mode" checked={startMode === 'clock'} onchange={() => (startMode = 'clock')} />
            At a time (hour:minute)
            <input class="w-20 rounded-md border border-text/15 bg-crust/40 px-2 py-0.5 text-sm text-text" aria-label="Start time" value={startClock} oninput={(event) => (startClock = event.currentTarget.value)} />
          </label>
          <label class="flex items-center gap-2">
            <input type="radio" name="schedule-start-mode" checked={startMode === 'minutes'} onchange={() => (startMode = 'minutes')} />
            After minutes
            <input class="w-20 rounded-md border border-text/15 bg-crust/40 px-2 py-0.5 text-sm text-text" aria-label="Start minutes" value={startMinutes} oninput={(event) => (startMinutes = event.currentTarget.value)} />
          </label>
        </div>
        <div class="mt-2 flex flex-wrap gap-1">
          <button type="button" class="rounded-md bg-blue/15 px-2 py-1 text-[11px] text-blue disabled:opacity-40" disabled={scheduleBusy} onclick={reserveStart}>Reserve start</button>
          <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1 disabled:opacity-40" aria-label="Cancel the start reservation" disabled={scheduleBusy || !(schedule?.has_start ?? false)} onclick={() => void controlSchedule({ action: 'cancel_start' })}>Cancel</button>
        </div>
        <p class="mt-2 text-xs text-subtext1" role="status">Now reserved: {schedule?.start_display ?? '予約はありません'}</p>
      </div>

      <div class="rounded-md border border-text/10 bg-crust/25 p-3">
        <h3 class="mb-2 text-xs font-semibold text-subtext1">Stop reservation</h3>
        <div class="mt-2 space-y-1 text-xs text-subtext1">
          <label class="flex items-center gap-2">
            <input type="radio" name="schedule-stop-mode" checked={stopMode === 'clock'} onchange={() => (stopMode = 'clock')} />
            At a time (hour:minute)
            <input class="w-20 rounded-md border border-text/15 bg-crust/40 px-2 py-0.5 text-sm text-text" aria-label="Stop time" value={stopClock} oninput={(event) => (stopClock = event.currentTarget.value)} />
          </label>
          <label class="flex items-center gap-2">
            <input type="radio" name="schedule-stop-mode" checked={stopMode === 'minutes'} onchange={() => (stopMode = 'minutes')} />
            After start (minutes)
            <input class="w-20 rounded-md border border-text/15 bg-crust/40 px-2 py-0.5 text-sm text-text" aria-label="Stop minutes" value={stopMinutes} oninput={(event) => (stopMinutes = event.currentTarget.value)} />
          </label>
        </div>
        <p class="mt-1 text-[11px] text-subtext0">"After start" counts from now while a command is running, or from the reserved start.</p>
        <div class="mt-2 flex flex-wrap gap-1">
          <button type="button" class="rounded-md bg-blue/15 px-2 py-1 text-[11px] text-blue disabled:opacity-40" disabled={scheduleBusy} onclick={reserveStop}>Reserve stop</button>
          <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1 disabled:opacity-40" aria-label="Cancel the stop reservation" disabled={scheduleBusy || !(schedule?.has_stop ?? false)} onclick={() => void controlSchedule({ action: 'cancel_stop' })}>Cancel</button>
        </div>
        <p class="mt-2 text-xs text-subtext1" role="status">Now reserved: {schedule?.stop_display ?? '予約はありません'}</p>
      </div>
    </div>
    <p class="mt-2 text-[11px] text-subtext0">Reservations stay after this window is closed; they are cleared when the application exits. If the PC is asleep at the reserved time, reservations will not work correctly.</p>
    {#if scheduleMessage !== null}
      <p class="mt-2 text-xs text-yellow" role="status">{scheduleMessage}</p>
    {/if}
    {#if scheduleError !== null}
      <p class="mt-2 text-xs text-red" role="alert">{scheduleError}</p>
    {/if}
    {/if}
  </section>
</div>
