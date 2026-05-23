<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import type { CommandEntry } from '$lib/api/client';

	interface Props {
		onSelect?: (name: string) => void;
	}

	let { onSelect }: Props = $props();

	// ── State ────────────────────────────────────────────────────────────────
	let allCommands = $state<CommandEntry[]>([]);
	let filteredCommands = $state<CommandEntry[]>([]);
	let loading = $state(true);
	let selectedName = $state<string | null>(null);
	let filterText = $state('');

	// ── Load commands ────────────────────────────────────────────────────────
	async function loadCommands() {
		loading = true;
		try {
			const list = await api.getCommands();
			// MCU commands: anything starting with "mcu_"
			allCommands = list.filter((c) => c.name.startsWith('mcu_'));
		} catch {
			// ignore
		}
		loading = false;
	}

	onMount(() => {
		loadCommands();
	});

	// Reactive filter
	$effect(() => {
		if (!filterText) {
			filteredCommands = allCommands;
			return;
		}
		const lower = filterText.toLowerCase();
		filteredCommands = allCommands.filter(
			(c) =>
				c.name.toLowerCase().includes(lower) ||
				(c.description ?? '').toLowerCase().includes(lower),
		);
	});

	function handleSelect(name: string) {
		selectedName = name;
		onSelect?.(name);
		api.loadCommand(name).catch(console.warn);
	}
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<h3 class="mb-2 text-sm font-medium text-gray-200">MCU コマンド</h3>

	<!-- Filter bar -->
	<div class="filter-bar">
		<input
			type="text"
			class="tk-input-xs"
			placeholder="検索..."
			bind:value={filterText}
		/>
	</div>

	<!-- Command list -->
	<div class="command-list">
		{#if loading}
			<p class="text-xs text-gray-500">読み込み中...</p>
		{:else if filteredCommands.length === 0}
			<p class="text-xs text-gray-500">MCUコマンドが見つかりません</p>
		{:else}
			{#each filteredCommands as cmd (cmd.name)}
				<button
					class="command-item"
					class:selected={selectedName === cmd.name}
					onclick={() => handleSelect(cmd.name)}
				>
					<span class="cmd-name">{cmd.name}</span>
					{#if cmd.description}
						<span class="cmd-desc">{cmd.description}</span>
					{/if}
				</button>
			{/each}
		{/if}
	</div>
</div>

<style>
	.filter-bar {
		display: flex;
		gap: 4px;
		margin-bottom: 4px;
	}

	:global(.tk-input-xs) {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 10px;
		padding: 1px 4px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-primary, #0f172a);
		color: var(--color-text-primary, #f1f5f9);
		flex: 1;
		min-width: 80px;
	}

	.command-list {
		max-height: 200px;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 1px;
	}

	.command-item {
		display: flex;
		flex-direction: column;
		gap: 1px;
		width: 100%;
		padding: 3px 6px;
		border: 1px solid transparent;
		border-radius: 2px;
		background-color: transparent;
		color: var(--color-text-secondary, #94a3b8);
		cursor: pointer;
		text-align: left;
		font-family: inherit;
		transition: background-color 0.1s;
	}

	.command-item:hover {
		background-color: var(--color-bg-tertiary, #334155);
		color: var(--color-text-primary, #f1f5f9);
	}

	.command-item.selected {
		background-color: var(--color-accent, #3b82f6);
		border-color: var(--color-accent-hover, #60a5fa);
		color: #fff;
	}

	.cmd-name {
		font-size: 11px;
		font-weight: 500;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.cmd-desc {
		font-size: 9px;
		color: var(--color-text-secondary, #64748b);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.command-item.selected .cmd-desc {
		color: #bfdbfe;
	}
</style>
