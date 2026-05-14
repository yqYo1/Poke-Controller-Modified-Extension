<script lang="ts">
	import SoftwareController from './SoftwareController.svelte';

	let output1Lines = $state<string[]>([]);
	let output2Lines = $state<string[]>([]);
	let output1Container: HTMLDivElement | undefined = $state();
	let output2Container: HTMLDivElement | undefined = $state();

	$effect(() => {
		if (output1Container) {
			output1Container.scrollTop = output1Container.scrollHeight;
		}
	});

	$effect(() => {
		if (output2Container) {
			output2Container.scrollTop = output2Container.scrollHeight;
		}
	});

	function clearOutput1() {
		output1Lines = [];
	}

	function clearOutput2() {
		output2Lines = [];
	}
</script>

<div class="right-panel">
	<!-- Output#1 -->
	<div class="tk-labelframe">
		<div class="tk-labelframe-label">Output#1</div>
		<div class="tk-labelframe-content">
			<div class="output-toolbar">
				<button class="tk-btn tk-btn-sm" onclick={clearOutput1}>Clear</button>
			</div>
			<div
				bind:this={output1Container}
				class="output-textarea"
			>
				{#if output1Lines.length === 0}
					<span class="text-gray-500 italic">[output #1]</span>
				{:else}
					{#each output1Lines as line (line)}
						<div class="output-line">{line}</div>
					{/each}
				{/if}
			</div>
		</div>
	</div>

	<!-- Output#2 -->
	<div class="tk-labelframe">
		<div class="tk-labelframe-label">Output#2</div>
		<div class="tk-labelframe-content">
			<div class="output-toolbar">
				<button class="tk-btn tk-btn-sm" onclick={clearOutput2}>Clear</button>
			</div>
			<div
				bind:this={output2Container}
				class="output-textarea"
			>
				{#if output2Lines.length === 0}
					<span class="text-gray-500 italic">[output #2]</span>
				{:else}
					{#each output2Lines as line (line)}
						<div class="output-line">{line}</div>
					{/each}
				{/if}
			</div>
		</div>
	</div>

	<!-- Software Controller -->
	<div class="tk-labelframe softcon-frame">
		<div class="tk-labelframe-label">Software-Controller</div>
		<div class="tk-labelframe-content softcon-content">
			<SoftwareController />
		</div>
	</div>
</div>

<style>
	.right-panel {
		display: flex;
		flex-direction: column;
		gap: 4px;
		height: 100%;
		min-width: 280px;
		max-width: 340px;
		border-left: 1px solid var(--color-border, #334155);
		background-color: var(--color-bg-secondary, #1e293b);
		padding: 4px;
	}

	.tk-labelframe {
		border: 2px solid var(--color-border, #334155);
		border-radius: 2px;
		background-color: var(--color-bg-card, #1e293b);
		position: relative;
		margin-top: 6px;
		display: flex;
		flex-direction: column;
	}

	.tk-labelframe-label {
		position: absolute;
		top: -10px;
		left: 8px;
		background-color: var(--color-bg-secondary, #1e293b);
		padding: 0 4px;
		font-size: 11px;
		font-weight: 600;
		color: var(--color-text-secondary, #94a3b8);
		font-family: 'Segoe UI', system-ui, sans-serif;
		z-index: 1;
	}

	.tk-labelframe-content {
		padding: 8px 4px 4px 4px;
		flex: 1;
		display: flex;
		flex-direction: column;
	}

	.output-toolbar {
		display: flex;
		justify-content: flex-end;
		padding: 1px 2px 3px 2px;
		gap: 4px;
	}

	.output-textarea {
		font-family: 'Cascadia Code', 'Consolas', 'Courier New', monospace;
		font-size: 11px;
		line-height: 1.4;
		color: var(--color-text-primary, #f1f5f9);
		background-color: var(--color-bg-primary, #0f172a);
		border: 1px solid var(--color-border-light, #1e293b);
		padding: 4px;
		min-height: 80px;
		max-height: 200px;
		overflow-y: auto;
		white-space: pre-wrap;
		word-break: break-all;
		flex: 1;
	}

	.output-line {
		padding: 0;
		margin: 0;
		font-size: 11px;
	}

	.softcon-frame {
		flex-shrink: 0;
	}

	.softcon-content {
		padding: 6px;
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.tk-btn {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		padding: 1px 8px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-tertiary, #334155);
		color: var(--color-text-primary, #f1f5f9);
		cursor: pointer;
		user-select: none;
	}

	.tk-btn:hover {
		background-color: var(--color-accent-hover, #3b82f6);
		color: #fff;
	}

	.tk-btn:active {
		background-color: var(--color-accent, #60a5fa);
	}

	.tk-btn-sm {
		font-size: 10px;
		padding: 0px 6px;
	}
</style>
