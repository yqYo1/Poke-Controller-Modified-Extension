<script lang="ts">
  import type {
    CommandTagCommand,
    CommandTagRequest,
    CommandTagResult,
    CommandTagSnapshot
  } from '../actions';

  interface TagActions {
    commandTags(): Promise<CommandTagSnapshot>;
    applyCommandTags(request: CommandTagRequest): Promise<CommandTagResult>;
  }

  interface Selection {
    readonly module_path: string;
    readonly class_name: string;
  }

  interface Props {
    actions: TagActions;
    selected: Selection | null;
    onclose: () => void;
    onsaved?: () => void;
  }

  const TAG_MAX_LENGTH = 50;

  let { actions, selected, onclose, onsaved }: Props = $props();

  let snapshot = $state<CommandTagSnapshot | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let note = $state<string | null>(null);
  let conflicts = $state<string[]>([]);
  let tab = $state<'commands' | 'tags'>('commands');
  let selectedKey = $state<string | null>(null);
  let draftTags = $state<string[]>([]);
  let newTag = $state('');
  let selectedTag = $state<string | null>(null);
  let renameTarget = $state('');
  let confirmingDelete = $state(false);
  let started = false;

  const commands = $derived(snapshot?.commands ?? []);
  const tags = $derived(snapshot?.tags ?? []);
  const current = $derived(
    commands.find((command) => key(command) === selectedKey) ?? null
  );
  const newTagProblem = $derived(tagFormatProblem(newTag, draftTags));
  const renameProblem = $derived(tagFormatProblem(renameTarget, []));
  const selectedTagInfo = $derived(tags.find((tag) => tag.name === selectedTag) ?? null);

  function key(command: Selection): string {
    return `${command.module_path}\u0000${command.class_name}`;
  }

  function tagFormatProblem(value: string, existing: string[]): string | null {
    const tag = value.trim();
    if (tag.length === 0) {
      return value.length > 0 ? 'タグを入力してください' : null;
    }
    if (Array.from(tag).length > TAG_MAX_LENGTH) {
      return `タグは${String(TAG_MAX_LENGTH)}文字以内で入力してください`;
    }
    if (tag.startsWith('@')) {
      return 'タグの先頭に「@」は使えません（ディレクトリ由来の自動タグ用）';
    }
    if (
      Array.from(tag).some((character) => {
        const code = character.codePointAt(0) ?? 0;
        return code < 0x20 || code === 0x7f;
      })
    ) {
      return 'タグに制御文字は使えません';
    }
    if (existing.includes(tag)) {
      return '同じタグが既にあります';
    }
    return null;
  }

  function begin(command: CommandTagCommand): void {
    selectedKey = key(command);
    draftTags = [...command.manual_tags];
    newTag = '';
  }

  $effect(() => {
    if (!started) {
      started = true;
      void load();
    }
  });

  async function load(): Promise<void> {
    loading = true;
    error = null;
    try {
      const loaded = await actions.commandTags();
      snapshot = loaded;
      const preferred =
        loaded.commands.find(
          (command) =>
            selected !== null &&
            command.module_path === selected.module_path &&
            command.class_name === selected.class_name
        ) ?? loaded.commands[0];
      if (preferred !== undefined) {
        begin(preferred);
      }
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'タグの読み込みに失敗しました';
    } finally {
      loading = false;
    }
  }

  function addTag(): void {
    const tag = newTag.trim();
    if (tagFormatProblem(tag, draftTags) !== null || tag.length === 0) {
      return;
    }
    draftTags = [...draftTags, tag];
    newTag = '';
  }

  function removeTag(tag: string): void {
    draftTags = draftTags.filter((existing) => existing !== tag);
  }

  function applyResult(result: CommandTagResult): void {
    snapshot = result.snapshot;
    conflicts = result.results
      .filter((outcome) => outcome.status === 'conflict' || outcome.status === 'error')
      .map((outcome) => {
        const reason = outcome.message ?? outcome.status;
        return `${outcome.class_name} (${outcome.relative_path}): ${reason}`;
      });
    if (result.changed > 0) {
      note = `${String(result.changed)} 件のコマンドを更新しました`;
      onsaved?.();
    } else if (result.conflicts > 0) {
      note = null;
    } else {
      note = '変更はありませんでした';
    }
    if (result.reload_error !== null) {
      error = `ファイルは更新しましたが、コマンド一覧の再読み込みに失敗しました: ${String(result.reload_error)}`;
    }
    if (current !== null) {
      draftTags = [...current.manual_tags];
    }
    if (selectedTag !== null && !result.snapshot.tags.some((tag) => tag.name === selectedTag)) {
      selectedTag = null;
      renameTarget = '';
      confirmingDelete = false;
    }
  }

  async function save(): Promise<void> {
    if (current === null || busy) {
      return;
    }
    busy = true;
    error = null;
    note = null;
    try {
      const result = await actions.applyCommandTags({
        action: 'set',
        edits: [
          {
            module_path: current.module_path,
            class_name: current.class_name,
            digest: current.digest,
            tags: draftTags
          }
        ]
      });
      applyResult(result);
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'タグの保存に失敗しました';
    } finally {
      busy = false;
    }
  }

  function selectTag(tag: string): void {
    selectedTag = tag;
    renameTarget = tag;
    confirmingDelete = false;
    note = null;
    conflicts = [];
  }

  async function renameTag(): Promise<void> {
    if (selectedTag === null || busy) {
      return;
    }
    const from = selectedTag;
    const to = renameTarget.trim();
    if (renameProblem !== null || to.length === 0 || to === from) {
      return;
    }
    busy = true;
    error = null;
    note = null;
    try {
      const result = await actions.applyCommandTags({ action: 'rename', from, to });
      selectedTag = to;
      applyResult(result);
      renameTarget = to;
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'タグの変更に失敗しました';
    } finally {
      busy = false;
    }
  }

  async function deleteTag(): Promise<void> {
    if (selectedTag === null || busy) {
      return;
    }
    if (!confirmingDelete) {
      confirmingDelete = true;
      return;
    }
    const tag = selectedTag;
    busy = true;
    error = null;
    note = null;
    try {
      const result = await actions.applyCommandTags({ action: 'delete', tag });
      selectedTag = null;
      renameTarget = '';
      confirmingDelete = false;
      applyResult(result);
    } catch (failure) {
      error = failure instanceof Error ? failure.message : 'タグの削除に失敗しました';
    } finally {
      busy = false;
    }
  }
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" role="presentation">
  <section
    aria-label="Command tag editor"
    class="max-h-[90vh] w-full max-w-4xl overflow-auto rounded-lg border border-text/15 bg-base p-4 shadow-xl"
  >
    <header class="mb-3 flex items-center justify-between">
      <h2 class="text-sm font-semibold text-text">Command tag editor</h2>
      <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1" onclick={onclose}>Close</button>
    </header>

    <div class="mb-3 flex gap-1" role="tablist" aria-label="Command tag editor tabs">
      <button type="button" role="tab" aria-selected={tab === 'commands'} class={`rounded-md px-3 py-1 text-xs font-medium ${tab === 'commands' ? 'bg-blue/15 text-blue' : 'bg-text/5 text-subtext1'}`} onclick={() => (tab = 'commands')}>Commands</button>
      <button type="button" role="tab" aria-selected={tab === 'tags'} class={`rounded-md px-3 py-1 text-xs font-medium ${tab === 'tags' ? 'bg-blue/15 text-blue' : 'bg-text/5 text-subtext1'}`} onclick={() => (tab = 'tags')}>Bulk tags</button>
    </div>

    {#if error !== null}
      <p class="mb-3 rounded-md bg-red/10 px-3 py-2 text-xs text-red" role="alert">{error}</p>
    {/if}
    {#if note !== null}
      <p class="mb-3 rounded-md bg-green/10 px-3 py-2 text-xs text-green" role="status">{note}</p>
    {/if}
    {#if conflicts.length > 0}
      <div class="mb-3 rounded-md bg-yellow/10 px-3 py-2 text-xs text-yellow" role="alert">
        <p class="font-semibold">書き込み競合または失敗がありました</p>
        <ul class="mt-1 list-disc pl-4">
          {#each conflicts as conflict (conflict)}<li>{conflict}</li>{/each}
        </ul>
      </div>
    {/if}

    {#if loading}
      <p class="text-xs text-subtext0">Loading command tags…</p>
    {:else if tab === 'commands'}
      <div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
        <div class="rounded-md border border-text/10 bg-crust/25 p-3">
          <h3 class="mb-2 text-xs font-semibold text-subtext1">Commands</h3>
          <div class="space-y-1">
            {#each commands as command (key(command))}
              <button
                type="button"
                class={`w-full rounded-md border px-2 py-1 text-left text-xs ${key(command) === selectedKey ? 'border-blue/40 bg-blue/10 text-text' : 'border-transparent bg-text/[0.025] text-subtext1 hover:bg-text/5'}`}
                aria-label={`Edit tags for ${command.name}`}
                onclick={() => begin(command)}
              >
                {command.name}
                <span class="block break-all text-[10px] text-subtext0">{command.relative_path}</span>
                {#if command.error !== null}
                  <span class="block text-[10px] text-red">{command.error}</span>
                {/if}
              </button>
            {/each}
            {#if commands.length === 0}
              <p class="text-xs text-subtext0">No commands were discovered.</p>
            {/if}
          </div>
        </div>

        <div class="min-w-0 rounded-md border border-text/10 bg-crust/25 p-3">
          {#if current !== null}
            <h3 class="text-xs font-semibold text-subtext1">{current.name}</h3>
            <p class="mt-1 break-all text-[11px] text-subtext0">{current.module_path}</p>

            <h4 class="mt-3 text-xs font-semibold text-subtext1">Manual tags</h4>
            <div class="mt-1 flex flex-wrap gap-1">
              {#each draftTags as tag (tag)}
                <span class="inline-flex items-center gap-1 rounded bg-blue/15 px-2 py-0.5 text-[11px] text-blue">
                  {tag}
                  <button type="button" class="text-blue/70" aria-label={`Remove tag ${tag}`} onclick={() => removeTag(tag)}>×</button>
                </span>
              {/each}
              {#if draftTags.length === 0}
                <span class="text-[11px] text-subtext0">No manual tags.</span>
              {/if}
            </div>

            <div class="mt-2 flex flex-wrap items-end gap-2">
              <label class="text-xs text-subtext1">
                New tag
                <input class="mt-1 w-56 rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label="New tag" value={newTag} oninput={(event) => (newTag = event.currentTarget.value)} onkeydown={(event) => { if (event.key === 'Enter') { event.preventDefault(); addTag(); } }} />
              </label>
              <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1 disabled:opacity-40" disabled={newTag.trim().length === 0 || newTagProblem !== null} onclick={addTag}>Add</button>
            </div>
            {#if newTagProblem !== null}
              <p class="mt-1 text-[11px] text-yellow" role="alert">{newTagProblem}</p>
            {/if}

            {#if current.automatic_tags.length > 0}
              <h4 class="mt-3 text-xs font-semibold text-subtext1">Automatic tags</h4>
              <div class="mt-1 flex flex-wrap gap-1">
                {#each current.automatic_tags as tag (tag)}
                  <span class="rounded bg-text/5 px-2 py-0.5 text-[11px] text-subtext0">{tag}</span>
                {/each}
              </div>
              <p class="mt-1 text-[11px] text-subtext0">「@」で始まるタグはディレクトリ名に由来するため、ここでは変更できません。</p>
            {/if}

            <div class="mt-3 flex items-center gap-2">
              <button type="button" class="rounded-md bg-green/15 px-3 py-1 text-xs font-medium text-green disabled:opacity-40" disabled={busy} onclick={() => void save()}>Save tags</button>
              {#if current.digest.length === 0}
                <span class="text-[11px] text-red">このファイルは読み込めないため保存できません</span>
              {/if}
            </div>
          {:else}
            <p class="text-xs text-subtext0">Select a command.</p>
          {/if}
        </div>
      </div>
    {:else}
      <div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
        <div class="rounded-md border border-text/10 bg-crust/25 p-3">
          <h3 class="mb-2 text-xs font-semibold text-subtext1">Tags</h3>
          <div class="space-y-1">
            {#each tags as tag (tag.name)}
              <button
                type="button"
                class={`w-full rounded-md border px-2 py-1 text-left text-xs ${tag.name === selectedTag ? 'border-blue/40 bg-blue/10 text-text' : 'border-transparent bg-text/[0.025] text-subtext1 hover:bg-text/5'}`}
                aria-label={`Select tag ${tag.name}`}
                onclick={() => selectTag(tag.name)}
              >
                {tag.name}
                <span class="ml-1 text-[10px] text-subtext0">×{tag.count}</span>
                {#if tag.manual}<span class="ml-1 rounded bg-blue/15 px-1 text-[10px] text-blue">manual</span>{/if}
                {#if tag.automatic}<span class="ml-1 rounded bg-text/5 px-1 text-[10px] text-subtext0">auto</span>{/if}
              </button>
            {/each}
            {#if tags.length === 0}
              <p class="text-xs text-subtext0">No tags.</p>
            {/if}
          </div>
        </div>

        <div class="min-w-0 rounded-md border border-text/10 bg-crust/25 p-3">
          {#if selectedTagInfo !== null}
            <h3 class="text-xs font-semibold text-subtext1">{selectedTagInfo.name}</h3>
            <p class="mt-1 text-[11px] text-subtext0">
              {#if selectedTagInfo.manual}手動タグとして {String(selectedTagInfo.count)} 件のコマンドで使われています。{:else}このタグは自動タグのみです（ディレクトリ名に由来）。{/if}
            </p>

            {#if selectedTagInfo.manual}
              <div class="mt-3 flex flex-wrap items-end gap-2">
                <label class="text-xs text-subtext1">
                  Rename to
                  <input class="mt-1 w-56 rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" aria-label="Rename tag to" value={renameTarget} oninput={(event) => (renameTarget = event.currentTarget.value)} />
                </label>
                <button type="button" class="rounded-md bg-blue/15 px-2 py-1 text-[11px] text-blue disabled:opacity-40" disabled={busy || renameTarget.trim().length === 0 || renameTarget.trim() === selectedTagInfo.name || renameProblem !== null} onclick={() => void renameTag()}>Rename</button>
                <button type="button" class="rounded-md bg-red/10 px-2 py-1 text-[11px] text-red disabled:opacity-40" disabled={busy} onclick={() => void deleteTag()}>{confirmingDelete ? 'Confirm delete' : 'Delete'}</button>
              </div>
              {#if renameProblem !== null}
                <p class="mt-1 text-[11px] text-yellow" role="alert">{renameProblem}</p>
              {/if}
              {#if confirmingDelete}
                <p class="mt-1 text-[11px] text-yellow" role="alert">もう一度 Delete を押すと、すべてのコマンドからこのタグを削除します。</p>
              {/if}
            {:else}
              <p class="mt-2 text-[11px] text-subtext0">自動タグを変更するには、コマンドのフォルダ名を変更してください。</p>
            {/if}
          {:else}
            <p class="text-xs text-subtext0">Select a tag.</p>
          {/if}
        </div>
      </div>
    {/if}
  </section>
</div>
