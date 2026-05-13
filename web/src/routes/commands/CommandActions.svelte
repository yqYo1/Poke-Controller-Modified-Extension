<script lang="ts">
	import { api } from "$lib/api/client";

	let { selectedCommand = $bindable("") }: { selectedCommand?: string } = $props();

	let running = $state(false);
	let activeCommand = $state("");

	async function start() {
		if (!selectedCommand) return;
		try {
			await api.startCommand(selectedCommand);
			running = true;
			activeCommand = selectedCommand;
		} catch (e) {
			console.warn("Failed to start command:", e);
		}
	}

	async function stop() {
		try {
			await api.stopCommand();
			running = false;
			activeCommand = "";
		} catch (e) {
			console.warn("Failed to stop command:", e);
		}
	}
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<h3 class="mb-2 text-sm font-medium text-gray-200">コマンド操作</h3>
	<div class="flex gap-2">
		<button
			onclick={start}
			disabled={!selectedCommand || running}
			class="flex-1 rounded bg-green-600 px-3 py-2 text-xs font-medium text-white hover:bg-green-500 disabled:opacity-50"
		>
			{running ? "実行中..." : "開始"}
		</button>
		<button
			onclick={stop}
			disabled={!running}
			class="flex-1 rounded bg-red-600 px-3 py-2 text-xs font-medium text-white hover:bg-red-500 disabled:opacity-50"
		>
			停止
		</button>
	</div>
	{#if activeCommand}
		<p class="mt-2 text-xs text-gray-400">アクティブ: {activeCommand}</p>
	{/if}
</div>
