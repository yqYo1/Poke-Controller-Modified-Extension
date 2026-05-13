<script lang="ts">
	import SoftwareController from '$lib/components/SoftwareController.svelte';
	import ControllerSimulator from '$lib/components/ControllerSimulator.svelte';
	import SoftwareControl from '$lib/components/SoftwareControl.svelte';
	import HardwareControl from '$lib/components/HardwareControl.svelte';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';

	let showSimulator = $state(false);
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">手動制御</h2>

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<!-- Main Joy-Con controller -->
		<SoftwareController />

		<!-- Software control settings (keyboard, mouse stick, key mapping) -->
		<SoftwareControl />
	</div>

	<!-- Hardware control (gamepad, recording, vibration) -->
	<HardwareControl />

	<!-- Controller simulator toggle -->
	<div class="flex items-center gap-2">
		<button
			onclick={() => (showSimulator = !showSimulator)}
			class="rounded px-3 py-1.5 text-xs font-medium transition-colors {showSimulator
				? 'bg-blue-600 text-white'
				: 'border border-gray-600 text-gray-300 hover:bg-gray-700'}"
		>
			{showSimulator ? '▲ シミュレーターを隠す' : '▼ コントローラーシミュレーターを表示'}
		</button>
	</div>

	<!-- Controller simulator (toggleable sub-window) -->
	<ControllerSimulator visible={showSimulator} />

	<OutputPanel title="制御出力" />
	<LogPanel />
</div>
