<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Tab {
		id: string;
		label: string;
		content?: Snippet;
	}

	let { tabs = [], activeTab = $bindable(''), class: className = '' }: {
		tabs: Tab[];
		activeTab: string;
		class?: string;
	} = $props();
</script>

<div class="tk-notebook {className}">
	<div class="tk-notebook-tabs">
		{#each tabs as tab (tab.id)}
			<button
				class="tk-notebook-tab"
				class:active={activeTab === tab.id}
				onclick={() => (activeTab = tab.id)}
				role="tab"
				aria-selected={activeTab === tab.id}
			>
				{tab.label}
			</button>
		{/each}
	</div>
	<div class="tk-notebook-content">
		{#each tabs as tab (tab.id)}
			{#if activeTab === tab.id && tab.content}
				<div role="tabpanel" class="tk-notebook-page">
					{@render tab.content()}
				</div>
			{/if}
		{/each}
	</div>
</div>

<style>
	.tk-notebook {
		display: flex;
		flex-direction: column;
		border: 2px solid var(--color-border, #334155);
		border-radius: 2px;
		background-color: var(--color-bg-card, #1e293b);
		overflow: hidden;
	}

	.tk-notebook-tabs {
		display: flex;
		background-color: var(--color-bg-tertiary, #334155);
		border-bottom: 1px solid var(--color-border, #475569);
		padding: 0;
		gap: 0;
		overflow-x: auto;
		flex-shrink: 0;
	}

	.tk-notebook-tab {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 12px;
		padding: 4px 14px;
		border: none;
		border-right: 1px solid var(--color-border, #475569);
		background-color: transparent;
		color: var(--color-text-secondary, #94a3b8);
		cursor: pointer;
		white-space: nowrap;
		user-select: none;
		transition: background-color 0.1s;
	}

	.tk-notebook-tab:hover {
		background-color: var(--color-bg-card, #1e293b);
		color: var(--color-text-primary, #f1f5f9);
	}

	.tk-notebook-tab.active {
		background-color: var(--color-bg-card, #1e293b);
		color: var(--color-text-primary, #f1f5f9);
		border-bottom: 2px solid var(--color-accent, #60a5fa);
		margin-bottom: -1px;
	}

	.tk-notebook-content {
		padding: 6px;
		overflow-y: auto;
		flex: 1;
		min-height: 100px;
	}

	.tk-notebook-page {
		height: 100%;
	}
</style>
