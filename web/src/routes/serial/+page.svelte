<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';
	import SerialMonitor from '$lib/components/SerialMonitor.svelte';

	let ports = $state<{ port_num: number; port_name: string }[]>([]);
	let selectedNum = $state(0);
	let selectedName = $state('');
	let baudrate = $state(115200);
	let connected = $state(false);

	const baudrates = [9600, 19200, 38400, 57600, 115200, 230400];

	onMount(() => {
		refreshPorts();
	});

	async function refreshPorts() {
		try {
			const list = await api.getSerialPorts();
			ports = list;
			if (list.length > 0) {
				selectedNum = list[0].port_num;
				selectedName = list[0].port_name;
			}
		} catch { ports = []; }
	}

	async function toggleSerial() {
		if (connected) {
			await api.closeSerial();
			connected = false;
		} else {
			await api.openSerial({ port_num: selectedNum, port_name: selectedName, baudrate });
			connected = true;
		}
	}
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">シリアル通信</h2>

	<div class="rounded border border-gray-700 bg-gray-900 p-3">
		<h3 class="mb-2 text-sm font-medium text-gray-200">ポート設定</h3>
		<div class="space-y-2">
			<div class="flex items-center justify-between">
				<span class="text-xs text-gray-400">ポート</span>
				<div class="flex gap-2">
					<select bind:value={selectedName} onchange={() => {
						const p = ports.find((x) => x.port_name === selectedName);
						if (p) selectedNum = p.port_num;
					}} class="rounded bg-gray-800 px-2 py-1 text-xs text-gray-200">
						{#each ports as port (port.port_num)}
							<option value={port.port_name}>{port.port_name}</option>
						{/each}
					</select>
					<button onclick={refreshPorts} class="rounded bg-gray-700 px-2 py-1 text-xs text-gray-300 hover:bg-gray-600">更新</button>
				</div>
			</div>
			<div class="flex items-center justify-between">
				<span class="text-xs text-gray-400">ボーレート</span>
				<select bind:value={baudrate} class="rounded bg-gray-800 px-2 py-1 text-xs text-gray-200">
					{#each baudrates as b (b)}
						<option value={b}>{b}</option>
					{/each}
				</select>
			</div>
			<button
				onclick={toggleSerial}
				class="w-full rounded px-3 py-2 text-sm font-medium text-white {connected ? 'bg-red-600 hover:bg-red-500' : 'bg-blue-600 hover:bg-blue-500'}"
			>
				{connected ? 'シリアルを閉じる' : 'シリアルを開く'}
			</button>
		</div>
	</div>

	<SerialMonitor {connected} {baudrate} portName={selectedName} />

	<OutputPanel title="シリアル出力" />
	<LogPanel />
</div>
