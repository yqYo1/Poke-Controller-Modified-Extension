<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';

	let commands = $state<{ name: string; description?: string }[]>([]);
	let filteredCommands = $state<{ name: string; description?: string }[]>([]);
	let filterText = $state('');
	let selectedCommand = $state('');
	let running = $state(false);

	onMount(() => {
		refreshCommands();
		api.getActiveCommand().then((r) => {
			running = r.running;
		}).catch(() => {});
	});

	async function refreshCommands() {
		try {
			commands = await api.getCommands();
			filteredCommands = commands;
		} catch { commands = []; }
	}

	function applyFilter() {
		if (!filterText.trim()) {
			filteredCommands = commands;
		} else {
			api.filterCommands(filterText).then((r) => {
				filteredCommands = r;
			}).catch(() => {});
		}
	}

	function selectCommand(name: string) {
		selectedCommand = name;
		api.loadCommand(name).catch(console.warn);
	}

	async function startSelected() {
		if (!selectedCommand) return;
		await api.startCommand(selectedCommand);
		running = true;
	}

	async function stopRunning() {
		await api.stopCommand();
		running = false;
	}
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">コマンド</h2>

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<div class="mb-3 flex items-center gap-2">
				<input
					bind:value={filterText}
					oninput={applyFilter}
					placeholder="フィルター..."
					class="flex-1 rounded bg-gray-800 px-2 py-1 text-xs text-gray-200 placeholder-gray-500"
				/>
				<button onclick={refreshCommands} class="rounded bg-gray-700 px-2 py-1 text-xs text-gray-300 hover:bg-gray-600">再読み込み</button>
			</div>
			<div class="max-h-60 overflow-y-auto">
				{#each filteredCommands as cmd (cmd.name)}
					<button
						onclick={() => selectCommand(cmd.name)}
						class="w-full rounded px-2 py-1 text-left text-xs transition-colors {selectedCommand === cmd.name ? 'bg-blue-700 text-white' : 'text-gray-300 hover:bg-gray-700'}"
					>
						<div class="font-medium">{cmd.name}</div>
						{#if cmd.description}
							<div class="text-[10px] text-gray-500">{cmd.description}</div>
						{/if}
					</button>
				{/each}
			</div>
		</div>

		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">実行制御</h3>
			<div class="space-y-2">
				<div class="text-xs text-gray-400">
					選択中: <span class="text-gray-200">{selectedCommand || 'なし'}</span>
				</div>
				<div class="text-xs text-gray-400">
					状態: <span class={running ? 'text-green-400' : 'text-gray-500'}>{running ? '実行中' : '停止中'}</span>
				</div>
				{#if running}
					<button onclick={stopRunning} class="w-full rounded bg-red-600 px-3 py-2 text-sm font-medium text-white hover:bg-red-500">
						停止
					</button>
				{:else}
					<button onclick={startSelected} disabled={!selectedCommand} class="w-full rounded bg-green-700 px-3 py-2 text-sm font-medium text-white hover:bg-green-600 disabled:opacity-50">
						開始
					</button>
				{/if}
			</div>
		</div>
	</div>

	<OutputPanel title="コマンド出力" />
	<LogPanel />
</div>
