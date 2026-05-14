<script lang="ts">
	import { api } from '$lib/api/client';

	function handleStart() {
		api.startCommand('').catch(console.warn);
	}

	function handlePause() {
		api.stopCommand().catch(console.warn);
	}

	function handleCapture() {
		api.captureCamera('capture.png').catch(console.warn);
	}

	function handleOpenCaptureDir() {
		window.dispatchEvent(new CustomEvent('open-capture-dir'));
	}
</script>

<div class="main-toolbar">
	<button class="tk-btn tk-btn-action tk-btn-start" onclick={handleStart}>Start</button>
	<button class="tk-btn tk-btn-action" onclick={handlePause}>Pause</button>
	<button class="tk-btn tk-btn-action" onclick={() => window.dispatchEvent(new CustomEvent('restart'))}>Restart</button>
	<button class="tk-btn tk-btn-action tk-btn-stop" onclick={handlePause}>Stop</button>

	<span class="toolbar-separator"></span>

	<button class="tk-btn" onclick={() => window.dispatchEvent(new CustomEvent('open-controller'))}>Controller</button>

	<span class="toolbar-separator"></span>

	<button class="tk-btn" onclick={() => window.dispatchEvent(new CustomEvent('clear-outputs'))}>Clear Outputs</button>
	<button class="tk-btn" onclick={handleCapture}>Capture</button>
	<button class="tk-btn tk-btn-icon" onclick={handleOpenCaptureDir} title="Open Capture Directory">📂</button>
	<button class="tk-btn" onclick={() => window.dispatchEvent(new CustomEvent('discord-notify'))}>Discord</button>
</div>

<style>
	.main-toolbar {
		display: flex;
		align-items: center;
		gap: 3px;
		padding: 3px 6px;
		background-color: var(--color-bg-secondary, #1e293b);
		border-bottom: 1px solid var(--color-border, #334155);
		flex-wrap: wrap;
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
	}

	.tk-btn:hover {
		background-color: var(--color-accent-hover, #3b82f6);
		color: #fff;
	}

	.tk-btn:active {
		background-color: var(--color-accent, #60a5fa);
	}

	.tk-btn-action {
		font-weight: 600;
		padding: 2px 14px;
	}

	.tk-btn-start {
		background-color: #1b5e20;
		border-color: #2e7d32;
		color: #a5d6a7;
	}

	.tk-btn-start:hover {
		background-color: #2e7d32;
		color: #fff;
	}

	.tk-btn-stop {
		background-color: #b71c1c;
		border-color: #c62828;
		color: #ef9a9a;
	}

	.tk-btn-stop:hover {
		background-color: #c62828;
		color: #fff;
	}

	.tk-btn-icon {
		font-size: 12px;
		padding: 2px 6px;
	}

	.toolbar-separator {
		width: 1px;
		height: 20px;
		background-color: var(--color-border, #475569);
		margin: 0 4px;
	}
</style>
