<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { api } from '$lib/api/client';
	import { wsClient } from '$lib/api/websocket';
	import type { WSMessage } from '$lib/api/websocket';

	// ─── Props ──────────────────────────────────────────────────────────────
	interface SerialMonitorProps {
		connected?: boolean;
		baudrate?: number;
		portName?: string;
	}

	let {
		connected = false,
		baudrate = 115200,
		portName = '',
	}: SerialMonitorProps = $props();

	// ─── Serial data entry ──────────────────────────────────────────────────
	interface SerialEntry {
		id: number;
		timestamp: string;
		direction: 'RX' | 'TX';
		data: string;
		raw: Uint8Array;
	}

	// ─── Reactive state ─────────────────────────────────────────────────────
	let entries = $state<SerialEntry[]>([]);
	let sendText = $state('');
	let autoScroll = $state(true);
	let hexMode = $state(false);
	let showTimestamp = $state(true);
	let monitorContainer: HTMLDivElement | undefined = $state();
	// eslint-disable-next-line @typescript-eslint/no-unused-vars
	let wsConnected = $state(false);

	let entryCounter = 0;

	const MAX_ENTRIES = 1000;

	// ─── Derived ────────────────────────────────────────────────────────────
	const displayEntries = $derived(entries);

	// ─── WebSocket handling ─────────────────────────────────────────────────
	function handleWSMessage(msg: WSMessage) {
		if (msg.type === 'serial') {
			connected = msg.connected;
			if (msg.baudrate !== undefined) baudrate = msg.baudrate;
			if (msg.port_name !== undefined) portName = msg.port_name;
		}
		// Future: handle serial_data messages when available from backend
	}

	function handleWSConnect() {
		wsConnected = true;
	}

	function handleWSDisconnect() {
		wsConnected = false;
	}

	onMount(() => {
		// Fetch initial serial status
		api.getSerialStatus()
			.then((status) => {
				connected = status.connected;
				if (status.port_name) portName = status.port_name;
				if (status.baudrate) baudrate = status.baudrate;
			})
			.catch(() => {});

		// Register WebSocket listeners (connection managed by StatusBar)
		wsClient.on('connect', handleWSConnect);
		wsClient.on('disconnect', handleWSDisconnect);
		wsClient.on('message', handleWSMessage);

		// Poll serial status periodically
		const statusInterval = setInterval(async () => {
			try {
				const status = await api.getSerialStatus();
				connected = status.connected;
				if (status.port_name) portName = status.port_name;
				if (status.baudrate) baudrate = status.baudrate;
			} catch {
				// ignore
			}
		}, 5000);

		onDestroy(() => {
			clearInterval(statusInterval);
			wsClient.off('connect', handleWSConnect);
			wsClient.off('disconnect', handleWSDisconnect);
			wsClient.off('message', handleWSMessage);
		});
	});

	// ─── Auto-scroll effect ─────────────────────────────────────────────────
	$effect(() => {
		if (autoScroll && monitorContainer) {
			monitorContainer.scrollTop = monitorContainer.scrollHeight;
		}
	});

	// ─── Actions ────────────────────────────────────────────────────────────

	function addEntry(direction: 'RX' | 'TX', data: string) {
		const encoder = new TextEncoder();
		const raw = encoder.encode(data);
		const now = new Date();
		const timestamp = now.toLocaleTimeString('ja-JP', { hour12: false }) +
			'.' + String(now.getMilliseconds()).padStart(3, '0');

		entries = [...entries.slice(-(MAX_ENTRIES - 1)), {
			id: entryCounter++,
			timestamp,
			direction,
			data,
			raw,
		}];
	}

	async function handleSend() {
		const text = sendText.trim();
		if (!text || !connected) return;

		try {
			await api.writeSerial(text);
			addEntry('TX', text);
			sendText = '';
		} catch (err) {
			console.error('Serial send failed:', err);
		}
	}

	function handleSendKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter' && !e.shiftKey) {
			e.preventDefault();
			handleSend();
		}
	}

	function clearEntries() {
		entries = [];
		entryCounter = 0;
	}

	function formatHex(raw: Uint8Array): string {
		return Array.from(raw)
			.map((b) => b.toString(16).padStart(2, '0').toUpperCase())
			.join(' ');
	}

	function formatAscii(data: string): string {
		// Replace non-printable characters with visible representation
		// eslint-disable-next-line no-control-regex
		return data.replace(/[\x00-\x1F\x7F-\x9F]/g, (ch) => {
			const code = ch.charCodeAt(0);
			if (code === 0x0a) return '\\n';
			if (code === 0x0d) return '\\r';
			if (code === 0x09) return '\\t';
			return `\\x${code.toString(16).padStart(2, '0')}`;
		});
	}

	function directionColor(dir: 'RX' | 'TX'): string {
		return dir === 'RX' ? 'text-green-400' : 'text-blue-400';
	}

	function directionBadge(dir: 'RX' | 'TX'): string {
		return dir === 'RX' ? 'bg-green-700' : 'bg-blue-700';
	}
</script>

<div class="flex flex-col rounded border border-gray-700 bg-gray-900">
	<!-- Header with status and controls -->
	<div class="flex items-center justify-between border-b border-gray-700 px-3 py-2">
		<div class="flex items-center gap-2">
			<h3 class="text-sm font-medium text-gray-200">シリアルモニタ</h3>
			<!-- Connection indicator -->
			<span
				class="inline-block h-2 w-2 rounded-full {connected ? 'bg-green-500' : 'bg-red-500'}"
				title={connected ? '接続済み' : '未接続'}
			></span>
			{#if connected}
				<span class="text-xs text-gray-400">
					{baudrate} bps
					{#if portName}
						<span class="ml-1 text-gray-500">| {portName}</span>
					{/if}
				</span>
			{/if}
		</div>

		<div class="flex items-center gap-2">
			<!-- Hex/ASCII toggle -->
			<button
				onclick={() => (hexMode = !hexMode)}
				class="rounded px-2 py-0.5 text-xs transition-colors {hexMode
					? 'bg-purple-600 text-white'
					: 'bg-gray-700 text-gray-300 hover:bg-gray-600'}"
				title="表示モード切替"
			>
				{hexMode ? 'HEX' : 'ASCII'}
			</button>

			<!-- Timestamp toggle -->
			<button
				onclick={() => (showTimestamp = !showTimestamp)}
				class="rounded px-2 py-0.5 text-xs transition-colors {showTimestamp
					? 'bg-gray-600 text-gray-200'
					: 'bg-gray-700 text-gray-400 hover:bg-gray-600'}"
				title="タイムスタンプ表示"
			>
				<span class="text-[10px]">⏱</span>
			</button>

			<!-- Auto-scroll toggle -->
			<label class="flex items-center gap-1 text-xs text-gray-400">
				<input type="checkbox" bind:checked={autoScroll} class="accent-blue-500" />
				自動
			</label>

			<!-- Clear button -->
			<button
				onclick={clearEntries}
				class="rounded bg-gray-700 px-2 py-0.5 text-xs text-gray-300 hover:bg-gray-600"
				title="クリア"
			>
				クリア
			</button>
		</div>
	</div>

	<!-- Data display area -->
	<div
		bind:this={monitorContainer}
		class="h-48 overflow-y-auto p-2 font-mono text-xs leading-relaxed"
	>
		{#if displayEntries.length === 0}
			<p class="italic text-gray-500">データはまだありません</p>
		{:else}
			{#each displayEntries as entry (entry.id)}
				<div class="flex gap-2 py-0.5">
					{#if showTimestamp}
						<span class="shrink-0 text-gray-500" title={entry.timestamp}>
							[{entry.timestamp}]
						</span>
					{/if}
					<span
						class="shrink-0 rounded px-1 {directionBadge(entry.direction)} text-white text-[10px] font-bold"
					>
						{entry.direction}
					</span>
					<span class={directionColor(entry.direction)}>
						{hexMode ? formatHex(entry.raw) : formatAscii(entry.data)}
					</span>
				</div>
			{/each}
		{/if}
	</div>

	<!-- Send input area -->
	<div class="flex items-center gap-2 border-t border-gray-700 px-3 py-2">
		<input
			bind:value={sendText}
			onkeydown={handleSendKeydown}
			placeholder={connected ? '送信データを入力...' : 'シリアル未接続'}
			disabled={!connected}
			class="flex-1 rounded bg-gray-800 px-2 py-1.5 text-xs text-gray-200 placeholder-gray-500 outline-none ring-1 ring-gray-600 focus:ring-blue-500 disabled:opacity-50"
		/>
		<button
			onclick={handleSend}
			disabled={!connected || !sendText.trim()}
			class="rounded bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500 disabled:opacity-50"
		>
			送信
		</button>
	</div>
</div>
