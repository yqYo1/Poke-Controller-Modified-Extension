<script lang="ts">
	import SoftwareController from './SoftwareController.svelte';
	import { uiState, WIDGET_MODES } from '$lib/stores/ui.svelte.ts';
	import { onMount } from 'svelte';
	import { wsClient } from '$lib/api/websocket';
	import type { LogMessage, LogLevel } from '$lib/api/websocket';

	let currentWidgetConfig = $derived(WIDGET_MODES[uiState.widgetMode] ?? WIDGET_MODES[1]);

	// ─── Structured log entries (preserve level info) ──────────────────────
	interface OutputEntry {
		line: string;
		level: LogLevel | 'log';
	}

	let output1Entries = $state<OutputEntry[]>([]);
	let output2Entries = $state<OutputEntry[]>([]);

	let output1Container: HTMLDivElement | undefined = $state();
	let output2Container: HTMLDivElement | undefined = $state();

	let filter1 = $state<'all' | LogLevel>('all');
	let filter2 = $state<'all' | LogLevel>('all');

	const filterLevels: { value: 'all' | LogLevel; label: string }[] = [
		{ value: 'all', label: 'ALL' },
		{ value: 'debug', label: 'DEBUG' },
		{ value: 'info', label: 'INFO' },
		{ value: 'warn', label: 'WARN' },
		{ value: 'error', label: 'ERROR' },
	];

	const filtered1 = $derived(
		filter1 === 'all'
			? output1Entries
			: output1Entries.filter((e) => e.level === filter1),
	);
	const filtered2 = $derived(
		filter2 === 'all'
			? output2Entries
			: output2Entries.filter((e) => e.level === filter2),
	);

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
		output1Entries = [];
	}

	function clearOutput2() {
		output2Entries = [];
	}

	async function copyOutput1() {
		const text = filtered1.map((e) => e.line).join('\n');
		if (text) {
			await navigator.clipboard.writeText(text);
		}
	}

	async function copyOutput2() {
		const text = filtered2.map((e) => e.line).join('\n');
		if (text) {
			await navigator.clipboard.writeText(text);
		}
	}

	function handleClearOutputs(e: Event) {
		const detail = (e as CustomEvent).detail;
		if (detail?.panel === 1) {
			clearOutput1();
		} else if (detail?.panel === 2) {
			clearOutput2();
		} else {
			clearOutput1();
			clearOutput2();
		}
	}

	onMount(() => {
		window.addEventListener('clear-outputs', handleClearOutputs);

		const handleMessage = (msg: LogMessage) => {
			if (msg.type === 'log') {
				const line = `[${msg.timestamp}] [${msg.level}] ${msg.message}`;
				const entry: OutputEntry = { line, level: msg.level };
				if (uiState.stdoutDestination === 1) {
					output1Entries = [...output1Entries, entry];
				} else {
					output2Entries = [...output2Entries, entry];
				}
			}
		};

		wsClient.on('message', handleMessage);

		return () => {
			window.removeEventListener('clear-outputs', handleClearOutputs);
			wsClient.off('message', handleMessage);
		};
	});
</script>

<div class="right-panel">
	{#if uiState.controllerPosition === 'top' && currentWidgetConfig.showController}
		<!-- Software Controller (top) -->
		<div class="tk-labelframe softcon-frame">
			<div class="tk-labelframe-label">Software-Controller</div>
			<div class="tk-labelframe-content softcon-content">
				<SoftwareController />
			</div>
		</div>
	{/if}

	{#if currentWidgetConfig.showOutput1}
		<!-- Output#1 -->
		<div
			class="tk-labelframe"
			style="flex: {uiState.splitRatio} 1 0%"
		>
			<div class="tk-labelframe-label">Output#1</div>
			<div class="tk-labelframe-content">
				<div class="output-toolbar">
					<select
						bind:value={filter1}
						class="tk-select tk-select-sm"
					>
						{#each filterLevels as fl (fl.value)}
							<option value={fl.value}>{fl.label}</option>
						{/each}
					</select>
					<button class="tk-btn tk-btn-sm" onclick={copyOutput1}>Copy</button>
					<button class="tk-btn tk-btn-sm" onclick={clearOutput1}>Clear</button>
				</div>
				<div
					bind:this={output1Container}
					class="output-textarea"
				>
					{#if filtered1.length === 0}
						<span class="text-gray-500 italic">[output #1]</span>
					{:else}
						{#each filtered1 as entry (entry.line)}
							<div class="output-line">{entry.line}</div>
						{/each}
					{/if}
				</div>
			</div>
		</div>
	{/if}

	{#if currentWidgetConfig.showOutput2}
		<!-- Output#2 -->
		<div
			class="tk-labelframe"
			style="flex: {100 - uiState.splitRatio} 1 0%"
		>
			<div class="tk-labelframe-label">Output#2</div>
			<div class="tk-labelframe-content">
				<div class="output-toolbar">
					<select
						bind:value={filter2}
						class="tk-select tk-select-sm"
					>
						{#each filterLevels as fl (fl.value)}
							<option value={fl.value}>{fl.label}</option>
						{/each}
					</select>
					<button class="tk-btn tk-btn-sm" onclick={copyOutput2}>Copy</button>
					<button class="tk-btn tk-btn-sm" onclick={clearOutput2}>Clear</button>
				</div>
				<div
					bind:this={output2Container}
					class="output-textarea"
				>
					{#if filtered2.length === 0}
						<span class="text-gray-500 italic">[output #2]</span>
					{:else}
						{#each filtered2 as entry (entry.line)}
							<div class="output-line">{entry.line}</div>
						{/each}
					{/if}
				</div>
			</div>
		</div>
	{/if}

	{#if uiState.controllerPosition === 'bottom' && currentWidgetConfig.showController}
		<!-- Software Controller (bottom) -->
		<div class="tk-labelframe softcon-frame">
			<div class="tk-labelframe-label">Software-Controller</div>
			<div class="tk-labelframe-content softcon-content">
				<SoftwareController />
			</div>
		</div>
	{/if}
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

	.tk-select {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		padding: 1px 4px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-tertiary, #334155);
		color: var(--color-text-primary, #f1f5f9);
		cursor: pointer;
		user-select: none;
		outline: none;
	}

	.tk-select:hover {
		border-color: var(--color-accent-hover, #3b82f6);
	}

	.tk-select-sm {
		font-size: 10px;
		padding: 0px 4px;
	}
</style>
