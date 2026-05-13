<script lang="ts">
	import CameraPreview from '$lib/components/CameraPreview.svelte';
	import CaptureRegion from '$lib/components/CaptureRegion.svelte';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';

	let captureName = $state('capture.png');
	let showRegionSelector = $state(false);
	let previewWidth = $state(640);
	let previewHeight = $state(480);

	async function handleCapture() {
		try {
			const { api } = await import('$lib/api/client');
			await api.captureCamera(captureName);
		} catch (e) {
			console.error('Failed to capture camera:', e);
		}
	}
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">カメラ</h2>

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<div class="space-y-3">
			<!-- Camera Preview (WebRTC primary, MJPEG fallback) -->
			<CameraPreview height={previewHeight} />

			<div class="rounded border border-gray-700 bg-gray-900 p-3">
				<h3 class="mb-2 text-sm font-medium text-gray-200">キャプチャ</h3>
				<div class="space-y-2">
					<div class="flex items-center gap-2">
						<input bind:value={captureName} class="flex-1 rounded bg-gray-800 px-2 py-1 text-xs text-gray-200" />
						<button onclick={handleCapture} class="rounded bg-green-700 px-3 py-1 text-xs text-white hover:bg-green-600">保存</button>
					</div>
					<button
						onclick={() => (showRegionSelector = !showRegionSelector)}
						class="w-full rounded bg-gray-700 px-3 py-1 text-xs text-gray-300 hover:bg-gray-600"
					>
						{showRegionSelector ? '範囲選択を閉じる' : 'キャプチャ範囲を選択'}
					</button>
				</div>
			</div>
		</div>

		{#if showRegionSelector}
			<div class="rounded border border-gray-700 bg-gray-900 p-3">
				<h3 class="mb-2 text-sm font-medium text-gray-200">キャプチャ範囲選択</h3>
				<CaptureRegion
					width={previewWidth}
					height={previewHeight}
				/>
			</div>
		{/if}
	</div>

	<OutputPanel title="カメラ出力" />
	<LogPanel />
</div>
