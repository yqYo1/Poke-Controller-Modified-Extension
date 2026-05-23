<script lang="ts">
	import SoftwareController from '$lib/components/SoftwareController.svelte';
	import SoftwareControl from '$lib/components/SoftwareControl.svelte';
	import HardwareControl from '$lib/components/HardwareControl.svelte';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">手動制御</h2>

	<!-- SPEC §4.3.1 + §4.3.3: Software Control + Switch Controller Simulator -->
	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<!-- §4.3.3 Switch Controller Simulator: Full Joy-Con layout -->
		<SoftwareController />

		<!-- §4.3.1 Software Control: Keyboard + LStick/RStick Mouse checkboxes -->
		<SoftwareControl />
	</div>

	<!-- §4.3.2 Hardware Control: ProController/Xinput radio + Record checkbox -->
	<HardwareControl />

	<OutputPanel title="制御出力" />
	<LogPanel />
</div>
