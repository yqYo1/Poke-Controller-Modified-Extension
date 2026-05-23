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
	let filterTag = $state('');

	// Derive unique tags from command names (since API doesn't return tags directly)
	// Use prefix-based tagging: "action_", "mcu_", etc.
	let availableTags = $derived.by<string[]>(() => {
		const tags = ['']; // Use array instead of Set for Svelte reactivity
		for (const cmd of allCommands) {
			const tag = deriveTag(cmd.name);
			if (tag && !tags.includes(tag)) tags.push(tag);
		}
		return tags.sort();
	});

	function deriveTag(name: string): string {
		if (name.startsWith('mcu_')) return 'MCU';
		if (name.startsWith('action_') || name.startsWith('cmd_')) return 'Action';
		if (name.startsWith('test_')) return 'Test';
		if (name.startsWith('util_') || name.startsWith('tool_')) return 'Utility';
		return '';
	}

	// ── Load commands ────────────────────────────────────────────────────────
	async function loadCommands() {
		loading = true;
		try {
			// Python commands: everything that isn't mcu_
			const list = await api.getCommands();
			allCommands = list.filter((c) => !c.name.startsWith('mcu_'));
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
		let result = allCommands;

		// Tag filter
		if (filterTag) {
			result = result.filter((c) => deriveTag(c.name) === filterTag);
		}

		// Text filter
		if (filterText) {
			const lower = filterText.toLowerCase();
			result = result.filter(
				(c) =>
					c.name.toLowerCase().includes(lower) ||
					(c.description ?? '').toLowerCase().includes(lower),
			);
		}

		filteredCommands = result;
	});

	function handleSelect(name: string) {
		selectedName = name;
		onSelect?.(name);
		api.loadCommand(name).catch(console.warn);
	}
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<h3 class="mb-2 text-sm font-medium text-gray-200">Python コマンド</h3>

	<!-- Filter bar -->
	<div class="filter-bar">
		<select
			class="tk-select-xs"
			bind:value={filterTag}
		>
			<option value="">全てのタグ</option>
			{#each availableTags as tag (tag)}
				{#if tag}
					<option value={tag}>{tag}</option>
				{/if}
			{/each}
		</select>
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
			<p class="text-xs text-gray-500">コマンドが見つかりません</p>
		{:else}
			{#each filteredCommands as cmd (cmd.name)}
				<button
					class="command-item"
					class:selected={selectedName === cmd.name}
					onclick={() => handleSelect(cmd.name)}
				>
					<span class="cmd-top">
						<span class="cmd-name">{cmd.name}</span>
						<span class="cmd-tag">{deriveTag(cmd.name)}</span>
					</span>
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

	:global(.tk-select-xs) {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 10px;
		padding: 1px 2px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-primary, #0f172a);
		color: var(--color-text-primary, #f1f5f9);
		flex: 1;
		min-width: 60px;
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

	.cmd-top {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.cmd-tag {
		font-size: 8px;
		font-weight: 600;
		padding: 1px 4px;
		border-radius: 3px;
		background-color: var(--color-accent, #3b82f6);
		color: #fff;
		white-space: nowrap;
		line-height: 1.2;
		text-transform: uppercase;
	}

	.command-item.selected .cmd-tag {
		background-color: rgba(255, 255, 255, 0.25);
	}
</style>
