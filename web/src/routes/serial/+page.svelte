<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { api, wsClient } from '$lib/api/client';
	import type { SerialPort, SerialStatus, WSMessage } from '$lib/api/client';
	import SerialMonitor from '$lib/components/SerialMonitor.svelte';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';

	// ─── Types ──────────────────────────────────────────────────────────────
	type DataFormat = 'default' | 'qingpi' | '3ds';

	// ─── Reactive state ─────────────────────────────────────────────────────
	let ports = $state<SerialPort[]>([]);
	let selectedPortName = $state('');
	let baudrate = $state(9600);
	let dataFormat = $state<DataFormat>('default');
	let connected = $state(false);
	let connecting = $state(false);
	let errorMessage = $state<string | null>(null);
	let successMessage = $state<string | null>(null);

	// ─── Constants ───────────────────────────────────────────────────────────
	const baudrates = [9600, 115200];

	const formatBaudRate: Record<DataFormat, number> = {
		default: 9600,
		qingpi: 9600,
		'3ds': 115200,
	};

	const formatLabels: Record<DataFormat, string> = {
		default: 'Default',
		qingpi: 'Qingpi',
		'3ds': '3DS Controller',
	};

	// ─── Helpers ─────────────────────────────────────────────────────────────
	let statusTimer: ReturnType<typeof setInterval> | undefined;

	function showError(msg: string, durationMs = 5000) {
		errorMessage = msg;
		if (durationMs > 0) {
			setTimeout(() => {
				if (errorMessage === msg) errorMessage = null;
			}, durationMs);
		}
	}

	function showSuccess(msg: string, durationMs = 3000) {
		successMessage = msg;
		if (durationMs > 0) {
			setTimeout(() => {
				if (successMessage === msg) successMessage = null;
			}, durationMs);
		}
	}

	// ─── Lifecycle ──────────────────────────────────────────────────────────
	onMount(() => {
		refreshPorts();
		loadSerialStatus();

		// Start polling serial status
		statusTimer = setInterval(pollSerialStatus, 5000);

		// Register WebSocket event listeners
		wsClient.on('connect', handleWSConnect);
		wsClient.on('disconnect', handleWSDisconnect);
		wsClient.on('message', handleWSMessage);

		onDestroy(() => {
			if (statusTimer) clearInterval(statusTimer);
			wsClient.off('connect', handleWSConnect);
			wsClient.off('disconnect', handleWSDisconnect);
			wsClient.off('message', handleWSMessage);
		});
	});

	// ─── Port management ────────────────────────────────────────────────────
	async function refreshPorts() {
		try {
			const list = await api.getSerialPorts();
			ports = list;
			// Auto-select first port if none selected or current selection is stale
			if (list.length > 0) {
				const currentInList = list.find((p) => p.port_name === selectedPortName);
				if (!currentInList) {
					selectedPortName = list[0].port_name;
				}
			} else {
				selectedPortName = '';
			}
		} catch {
			ports = [];
			showError('Failed to fetch serial ports');
		}
	}

	// ─── Serial status ──────────────────────────────────────────────────────
	async function loadSerialStatus() {
		try {
			const status = await api.getSerialStatus();
			applyStatus(status);
		} catch {
			// Ignore on initial load
		}
	}

	async function pollSerialStatus() {
		try {
			const status = await api.getSerialStatus();
			applyStatus(status);
		} catch {
			// Ignore polling errors
		}
	}

	function applyStatus(status: SerialStatus) {
		connected = status.connected;
		if (status.port_name) selectedPortName = status.port_name;
		if (status.baudrate) {
			baudrate = status.baudrate;
			// Sync data format with baud rate
			for (const [fmt, rate] of Object.entries(formatBaudRate)) {
				if (rate === status.baudrate) {
					dataFormat = fmt as DataFormat;
					break;
				}
			}
		}
	}

	// ─── WebSocket handling ─────────────────────────────────────────────────
	function handleWSConnect() {
		loadSerialStatus();
	}

	function handleWSDisconnect() {
		// WebSocket disconnected — will reconnect automatically
	}

	function handleWSMessage(msg: WSMessage) {
		if (msg.type === 'serial') {
			connected = msg.connected;
			if (msg.port_name !== undefined) selectedPortName = msg.port_name;
			if (msg.baudrate !== undefined) {
				baudrate = msg.baudrate;
				for (const [fmt, rate] of Object.entries(formatBaudRate)) {
					if (rate === msg.baudrate) {
						dataFormat = fmt as DataFormat;
						break;
					}
				}
			}
		}
	}

	// ─── Data Format change ─────────────────────────────────────────────────
	function handleDataFormatChange(e: Event) {
		const format = (e.target as HTMLSelectElement).value as DataFormat;
		dataFormat = format;
		const newBaud = formatBaudRate[format];
		if (newBaud !== baudrate) {
			baudrate = newBaud;
		}
		// Push config to backend if connected
		if (connected) {
			api.updateSerialConfig({ baudrate, data_format: format })
				.then(() => showSuccess(`Data format set to ${formatLabels[format]}`))
				.catch((err) => showError(`Config update failed: ${err.message}`));
		}
	}

	// ─── Baud Rate change ───────────────────────────────────────────────────
	function handleBaudrateChange(e: Event) {
		const newBaud = Number((e.target as HTMLSelectElement).value);
		baudrate = newBaud;
		// Sync data format: find matching format
		for (const [fmt, rate] of Object.entries(formatBaudRate)) {
			if (rate === newBaud) {
				dataFormat = fmt as DataFormat;
				break;
			}
		}
		// Push config to backend if connected
		if (connected) {
			api.updateSerialConfig({ baudrate: newBaud })
				.then(() => showSuccess(`Baud rate set to ${newBaud}`))
				.catch((err) => showError(`Config update failed: ${err.message}`));
		}
	}

	// ─── Connect / Disconnect ───────────────────────────────────────────────
	async function toggleSerial() {
		errorMessage = null;
		connecting = true;

		try {
			if (connected) {
				await api.closeSerial();
				connected = false;
				showSuccess('Serial port disconnected');
			} else {
				if (!selectedPortName) {
					showError('No port selected');
					return;
				}
				const port = ports.find((p) => p.port_name === selectedPortName);
				if (!port) {
					showError('Selected port not found — refresh port list');
					return;
				}
				await api.openSerial({
					port_num: port.port_num,
					port_name: port.port_name,
					baudrate,
				});
				connected = true;
				showSuccess(`Connected to ${selectedPortName} @ ${baudrate} bps`);
			}
		} catch (e) {
			const msg = e instanceof Error ? e.message : 'Serial operation failed';
			showError(msg);
			connected = false;
		} finally {
			connecting = false;
		}
	}
</script>

<div class="tab-content">
	<!-- Header -->
	<div class="flex items-center justify-between px-1 py-0.5">
		<h2 class="text-sm font-bold text-gray-200">Serial</h2>
		<div class="flex items-center gap-2">
			{#if successMessage}
				<span class="rounded bg-green-900/50 px-2 py-0.5 text-[10px] text-green-400">
					{successMessage}
				</span>
			{/if}
			{#if errorMessage}
				<span class="rounded bg-red-900/50 px-2 py-0.5 text-[10px] text-red-400">
					{errorMessage}
				</span>
			{/if}
			<!-- Connection indicator -->
			<span
				class="inline-block h-2 w-2 rounded-full {connected ? 'bg-green-500' : 'bg-red-500'}"
				title={connected ? 'Connected' : 'Disconnected'}
			></span>
		</div>
	</div>

	<div class="grid grid-cols-1 gap-2 lg:grid-cols-5">
		<!-- ── LEFT: Connection & Configuration ──────────────────────── -->
		<div class="space-y-2 lg:col-span-2">
			<!-- Connection Control -->
			<div class="tk-labelframe">
				<div class="tk-labelframe-label">Connection</div>
				<div class="tk-labelframe-content">
					<div class="form-row">
						<span class="tk-label">Port</span>
						<select
							bind:value={selectedPortName}
							disabled={connected}
							class="tk-select flex-1"
						>
							<option value="" disabled>Select a port...</option>
							{#each ports as port (port.port_name)}
								<option value={port.port_name}>
									{port.port_name}
									{#if port.description}
										&mdash; {port.description}
									{/if}
								</option>
							{/each}
						</select>
						<button
							onclick={refreshPorts}
							disabled={connected || connecting}
							class="tk-btn"
							title="Rescan available ports"
						>&#x21bb;</button>
					</div>
					<div class="form-row" style="padding-top: 6px;">
						<button
							onclick={toggleSerial}
							disabled={connecting || !selectedPortName}
							class="tk-btn w-full"
							class:tk-btn-primary={!connected}
							class:tk-btn-danger={connected}
						>
							{#if connecting}
								{connected ? 'Disconnecting...' : 'Connecting...'}
							{:else}
								{connected ? 'Disconnect' : 'Connect'}
							{/if}
						</button>
					</div>
				</div>
			</div>

			<!-- Configuration -->
			<div class="tk-labelframe">
				<div class="tk-labelframe-label">Configuration</div>
				<div class="tk-labelframe-content">
					<div class="form-row">
						<span class="tk-label">Baud Rate</span>
						<select
							value={baudrate}
							onchange={handleBaudrateChange}
							class="tk-select"
						>
							{#each baudrates as b (b)}
								<option value={b}>{b} bps</option>
							{/each}
						</select>
					</div>
					<div class="form-row">
						<span class="tk-label">Data Format</span>
						<select
							value={dataFormat}
							onchange={handleDataFormatChange}
							class="tk-select"
						>
							{#each Object.entries(formatLabels) as [value, label] (value)}
								<option value={value}>{label}</option>
							{/each}
						</select>
					</div>
					<div class="form-row" style="padding-top: 4px;">
						<span class="text-[10px] text-gray-500">
							{#if dataFormat === 'default'}
								Default format &mdash; Baud rate 9600
							{:else if dataFormat === 'qingpi'}
								Qingpi format &mdash; Baud rate 9600
							{:else if dataFormat === '3ds'}
								3DS Controller format &mdash; Baud rate 115200
							{/if}
						</span>
					</div>
				</div>
			</div>
		</div>

		<!-- ── RIGHT: Serial Monitor ────────────────────────────────── -->
		<div class="lg:col-span-3">
			<SerialMonitor {connected} {baudrate} portName={selectedPortName} />
		</div>
	</div>

	<!-- Output panels (consistent with other tabs) -->
	<div class="grid grid-cols-1 gap-2 lg:grid-cols-2" style="padding-top: 8px;">
		<OutputPanel title="Serial Output" />
		<LogPanel />
	</div>
</div>

<style>
	.form-row {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 2px 0;
	}
	.form-row + .form-row {
		padding-top: 4px;
	}

	.tk-label {
		font-size: 11px;
		color: var(--color-text-secondary, #94a3b8);
		min-width: 72px;
		flex-shrink: 0;
	}

	.tk-select {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		padding: 1px 4px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-input, #1e293b);
		color: var(--color-text-primary, #f1f5f9);
		outline: none;
		cursor: pointer;
	}
	.tk-select:focus {
		border-color: var(--color-accent, #60a5fa);
	}
	.tk-select:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.tk-btn {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		padding: 2px 10px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-tertiary, #334155);
		color: var(--color-text-primary, #f1f5f9);
		cursor: pointer;
		user-select: none;
		white-space: nowrap;
		line-height: 1.4;
	}
	.tk-btn:hover:not(:disabled) {
		background-color: var(--color-accent-hover, #3b82f6);
		color: #fff;
	}
	.tk-btn:active:not(:disabled) {
		background-color: var(--color-accent, #60a5fa);
	}
	.tk-btn:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	.tk-btn-primary {
		background-color: var(--color-accent, #3b82f6);
		color: #fff;
		border-color: var(--color-accent, #3b82f6);
	}
	.tk-btn-primary:hover:not(:disabled) {
		background-color: var(--color-accent-hover, #2563eb);
		border-color: var(--color-accent-hover, #2563eb);
	}

	.tk-btn-danger {
		background-color: var(--color-error, #ef4444);
		color: #fff;
		border-color: var(--color-error, #ef4444);
	}
	.tk-btn-danger:hover:not(:disabled) {
		background-color: #dc2626;
		border-color: #dc2626;
	}

	/* ── Labelframe (Tkinter-style bordered group) ───────────────────── */
	.tk-labelframe {
		border: 2px solid var(--color-border, #334155);
		border-radius: 2px;
		background-color: var(--color-bg-card, #1e293b);
		position: relative;
		margin-top: 6px;
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
		padding: 10px 4px 4px 4px;
	}

	/* ── Full-width button utility ───────────────────────────────────── */
	.w-full {
		width: 100%;
	}
</style>
