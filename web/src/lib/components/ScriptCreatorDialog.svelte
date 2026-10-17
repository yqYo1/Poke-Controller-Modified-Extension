<script lang="ts">
  import type {
    ScriptCreatorPreview,
    ScriptCreatorResult,
    ScriptCreatorSnapshot,
    ScriptCreatorSpec
  } from '../actions';

  interface ScriptCreatorActions {
    createScript(request: ScriptCreatorSpec): Promise<ScriptCreatorResult>;
    previewScriptCreation(request: ScriptCreatorSpec): Promise<ScriptCreatorPreview>;
    scriptCreatorSnapshot(): Promise<ScriptCreatorSnapshot>;
  }

  interface Props {
    actions: ScriptCreatorActions;
    onclose: () => void;
  }

  let { actions, onclose }: Props = $props();
  let author = $state('');
  let busy = $state<'create' | 'preview' | null>(null);
  let error = $state<string | null>(null);
  let file_name = $state('');
  let folder = $state('');
  let name = $state('');
  let preview = $state<readonly string[]>([]);
  let result = $state<ScriptCreatorResult | null>(null);
  let snapshot = $state<ScriptCreatorSnapshot | null>(null);
  let template_folder = $state('');
  let template_location = $state<'template' | 'script'>('template');
  let use_bridge = $state(false);

  $effect(() => {
    void load();
  });

  async function load(): Promise<void> {
    try {
      const loaded = await actions.scriptCreatorSnapshot();
      snapshot = loaded;
      author = loaded.author;
    } catch (reason: unknown) {
      error = message(reason);
    }
  }

  function spec(): ScriptCreatorSpec {
    return {
      author,
      file_name,
      folder,
      name,
      template_folder,
      template_location,
      use_bridge
    };
  }

  function message(reason: unknown): string {
    return reason instanceof Error ? reason.message : 'Script operation failed';
  }

  async function run(kind: 'create' | 'preview'): Promise<void> {
    busy = kind;
    error = null;
    if (kind === 'preview') preview = [];
    try {
      if (kind === 'preview') {
        const loaded = await actions.previewScriptCreation(spec());
        preview = loaded.lines;
        if (loaded.error) error = loaded.error;
      } else {
        result = await actions.createScript(spec());
      }
    } catch (reason: unknown) {
      error = message(reason);
    } finally {
      busy = null;
    }
  }

  const scriptLocationLocked = $derived(!use_bridge);
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" role="presentation">
  <section
    aria-label="Create a new script"
    class="max-h-[90vh] w-full max-w-2xl overflow-auto rounded-lg border border-text/15 bg-base p-4 shadow-xl"
  >
    <header class="mb-3 flex items-center justify-between">
      <h2 class="text-sm font-semibold text-text">Create a new script</h2>
      <button type="button" class="rounded-md bg-text/5 px-2 py-1 text-[11px] text-subtext1" onclick={onclose}>Close</button>
    </header>

    {#if error !== null}
      <p class="mb-3 rounded-md bg-red/10 px-3 py-2 text-xs text-red" role="alert">{error}</p>
    {/if}

    <div class="grid gap-3 sm:grid-cols-2">
      <label class="text-xs text-subtext1">
        Script name
        <input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" bind:value={name} />
      </label>
      <label class="text-xs text-subtext1">
        File name (letters, digits, - and _)
        <input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" bind:value={file_name} />
      </label>
      <label class="text-xs text-subtext1">
        Folder (inside Commands/PythonCommands)
        <input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" bind:value={folder} list="script-creator-folders" />
        <datalist id="script-creator-folders">
          {#each snapshot?.script_folders ?? [] as choice (choice)}
            <option value={choice}></option>
          {/each}
        </datalist>
      </label>
      <label class="text-xs text-subtext1">
        Author
        <input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" bind:value={author} />
      </label>
      <label class="text-xs text-subtext1">
        Image folder (optional)
        <input class="mt-1 w-full rounded-md border border-text/15 bg-crust/40 px-2 py-1 text-sm text-text" bind:value={template_folder} list="script-creator-template-folders" />
        <datalist id="script-creator-template-folders">
          {#each snapshot?.template_folders ?? [] as choice (choice)}
            <option value={choice}></option>
          {/each}
        </datalist>
      </label>
      <fieldset class="text-xs text-subtext1">
        <legend>Image folder location</legend>
        <label class="mt-1 flex items-center gap-2">
          <input type="radio" value="template" bind:group={template_location} disabled={scriptLocationLocked} />
          Template folder
        </label>
        <label class="mt-1 flex items-center gap-2">
          <input type="radio" value="script" bind:group={template_location} disabled={scriptLocationLocked} />
          Next to the script (requires BridgeFunctions)
        </label>
      </fieldset>
      <label class="flex items-center gap-2 text-xs text-subtext1 sm:col-span-2">
        <input type="checkbox" bind:checked={use_bridge} disabled={!(snapshot?.bridge_available ?? false)} />
        Use BridgeFunctions
        {#if !(snapshot?.bridge_available ?? false)}
          <span class="text-subtext0">(bridge_functions is not installed)</span>
        {/if}
      </label>
    </div>

    <div class="mt-4 flex gap-2">
      <button
        type="button"
        class="rounded-lg bg-text/10 px-4 py-2 text-sm font-medium text-text disabled:opacity-40"
        disabled={busy !== null}
        onclick={() => void run('preview')}
      >
        Preview
      </button>
      <button
        type="button"
        class="rounded-lg bg-blue/15 px-4 py-2 text-sm font-medium text-blue disabled:opacity-40"
        disabled={busy !== null}
        onclick={() => void run('create')}
      >
        Create
      </button>
    </div>

    {#if preview.length > 0}
      <section class="mt-4 rounded-md border border-text/10 bg-crust/25 p-3" aria-label="Preview">
        <h3 class="mb-1 text-xs font-semibold text-subtext1">Preview</h3>
        {#each preview as line (line)}
          <p class="font-mono text-xs text-subtext1">{line}</p>
        {/each}
      </section>
    {/if}

    {#if result !== null}
      <section class="mt-4 rounded-md border border-text/10 bg-crust/25 p-3" aria-label="Result">
        <h3 class="mb-1 text-xs font-semibold text-subtext1">Created</h3>
        <p class="font-mono text-xs text-subtext1">{result.path}</p>
        <p class="font-mono text-xs text-subtext1">{result.command_name}</p>
        {#if result.created_files.length > 0}
          <p class="mt-1 text-xs text-subtext0">Created: {result.created_files.join(', ')}</p>
        {/if}
        {#if result.skipped_files.length > 0}
          <p class="mt-1 text-xs text-subtext0">Kept (already present): {result.skipped_files.join(', ')}</p>
        {/if}
      </section>
    {/if}
  </section>
</div>
