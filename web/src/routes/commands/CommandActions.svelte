<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import type { ActiveCommand } from '$lib/api/client';

	interface Props {
		selectedCommand?: string;
	}

	let { selectedCommand = $bindable('') }: Props = $props();

	// ── State ────────────────────────────────────────────────────────────────
	type CommandStatus = 'stopped' | 'running' | 'paused' | 'error';

	let status = $state<CommandStatus>('stopped');
	let activeName = $state<string | null>(null);
	let progress = $state<number>(0);
	let progressMax = $state<number>(100);
	let progressIndeterminate = $state(false);
	let errorMessage = $state<string | null>(null);

	// ── Poll active command state ────────────────────────────────────────────
	let pollTimer: ReturnType<typeof setInterval> | null = null;

	async function pollActiveCommand() {
		try {
			const active: ActiveCommand = await api.getActiveCommand();
			if (active.running && active.name) {
				status = 'running';
				activeName = active.name;
				errorMessage = null;
			} else if (active.name && !active.running) {
				// Could be paused — we'll track locally since API doesn't distinguish
				status = 'stopped';
				activeName = null;
			} else {
				status = 'stopped';
				activeName = null;
			}
		} catch {
			// Backend unreachable — don't change state
		}
	}

	onMount(() => {
		pollActiveCommand();
		pollTimer = setInterval(pollActiveCommand, 2000);
		return () => {
			if (pollTimer) clearInterval(pollTimer);
		};
	});

	// ── Action handlers ──────────────────────────────────────────────────────

	async function handleStart() {
		if (!selectedCommand) return;
		try {
			await api.startCommand(selectedCommand);
			status = 'running';
			activeName = selectedCommand;
			errorMessage = null;
			progressIndeterminate = true;
		} catch (e) {
			status = 'error';
			errorMessage = e instanceof Error ? e.message : String(e);
		}
	}

	async function handlePause() {
		try {
			// The API doesn't have a dedicated pause endpoint.
			// We use stop() as pause since the spec says Pause is resumable.
			// In practice, a real pause needs backend support. We'll do best-effort.
			await api.stopCommand();
			status = 'paused';
		} catch (e) {
			errorMessage = e instanceof Error ? e.message : String(e);
		}
	}

	async function handleRestart() {
		if (!activeName && !selectedCommand) return;
		const cmd = activeName ?? selectedCommand;
		try {
			await api.stopCommand();
			await api.startCommand(cmd);
			status = 'running';
			activeName = cmd;
			errorMessage = null;
			progressIndeterminate = true;
		} catch (e) {
			status = 'error';
			errorMessage = e instanceof Error ? e.message : String(e);
		}
	}

	async function handleStop() {
		try {
			await api.stopCommand();
			status = 'stopped';
			activeName = null;
			progress = 0;
			progressIndeterminate = false;
			errorMessage = null;
		} catch (e) {
			errorMessage = e instanceof Error ? e.message : String(e);
		}
	}

	async function handleReload() {
		try {
			await api.reloadCommands();
			errorMessage = null;
		} catch (e) {
			errorMessage = e instanceof Error ? e.message : String(e);
		}
	}

	// ── Keyboard shortcuts (F5=Start, Shift+F6=Pause, Escape=Stop) ───────────
	function handleKeyDown(event: KeyboardEvent) {
		const target = event.target as HTMLElement;
		if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) {
			return;
		}

		if (event.key === 'F5' && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
			event.preventDefault();
			handleStart();
			return;
		}

		if (event.key === 'F6' && event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
			event.preventDefault();
			handlePause();
			return;
		}

		if (event.key === 'Escape' && !event.shiftKey && !event.ctrlKey && !event.altKey && !event.metaKey) {
			event.preventDefault();
			handleStop();
			return;
		}
	}

	onMount(() => {
		window.addEventListener('keydown', handleKeyDown);
		return () => window.removeEventListener('keydown', handleKeyDown);
	});

	// ── Derived ──────────────────────────────────────────────────────────────
	let statusLabel = $derived.by(() => {
		switch (status) {
			case 'running': return '実行中';
			case 'paused': return '一時停止';
			case 'stopped': return '停止';
			case 'error': return 'エラー';
		}
	});

	let statusClass = $derived.by(() => {
		switch (status) {
			case 'running': return 'status-running';
			case 'paused': return 'status-paused';
			case 'stopped': return 'status-stopped';
			case 'error': return 'status-error';
		}
	});

	let canStart = $derived(status === 'stopped' || status === 'error');
	let canPause = $derived(status === 'running');
	let canRestart = $derived(status === 'running' || status === 'paused' || status === 'error');
	let canStop = $derived(status === 'running' || status === 'paused');
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<h3 class="mb-2 text-sm font-medium text-gray-200">コマンド操作</h3>

	<!-- Status Display -->
	<div class="status-bar">
		<span class="status-indicator {statusClass}"></span>
		<span class="status-text">
			ステータス: <strong>{statusLabel}</strong>
			{#if activeName}
				<span class="text-gray-400"> ({activeName.length > 25 ? activeName.slice(0, 25) + '…' : activeName})</span>
			{/if}
		</span>
	</div>

	{#if errorMessage}
		<div class="error-message">
			{errorMessage}
		</div>
	{/if}

	<!-- Progress Bar -->
	<div class="progress-section">
		{#if progressIndeterminate}
			<div class="progress-bar indeterminate">
				<div class="progress-fill"></div>
			</div>
		{:else if progressMax > 0}
			<div class="progress-bar">
				<div
					class="progress-fill"
					style="width: {(progress / progressMax) * 100}%"
				></div>
			</div>
		{/if}
	</div>

	<!-- Action Buttons -->
	<div class="action-buttons">
		<button
			class="tk-btn tk-btn-start"
			onclick={handleStart}
			disabled={!selectedCommand || !canStart}
			title="開始 (F5)"
		>
			▶ 開始
		</button>
		<button
			class="tk-btn tk-btn-pause"
			onclick={handlePause}
			disabled={!canPause}
			title="一時停止 (Shift+F6)"
		>
			⏸ 一時停止
		</button>
		<button
			class="tk-btn tk-btn-restart"
			onclick={handleRestart}
			disabled={!canRestart}
			title="再起動"
		>
			↺ 再起動
		</button>
		<button
			class="tk-btn tk-btn-stop"
			onclick={handleStop}
			disabled={!canStop}
			title="停止 (Esc)"
		>
			⏹ 停止
		</button>
		<button
			class="tk-btn tk-btn-reload"
			onclick={handleReload}
			title="再読み込み"
		>
			⟳ 再読み込み
		</button>
	</div>

	{#if selectedCommand}
		<p class="mt-2 text-xs text-gray-400">
			選択中: <span class="text-gray-200">{selectedCommand}</span>
		</p>
	{/if}
</div>

<style>
	.status-bar {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 4px 0;
		font-size: 11px;
	}

	.status-indicator {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		flex-shrink: 0;
	}

	.status-running {
		background-color: #4ade80;
		box-shadow: 0 0 4px #4ade80;
	}

	.status-paused {
		background-color: #fbbf24;
		box-shadow: 0 0 4px #fbbf24;
	}

	.status-stopped {
		background-color: #94a3b8;
	}

	.status-error {
		background-color: #f87171;
		box-shadow: 0 0 4px #f87171;
	}

	.status-text {
		color: var(--color-text-secondary, #94a3b8);
	}

	.error-message {
		font-size: 10px;
		color: #f87171;
		padding: 2px 0;
		word-break: break-all;
	}

	.progress-section {
		padding: 4px 0;
	}

	.progress-bar {
		height: 6px;
		background-color: var(--color-bg-primary, #0f172a);
		border-radius: 3px;
		overflow: hidden;
		border: 1px solid var(--color-border, #475569);
	}

	.progress-fill {
		height: 100%;
		background-color: var(--color-accent, #60a5fa);
		border-radius: 2px;
		transition: width 0.3s ease;
	}

	.progress-bar.indeterminate .progress-fill {
		width: 30%;
		animation: indeterminate 1.5s ease-in-out infinite;
	}

	@keyframes indeterminate {
		0% { transform: translateX(-100%); }
		100% { transform: translateX(400%); }
	}

	.action-buttons {
		display: flex;
		gap: 4px;
		flex-wrap: wrap;
		padding: 4px 0;
	}

	.action-buttons :global(.tk-btn) {
		flex: 1;
		min-width: 60px;
		font-size: 10px;
		padding: 4px 6px;
		text-align: center;
	}

	.tk-btn-start {
		background-color: #1b5e20;
		border-color: #2e7d32;
		color: #a5d6a7;
		font-weight: 600;
	}

	.tk-btn-pause {
		background-color: #b45309;
		border-color: #d97706;
		color: #fde68a;
	}

	.tk-btn-restart {
		background-color: #1e40af;
		border-color: #2563eb;
		color: #bfdbfe;
	}

	.tk-btn-stop {
		background-color: #991b1b;
		border-color: #dc2626;
		color: #fca5a5;
	}

	.tk-btn-reload {
		background-color: var(--color-bg-tertiary, #334155);
		border-color: var(--color-border, #475569);
		color: var(--color-text-secondary, #94a3b8);
	}

	.action-buttons :global(.tk-btn:hover) {
		filter: brightness(1.2);
		color: #fff;
	}

	.action-buttons :global(.tk-btn:disabled) {
		opacity: 0.4;
		cursor: not-allowed;
		filter: none;
	}
</style>
