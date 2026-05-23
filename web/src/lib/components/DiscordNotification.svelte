<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';

	let webhookUrl = $state('');
	let username = $state('');
	let avatarUrl = $state('');
	let loading = $state(true);
	let saving = $state(false);
	let testSending = $state(false);
	let statusMessage = $state('');
	let statusType = $state<'success' | 'error' | ''>('');
	let connectionOk = $state<boolean | null>(null);

	const webhookPattern = /^https:\/\/discord\.com\/api\/webhooks\//;

	onMount(() => {
		loadSettings();
	});

	async function loadSettings() {
		try {
			// Use the generic notification config to load
			const config = await api.getNotificationConfig();
			webhookUrl = config.discord_webhook_url ?? '';
			checkConnection();
		} catch (e) {
			console.warn('Failed to load Discord notification settings:', e);
		}
		loading = false;
	}

	function checkConnection() {
		if (webhookUrl && webhookPattern.test(webhookUrl)) {
			connectionOk = true;
		} else if (webhookUrl && webhookUrl.length > 0) {
			connectionOk = false;
		} else {
			connectionOk = null;
		}
	}

	function setStatus(msg: string, type: 'success' | 'error') {
		statusMessage = msg;
		statusType = type;
		setTimeout(() => {
			statusMessage = '';
			statusType = '';
		}, 3000);
	}

	async function saveWebhookUrl() {
		saving = true;
		try {
			if (webhookUrl && !webhookPattern.test(webhookUrl)) {
				setStatus('Invalid webhook URL format (must start with https://discord.com/api/webhooks/)', 'error');
				saving = false;
				return;
			}
			await api.updateNotificationConfig({
				discord_webhook_url: webhookUrl || null,
			});
			checkConnection();
			setStatus('Webhook URL saved', 'success');
		} catch (e) {
			setStatus('Save failed: ' + (e as Error).message, 'error');
		}
		saving = false;
	}

	async function sendTest() {
		testSending = true;
		try {
			// Enable discord for the test
			await api.updateNotificationConfig({ discord_enabled: true });
			await api.sendTestNotification({
				message: 'Discord webhook test notification',
				title: 'Poke-Controller Test',
			});
			setStatus('Test notification sent', 'success');
		} catch (e) {
			setStatus('Send failed: ' + (e as Error).message, 'error');
		}
		testSending = false;
	}
</script>

<div class="tk-labelframe">
	<div class="tk-labelframe-label">Discord Notification</div>
	<div class="tk-labelframe-content">
		{#if loading}
			<span class="text-[10px] text-gray-500">Loading...</span>
		{:else}
			<div class="space-y-2">
				<!-- Webhook URL -->
				<div class="form-row form-col">
					<span class="tk-label">Webhook URL</span>
					<div class="flex-1">
						<input
							type="url"
							bind:value={webhookUrl}
							oninput={checkConnection}
							placeholder="https://discord.com/api/webhooks/..."
							class="tk-input w-full"
						/>
						<!-- Connection status -->
						{#if connectionOk === true}
							<div class="flex items-center gap-1 text-[10px] text-green-400" style="margin-top: 2px;">
								<span class="inline-block h-1.5 w-1.5 rounded-full bg-green-500"></span>
								<span>Valid webhook URL format</span>
							</div>
						{:else if connectionOk === false}
							<div class="flex items-center gap-1 text-[10px] text-yellow-400" style="margin-top: 2px;">
								<span class="inline-block h-1.5 w-1.5 rounded-full bg-yellow-500"></span>
								<span>Invalid format — must start with https://discord.com/api/webhooks/</span>
							</div>
						{/if}
					</div>
				</div>

				<!-- Save webhook URL -->
				<div class="form-row" style="padding-top: 2px;">
					<button
						onclick={saveWebhookUrl}
						disabled={saving}
						class="tk-btn w-full"
					>
						{saving ? 'Saving...' : 'Save Webhook URL'}
					</button>
				</div>

				<!-- Username (optional) -->
				<div class="form-row form-col">
					<span class="tk-label">Username</span>
					<input
						type="text"
						bind:value={username}
						placeholder="Poke-Controller Bot (optional)"
						class="tk-input w-full"
					/>
				</div>

				<!-- Avatar URL (optional) -->
				<div class="form-row form-col">
					<span class="tk-label">Avatar URL</span>
					<input
						type="url"
						bind:value={avatarUrl}
						placeholder="https://example.com/avatar.png (optional)"
						class="tk-input w-full"
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
				<div class="form-row" style="padding-top: 4px;">
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
	.form-col {
		flex-direction: column;
		align-items: stretch;
	}

	.tk-label {
		font-size: 11px;
		color: var(--color-text-secondary, #94a3b8);
		min-width: 100px;
		flex-shrink: 0;
	}

	.tk-input {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		padding: 2px 6px;
		border: 1px solid var(--color-border, #475569);
		border-radius: 2px;
		background-color: var(--color-bg-input, #1e293b);
		color: var(--color-text-primary, #f1f5f9);
		outline: none;
		box-sizing: border-box;
	}
	.tk-input:focus {
		border-color: var(--color-accent, #60a5fa);
	}
	.tk-input::placeholder {
		color: var(--color-text-tertiary, #64748b);
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
	.flex-1 {
		flex: 1;
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
