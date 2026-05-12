<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import LogPanel from '$lib/components/LogPanel.svelte';

	// ── Pokemon Home state ─────────────────────────────────────────────────────
	let connected = $state(false);
	let connecting = $state(false);
	let retrieving = $state(false);
	let message = $state<string | null>(null);

	// Box / Pokemon data
	let boxes = $state<string[]>([]);
	let selectedBox = $state(1);
	let boxData = $state<{ slot: number; name: string; species: string; shiny: boolean }[]>([]);

	// Import/Export
	let exportFormat = $state('pkhex');
	let importFilePath = $state('');

	const boxNumbers = Array.from({ length: 32 }, (_, i) => i + 1);

	onMount(() => {
		checkConnection();
	});

	async function checkConnection() {
		try {
			// TODO: Implement dedicated Pokemon Home status endpoint
			// For now, assume connected if we can reach the API
			await api.getStatus();
			connected = true;
		} catch {
			connected = false;
		}
	}

	async function connectToHome() {
		connecting = true;
		message = null;
		try {
			// Send a mock Home connectivity request via generic input
			await api.sendInput('pokemon_home', { action: 'connect' });
			connected = true;
			message = 'Pokemon Homeに接続しました';
			await loadBoxList();
		} catch (e) {
			message = `接続エラー: ${e}`;
			connected = false;
		}
		connecting = false;
		setTimeout(() => (message = null), 3000);
	}

	async function disconnectFromHome() {
		try {
			await api.sendInput('pokemon_home', { action: 'disconnect' });
			connected = false;
			boxes = [];
			boxData = [];
			message = 'Pokemon Homeから切断しました';
		} catch (e) {
			message = `切断エラー: ${e}`;
		}
		setTimeout(() => (message = null), 3000);
	}

	async function loadBoxList() {
		// TODO: Replace with actual Pokemon Home box list API when available
		// For now, generate placeholder box names
		boxes = Array.from({ length: 32 }, (_, i) => `ボックス ${i + 1}`);
	}

	async function retrieveBox() {
		retrieving = true;
		message = null;
		try {
			await api.sendInput('pokemon_home', { action: 'retrieve_box', box: selectedBox });
			// Simulate box data retrieval
			boxData = Array.from({ length: 30 }, (_, i) => ({
				slot: i + 1,
				name: `ポケモン ${i + 1}`,
				species: `サンプル種 ${i + 1}`,
				shiny: Math.random() < 0.1,
			}));
			message = `ボックス ${selectedBox} のデータを取得しました`;
		} catch (e) {
			message = `取得エラー: ${e}`;
		}
		retrieving = false;
		setTimeout(() => (message = null), 3000);
	}

	async function exportData() {
		try {
			await api.sendInput('pokemon_home', { action: 'export', format: exportFormat });
			message = `${exportFormat.toUpperCase()} 形式でエクスポートしました`;
		} catch (e) {
			message = `エクスポートエラー: ${e}`;
		}
		setTimeout(() => (message = null), 3000);
	}

	async function importData() {
		if (!importFilePath) return;
		try {
			await api.sendInput('pokemon_home', { action: 'import', path: importFilePath });
			message = 'データをインポートしました';
		} catch (e) {
			message = `インポートエラー: ${e}`;
		}
		setTimeout(() => (message = null), 3000);
	}
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">Pokemon Home連携</h2>

	{#if message}
		<div class="rounded border border-green-700 bg-green-900/50 px-3 py-2 text-xs text-green-300">
			{message}
		</div>
	{/if}

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<!-- 接続管理 -->
		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">接続管理</h3>
			<div class="space-y-3">
				<div class="flex items-center gap-2">
					<span
						class="inline-block h-2 w-2 rounded-full {connected ? 'bg-green-500' : 'bg-gray-600'}"
					></span>
					<span class="text-xs text-gray-400">
						{connected ? '接続済み' : '未接続'}
					</span>
				</div>
				{#if connected}
					<button
						onclick={disconnectFromHome}
						class="w-full rounded bg-red-600 px-3 py-2 text-sm font-medium text-white hover:bg-red-500"
					>
						切断
					</button>
				{:else}
					<button
						onclick={connectToHome}
						disabled={connecting}
						class="w-full rounded bg-blue-600 px-3 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50"
					>
						{connecting ? '接続中...' : 'Pokemon Homeに接続'}
					</button>
				{/if}
			</div>
		</div>

		<!-- ボックス選択 -->
		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">ボックスデータ取得</h3>
			<div class="space-y-3">
				<label class="flex items-center justify-between text-xs text-gray-400">
					<span>ボックス番号</span>
					<select
						bind:value={selectedBox}
						class="rounded bg-gray-800 px-2 py-1 text-gray-200"
						disabled={!connected}
					>
						{#each boxNumbers as num}
							<option value={num}>ボックス {num}</option>
						{/each}
					</select>
				</label>
				<button
					onclick={retrieveBox}
					disabled={!connected || retrieving}
					class="w-full rounded bg-green-700 px-3 py-2 text-sm font-medium text-white hover:bg-green-600 disabled:opacity-50"
				>
					{retrieving ? '取得中...' : 'ボックスデータを取得'}
				</button>
			</div>
		</div>

		<!-- インポート/エクスポート -->
		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">インポート / エクスポート</h3>
			<div class="space-y-3">
				<label class="flex items-center justify-between text-xs text-gray-400">
					<span>エクスポート形式</span>
					<select bind:value={exportFormat} class="rounded bg-gray-800 px-2 py-1 text-gray-200">
						<option value="pkhex">PKHeX</option>
						<option value="pk8">PK8</option>
						<option value="json">JSON</option>
						<option value="csv">CSV</option>
					</select>
				</label>
				<button
					onclick={exportData}
					disabled={!connected}
					class="w-full rounded bg-blue-600 px-3 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50"
				>
					エクスポート
				</button>
				<div class="border-t border-gray-700 pt-2">
					<label class="mb-1 block text-xs text-gray-400">インポートファイル</label>
					<div class="flex gap-2">
						<input
							bind:value={importFilePath}
							placeholder="ファイルパスを入力..."
							class="flex-1 rounded bg-gray-800 px-2 py-1 text-xs text-gray-200 placeholder-gray-500"
						/>
						<button
							onclick={importData}
							disabled={!connected || !importFilePath}
							class="rounded bg-purple-700 px-3 py-1 text-xs text-white hover:bg-purple-600 disabled:opacity-50"
						>
							インポート
						</button>
					</div>
				</div>
			</div>
		</div>

		<!-- 取得データ表示 -->
		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">ボックスデータ</h3>
			{#if boxData.length > 0}
				<div class="max-h-80 overflow-y-auto">
					<table class="w-full text-xs">
						<thead>
							<tr class="text-left text-gray-500">
								<th class="px-2 py-1">スロット</th>
								<th class="px-2 py-1">名前</th>
								<th class="px-2 py-1">種類</th>
								<th class="px-2 py-1">色違い</th>
							</tr>
						</thead>
						<tbody>
							{#each boxData as entry}
								<tr class="border-t border-gray-800 hover:bg-gray-800">
									<td class="px-2 py-1 text-gray-400">{entry.slot}</td>
									<td class="px-2 py-1 text-gray-200">{entry.name}</td>
									<td class="px-2 py-1 text-gray-300">{entry.species}</td>
									<td class="px-2 py-1">
										{#if entry.shiny}
											<span class="text-yellow-400">★</span>
										{:else}
											<span class="text-gray-600">-</span>
										{/if}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{:else}
				<p class="text-xs text-gray-500">ボックスデータを取得するとここに表示されます</p>
			{/if}
		</div>
	</div>

	<LogPanel />
</div>
