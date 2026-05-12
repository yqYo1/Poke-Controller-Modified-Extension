<script lang="ts">
	interface LogEntry {
		timestamp: string;
		level: 'info' | 'warn' | 'error' | 'debug';
		message: string;
	}

	let { maxEntries = 500 }: { maxEntries?: number } = $props();

	let logs = $state<LogEntry[]>([]);
	let filterLevel = $state<string>('all');
	let autoScroll = $state(true);
	let logContainer: HTMLDivElement | undefined = $state();

	function addLog(level: LogEntry['level'], message: string) {
		const entry: LogEntry = {
			timestamp: new Date().toLocaleTimeString('ja-JP'),
			level,
			message,
		};
		logs = [...logs.slice(1 - maxEntries), entry];
	}

	function clearLogs() {
		logs = [];
	}

	function levelColor(level: string): string {
		switch (level) {
			case 'error': return 'text-red-400';
			case 'warn': return 'text-yellow-400';
			case 'info': return 'text-green-400';
			case 'debug': return 'text-gray-400';
			default: return 'text-gray-300';
		}
	}

	function levelBadge(level: string): string {
		switch (level) {
			case 'error': return 'bg-red-600';
			case 'warn': return 'bg-yellow-600';
			case 'info': return 'bg-green-600';
			case 'debug': return 'bg-gray-600';
			default: return 'bg-gray-600';
		}
	}

	const filteredLogs = $derived(
		filterLevel === 'all' ? logs : logs.filter((l) => l.level === filterLevel)
	);

	$effect(() => {
		if (autoScroll && logContainer) {
			logContainer.scrollTop = logContainer.scrollHeight;
		}
	});
</script>

<div class="flex flex-col border border-gray-700 rounded bg-gray-900">
	<div class="flex items-center justify-between border-b border-gray-700 px-3 py-2">
		<h3 class="text-sm font-medium text-gray-200">ログ</h3>
		<div class="flex items-center gap-2">
			<select
				bind:value={filterLevel}
				class="rounded bg-gray-800 px-2 py-0.5 text-xs text-gray-300"
			>
				<option value="all">すべて</option>
				<option value="info">Info</option>
				<option value="warn">Warn</option>
				<option value="error">Error</option>
				<option value="debug">Debug</option>
			</select>
			<label class="flex items-center gap-1 text-xs text-gray-400">
				<input type="checkbox" bind:checked={autoScroll} class="accent-blue-500" />
				自動スクロール
			</label>
			<button
				onclick={clearLogs}
				class="rounded bg-gray-700 px-2 py-0.5 text-xs text-gray-300 hover:bg-gray-600"
			>
				クリア
			</button>
		</div>
	</div>
	<div
		bind:this={logContainer}
		class="h-40 overflow-y-auto p-2 font-mono text-xs leading-relaxed"
	>
		{#if filteredLogs.length === 0}
			<p class="italic text-gray-500">ログはまだありません</p>
		{:else}
			{#each filteredLogs as entry (entry.timestamp + entry.message)}
				<div class="flex gap-2">
					<span class="shrink-0 text-gray-500">[{entry.timestamp}]</span>
					<span class="shrink-0 rounded px-1 {levelBadge(entry.level)} text-white">{entry.level.toUpperCase()}</span>
					<span class={levelColor(entry.level)}>{entry.message}</span>
				</div>
			{/each}
		{/if}
	</div>
</div>
