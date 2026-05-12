<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { api, wsClient as wsClientSingleton } from '$lib/api/client';

	let wsConnected = $state(false);
	let serialConnected = $state(false);
	let cameraOpened = $state(false);
	let wsClient: WebSocketClient | null = null;

	function updateStatus() {
		api.getStatus()
			.then((s) => {
				wsConnected = s.ws_connected;
				serialConnected = s.serial;
				cameraOpened = s.camera;
			})
			.catch(() => {
				wsConnected = false;
				serialConnected = false;
				cameraOpened = false;
			});
	}

	onMount(() => {
		const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
		wsClient = wsClientSingleton;

		wsClient.on('connect', () => {
			wsConnected = true;
			updateStatus();
		});
		wsClient.on('disconnect', () => {
			wsConnected = false;
		});
		wsClient.on('message', () => {
			updateStatus();
		});
		wsClient.connect();
		updateStatus();

		const interval = setInterval(updateStatus, 5000);

		onDestroy(() => {
			clearInterval(interval);
			wsClient?.disconnect();
		});
	});
</script>

<footer class="flex items-center gap-4 bg-gray-900 px-4 py-2 text-xs text-gray-300">
	<div class="flex items-center gap-1">
		<span class="inline-block h-2 w-2 rounded-full {wsConnected ? 'bg-green-500' : 'bg-red-500'}"></span>
		<span>WebSocket: {wsConnected ? '接続済み' : '未接続'}</span>
	</div>
	<div class="flex items-center gap-1">
		<span class="inline-block h-2 w-2 rounded-full {serialConnected ? 'bg-green-500' : 'bg-red-500'}"></span>
		<span>シリアル: {serialConnected ? '接続済み' : '未接続'}</span>
	</div>
	<div class="flex items-center gap-1">
		<span class="inline-block h-2 w-2 rounded-full {cameraOpened ? 'bg-green-500' : 'bg-red-500'}"></span>
		<span>カメラ: {cameraOpened ? '有効' : '無効'}</span>
	</div>
</footer>
