<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "$lib/api/client";
	import type { DiscordNotificationSettings } from "$lib/api/client";

	let settings = $state<DiscordNotificationSettings>({
		enabled: false,
		webhook_url: "",
		username: "",
		avatar_url: "",
	});
	let loading = $state(true);
	let saving = $state(false);
	let testSending = $state(false);
	let testMessage = $state("Discord Webhook テスト通知");
	let testTitle = $state("Poke-Controller");
	let statusMessage = $state("");
	let statusType = $state<"success" | "error" | "">("");
	let connectionOk = $state<boolean | null>(null);

	const webhookPattern = /^https:\/\/discord\.com\/api\/webhooks\//;

	onMount(() => {
		loadSettings();
	});

	async function loadSettings() {
		try {
			const result = await api.getDiscordNotificationSettings();
			settings = { ...settings, ...result };
			checkConnection();
		} catch (e) {
			console.warn("Failed to load Discord notification settings:", e);
		}
		loading = false;
	}

	function checkConnection() {
		if (settings.webhook_url && webhookPattern.test(settings.webhook_url)) {
			connectionOk = true;
		} else if (settings.webhook_url && settings.webhook_url.length > 0) {
			connectionOk = false;
		} else {
			connectionOk = null;
		}
	}

	function setStatus(msg: string, type: "success" | "error") {
		statusMessage = msg;
		statusType = type;
		setTimeout(() => {
			statusMessage = "";
			statusType = "";
		}, 3000);
	}

	async function saveSettings() {
		saving = true;
		try {
			await api.updateDiscordNotificationSettings(settings);
			checkConnection();
			setStatus("設定を保存しました", "success");
		} catch (e) {
			setStatus("保存に失敗しました: " + (e as Error).message, "error");
		}
		saving = false;
	}

	async function sendTest() {
		testSending = true;
		try {
			// Also update global notification config to ensure discord is enabled for the test
			await api.updateNotificationConfig({ discord_enabled: true });
			await api.sendTestNotification({ message: testMessage, title: testTitle });
			setStatus("テスト通知を送信しました", "success");
		} catch (e) {
			setStatus("送信に失敗しました: " + (e as Error).message, "error");
		}
		testSending = false;
	}

</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<div class="mb-3 flex items-center justify-between">
		<h3 class="text-sm font-medium text-gray-200">Discord Webhook 通知</h3>
		{#if loading}
			<span class="text-xs text-gray-500">読み込み中...</span>
		{/if}
	</div>

	{#if !loading}
		<div class="space-y-3">
			<!-- Enable toggle -->
			<label class="flex items-center justify-between text-xs text-gray-400">
				<span>Webhook 通知を有効にする</span>
				<input
					type="checkbox"
					bind:checked={settings.enabled}
					class="accent-blue-500"
				/>
			</label>

			<!-- Webhook URL -->
			<label class="flex flex-col gap-1 text-xs text-gray-400">
				<span>Webhook URL</span>
				<input
					type="url"
					bind:value={settings.webhook_url}
					oninput={() => checkConnection()}
					placeholder="https://discord.com/api/webhooks/..."
					class="w-full rounded bg-gray-800 px-2 py-1 font-mono text-gray-200"
				/>
			</label>

			<!-- Connection status -->
			{#if connectionOk === true}
				<div class="flex items-center gap-1 text-xs text-green-400">
					<span class="inline-block h-2 w-2 rounded-full bg-green-500"></span>
					<span>Webhook URL は有効です</span>
				</div>
			{:else if connectionOk === false}
				<div class="flex items-center gap-1 text-xs text-yellow-400">
					<span class="inline-block h-2 w-2 rounded-full bg-yellow-500"></span>
					<span>Webhook URL の形式が無効です（discord.com/api/webhooks/ が必要）</span>
				</div>
			{/if}

			<!-- Username override -->
			<label class="flex flex-col gap-1 text-xs text-gray-400">
				<span>ユーザー名（上書き）</span>
				<input
					type="text"
					bind:value={settings.username}
					placeholder="Poke-Controller Bot"
					class="w-full rounded bg-gray-800 px-2 py-1 text-gray-200"
				/>
			</label>

			<!-- Avatar URL -->
			<label class="flex flex-col gap-1 text-xs text-gray-400">
				<span>アバターURL（上書き）</span>
				<input
					type="url"
					bind:value={settings.avatar_url}
					placeholder="https://example.com/avatar.png"
					class="w-full rounded bg-gray-800 px-2 py-1 text-gray-200"
				/>
			</label>

			<!-- Save button -->
			<button
				onclick={saveSettings}
				disabled={saving}
				class="w-full rounded bg-blue-600 px-3 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50"
			>
				{saving ? "保存中..." : "保存"}
			</button>

			<!-- Status message -->
			{#if statusMessage}
				<div
					class="rounded px-2 py-1 text-xs {statusType === "success"
						? "bg-green-900 text-green-300"
						: "bg-red-900 text-red-300"}"
				>
					{statusMessage}
				</div>
			{/if}

			<!-- Message preview -->
			{#if settings.enabled}
				<hr class="border-gray-700" />
				<h4 class="text-xs font-medium text-gray-400">メッセージプレビュー</h4>
				<div class="rounded bg-gray-800 p-2 text-xs text-gray-300">
					<div class="flex items-start gap-2">
						{#if settings.avatar_url}
							<img
								src={settings.avatar_url}
								alt="avatar"
								class="h-8 w-8 rounded-full"
								onerror={(e) => {
									(e.target as HTMLImageElement).style.display = "none";
								}}
							/>
						{:else}
							<div class="flex h-8 w-8 items-center justify-center rounded-full bg-blue-600 text-xs font-bold text-white">
								{settings.username ? settings.username.charAt(0).toUpperCase() : "P"}
							</div>
						{/if}
						<div>
							<span class="font-medium text-blue-400">{settings.username || "Poke-Controller Bot"}</span>
							<span class="ml-1 text-gray-500">Today at 12:00</span>
							<p class="mt-0.5">{testTitle ? `**${testTitle}**` : ""} {testMessage}</p>
						</div>
					</div>
				</div>

				<!-- Test send -->
				<h4 class="text-xs font-medium text-gray-400">テスト送信</h4>
				<label class="flex items-center justify-between text-xs text-gray-400">
					<span>タイトル</span>
					<input
						type="text"
						bind:value={testTitle}
						class="w-36 rounded bg-gray-800 px-2 py-1 text-gray-200"
					/>
				</label>
				<label class="flex items-center justify-between text-xs text-gray-400">
					<span>メッセージ</span>
					<input
						type="text"
						bind:value={testMessage}
						class="w-36 rounded bg-gray-800 px-2 py-1 text-gray-200"
					/>
				</label>
				<button
					onclick={sendTest}
					disabled={testSending || !settings.webhook_url}
					class="w-full rounded bg-green-700 px-3 py-2 text-sm font-medium text-white hover:bg-green-600 disabled:opacity-50"
				>
					{testSending ? "送信中..." : "テスト通知を送信"}
				</button>
			{/if}
		</div>
	{/if}
</div>