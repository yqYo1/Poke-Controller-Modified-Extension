<script lang="ts">
	import { onMount } from "svelte";

	let { onSelect }: { onSelect?: (name: string) => void } = $props();
	let commands = $state<string[]>([]);
	let loading = $state(true);

	onMount(async () => {
		try {
			const { api } = await import("$lib/api/client");
			const list = await api.getCommands();
			commands = list.filter((c) => c.name.startsWith("mcu_")).map((c) => c.name);
		} catch { /* ignore */ }
		loading = false;
	});
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<h3 class="mb-2 text-sm font-medium text-gray-200">MCU コマンド</h3>
	{#if loading}
		<p class="text-xs text-gray-500">読み込み中...</p>
	{:else if commands.length === 0}
		<p class="text-xs text-gray-500">コマンドが見つかりません</p>
	{:else}
		<div class="max-h-48 space-y-1 overflow-y-auto">
			{#each commands as cmd (cmd)}
				<button
					onclick={() => onSelect?.(cmd)}
					class="w-full rounded px-2 py-1 text-left text-xs text-gray-300 hover:bg-gray-700"
				>
					{cmd}
				</button>
			{/each}
		</div>
	{/if}
</div>
