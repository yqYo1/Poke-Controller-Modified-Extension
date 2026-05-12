<script lang="ts">
	import SoftwareController from '$lib/components/SoftwareController.svelte';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">手動制御</h2>

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<SoftwareController />

		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">マウススティック制御</h3>
			<p class="text-xs text-gray-500">マウススティック制御の設定は「その他」タブで行えます。</p>
		</div>
	</div>

	<OutputPanel title="制御出力" />
	<LogPanel />
</div>
