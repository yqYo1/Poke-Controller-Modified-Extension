<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import type { WindowsNotificationSettings } from '$lib/api/client';

	let notifyOnStart = $state(false);
	let notifyOnEnd = $state(false);
	let fullSettings = $state<WindowsNotificationSettings | null>(null);
	let loading = $state(true);
	let testSending = $state(false);
	let statusMessage = $state('');
	let statusType = $state<'success' | 'error' | ''>('');

	onMount(() => {
		loadSettings();
	});

	async function loadSettings() {
		try {
			const settings = await api.getWindowsNotificationSettings();
			fullSettings = settings;
			// Backend only has a single `enabled` flag (no separate start/end).
			// Both checkboxes are kept for UX parity; both control the same flag.
			notifyOnStart = settings.enabled;
			notifyOnEnd = settings.enabled;
		} catch (e) {
			console.warn('Failed to load Windows notification settings:', e);
		}
		loading = false;
	}

	function setStatus(msg: string, type: 'success' | 'error') {
		statusMessage = msg;
		statusType = type;
		setTimeout(() => {
			statusMessage = '';
			statusType = '';
		}, 3000);
	}

	async function saveOnStart() {
		notifyOnStart = !notifyOnStart;
		if (!fullSettings) return;
		try {
			await api.updateWindowsNotificationSettings({
				...fullSettings,
				enabled: notifyOnStart || notifyOnEnd,
			});
		} catch (e) {
			setStatus('Save failed: ' + (e as Error).message, 'error');
		}
	}

	async function saveOnEnd() {
		notifyOnEnd = !notifyOnEnd;
		if (!fullSettings) return;
		try {
			await api.updateWindowsNotificationSettings({
				...fullSettings,
				enabled: notifyOnStart || notifyOnEnd,
			});
		} catch (e) {
			setStatus('Save failed: ' + (e as Error).message, 'error');
		}
	}

	async function sendTest() {
		testSending = true;
		try {
			await api.sendTestNotification({ message: 'Test notification from Poke-Controller', title: 'Poke-Controller Test' });
			setStatus('Test notification sent', 'success');
		} catch (e) {
			setStatus('Send failed: ' + (e as Error).message, 'error');
		}
		testSending = false;
	}
</script>

<div class="tk-labelframe">
	<div class="tk-labelframe-label">Windows Notification</div>
	<div class="tk-labelframe-content">
		{#if loading}
			<span class="text-[10px] text-gray-500">Loading...</span>
		{:else}
			<div class="space-y-2">
				<!-- Notify on script start -->
				<div class="form-row">
					<span class="tk-label">Notify on script start</span>
					<input
						type="checkbox"
						checked={notifyOnStart}
						onchange={saveOnStart}
						class="tk-checkbox"
					/>
				</div>

				<!-- Notify on script end -->
				<div class="form-row">
					<span class="tk-label">Notify on script end</span>
					<input
						type="checkbox"
						checked={notifyOnEnd}
						onchange={saveOnEnd}
						class="tk-checkbox"
					/>
				</div>

				<!-- Status message -->
				{#if statusMessage}
					<div
						class="rounded px-2 py-1 text-[10px] {statusType === 'success'
							? 'bg-green-900 text-green-300'
							: 'bg-red-900 text-red-300'}"
					>
						{statusMessage}
					</div>
				{/if}

				<!-- Test button -->
				<div class="form-row" style="padding-top: 6px;">
					<button
						onclick={sendTest}
						disabled={testSending}
						class="tk-btn w-full"
					>
						{testSending ? 'Sending...' : 'Test'}
					</button>
				</div>
			</div>
		{/if}
	</div>
</div>

<style>
	.form-row {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 2px 0;
	}
	.form-row + .form-row {
		padding-top: 4px;
	}

	.tk-label {
		font-size: 11px;
		color: var(--color-text-secondary, #94a3b8);
		min-width: 120px;
		flex-shrink: 0;
	}

	.tk-checkbox {
		accent-color: var(--color-accent, #60a5fa);
		cursor: pointer;
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

	.w-full {
		width: 100%;
	}

	/* ── Labelframe (Tkinter-style bordered group) ─── */
	:global(.tk-labelframe) {
		border: 2px solid var(--color-border, #334155);
		border-radius: 2px;
		background-color: var(--color-bg-card, #1e293b);
		position: relative;
		margin-top: 6px;
	}
	:global(.tk-labelframe-label) {
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
	:global(.tk-labelframe-content) {
		padding: 10px 4px 4px 4px;
	}
</style>
