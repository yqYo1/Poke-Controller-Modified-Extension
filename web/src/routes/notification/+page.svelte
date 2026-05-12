<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import LogPanel from '$lib/components/LogPanel.svelte';

	let config = $state<Record<string, unknown>>({});
	let testMessage = $state('テスト通知');
	let testTitle = $state('Poke-Controller');

	onMount(() => {
		api.getNotificationConfig().then((c) => {
			config = c;
		}).catch(console.warn);
	});

	async function saveConfig() {
		await api.updateNotificationConfig(config);
	}

	async function sendTest() {
		await api.sendTestNotification({ message: testMessage, title: testTitle });
	}
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">通知設定</h2>

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">設定</h3>
			<div class="space-y-2">
				{#each Object.entries(config) as [key, value]}
					<label class="flex items-center justify-between text-xs text-gray-400">
						<span>{key}</span>
						<input
							bind:value={config[key]}
							oninput={() => (config = { ...config })}
							class="w-48 rounded bg-gray-800 px-2 py-1 text-gray-200"
						/>
					</label>
				{/each}
				<button onclick={saveConfig} class="w-full rounded bg-blue-600 px-3 py-2 text-sm font-medium text-white hover:bg-blue-500">
					保存
				</button>
			</div>
		</div>

		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">テスト送信</h3>
			<div class="space-y-2">
				<label class="flex items-center justify-between text-xs text-gray-400">
					<span>タイトル</span>
					<input bind:value={testTitle} class="w-48 rounded bg-gray-800 px-2 py-1 text-gray-200" />
				</label>
				<label class="flex items-center justify-between text-xs text-gray-400">
					<span>メッセージ</span>
					<input bind:value={testMessage} class="w-48 rounded bg-gray-800 px-2 py-1 text-gray-200" />
				</label>
				<button onclick={sendTest} class="w-full rounded bg-green-700 px-3 py-2 text-sm font-medium text-white hover:bg-green-600">
					テスト通知を送信
				</button>
			</div>
		</div>
	</div>

	<LogPanel />
</div>
