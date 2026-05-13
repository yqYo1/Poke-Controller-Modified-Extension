<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "$lib/api/client";
	import type { WindowsNotificationSettings } from "$lib/api/client";

	let settings = $state<WindowsNotificationSettings>({
		enabled: false,
		duration_secs: 5,
		sound_enabled: true,
		priority: "Default",
		app_id: "Poke-Controller",
	});
	let loading = $state(true);
	let saving = $state(false);
	let testSending = $state(false);
	let testMessage = $state("テスト通知");
	let testTitle = $state("Poke-Controller");
	let statusMessage = $state("");
	let statusType = $state<"success" | "error" | "">("");

	const priorities = ["Default", "High", "Critical"];

	onMount(() => {
		loadSettings();
	});

	async function loadSettings() {
		try {
			const result = await api.getWindowsNotificationSettings();
			settings = { ...settings, ...result };
		} catch (e) {
			console.warn("Failed to load Windows notification settings:", e);
		}
		loading = false;
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
			await api.updateWindowsNotificationSettings(settings);
			setStatus("設定を保存しました", "success");
		} catch (e) {
			setStatus("保存に失敗しました: " + (e as Error).message, "error");
		}
		saving = false;
	}

	async function sendTest() {
		testSending = true;
		try {
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
		<h3 class="text-sm font-medium text-gray-200">Windows トースト通知</h3>
		{#if loading}
			<span class="text-xs text-gray-500">読み込み中...</span>
		{/if}
	</div>

	{#if !loading}
		<div class="space-y-3">
			<!-- Enable toggle -->
			<label class="flex items-center justify-between text-xs text-gray-400">
				<span>通知を有効にする</span>
				<input
					type="checkbox"
					bind:checked={settings.enabled}
					class="accent-blue-500"
				/>
			</label>

			<!-- Duration -->
			<label class="flex items-center justify-between text-xs text-gray-400">
				<span>表示時間（秒）</span>
				<input
					type="number"
					bind:value={settings.duration_secs}
					min="1"
					max="30"
					class="w-20 rounded bg-gray-800 px-2 py-1 text-gray-200"
				/>
			</label>

			<!-- Sound toggle -->
			<label class="flex items-center justify-between text-xs text-gray-400">
				<span>通知音を鳴らす</span>
				<input
					type="checkbox"
					bind:checked={settings.sound_enabled}
					class="accent-blue-500"
				/>
			</label>

			<!-- Priority -->
			<label class="flex items-center justify-between text-xs text-gray-400">
				<span>優先度</span>
				<select
					bind:value={settings.priority}
					class="w-36 rounded bg-gray-800 px-2 py-1 text-gray-200"
				>
					{#each priorities as p}
						<option value={p}>{p}</option>
					{/each}
				</select>
			</label>

			<!-- App ID -->
			<label class="flex items-center justify-between text-xs text-gray-400">
				<span>アプリID</span>
				<input
					type="text"
					bind:value={settings.app_id}
					placeholder="Poke-Controller"
					class="w-36 rounded bg-gray-800 px-2 py-1 text-gray-200"
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

			<!-- Test send section -->
			<hr class="border-gray-700" />
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
				disabled={testSending}
				class="w-full rounded bg-green-700 px-3 py-2 text-sm font-medium text-white hover:bg-green-600 disabled:opacity-50"
			>
				{testSending ? "送信中..." : "テスト通知を送信"}
			</button>
		</div>
	{/if}
</div>