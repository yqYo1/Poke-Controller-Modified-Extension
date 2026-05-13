<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';
	import CaptureRegion from '$lib/components/CaptureRegion.svelte';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';

	let cameras = $state<{ device_index: number; name: string }[]>([]);
	let selectedIndex = $state(0);
	let cameraOpen = $state(false);
	let frameWidth = $state(640);
	let frameHeight = $state(480);
	let frameSrc = $state<string | null>(null);
	let captureName = $state('capture.png');
	let showRegionSelector = $state(false);

	onMount(() => {
		api.getCameras().then((list) => {
			cameras = list;
			if (list.length > 0) selectedIndex = list[0].device_index;
		}).catch(console.warn);
	});

	async function toggleCamera() {
		if (cameraOpen) {
			await api.closeCamera();
			cameraOpen = false;
			frameSrc = null;
		} else {
			await api.openCamera({ device_index: selectedIndex, width: frameWidth, height: frameHeight });
			cameraOpen = true;
			refreshFrame();
		}
	}

	async function refreshFrame() {
		if (!cameraOpen) return;
		try {
			const res = await api.getCameraFrame();
			frameSrc = `data:image/jpeg;base64,${res.frame}`;
		} catch { frameSrc = null; }
		if (cameraOpen) setTimeout(refreshFrame, 100);
	}

	async function handleCapture() {
		await api.captureCamera(captureName);
	}
</script>

<div class="space-y-4">
	<h2 class="text-lg font-bold">カメラ</h2>

	<div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
		<div class="space-y-3">
			<div class="rounded border border-gray-700 bg-gray-900 p-3">
				<h3 class="mb-2 text-sm font-medium text-gray-200">カメラ設定</h3>
				<div class="space-y-2">
					<label class="flex items-center justify-between text-xs text-gray-400">
						<span>デバイス</span>
						<select bind:value={selectedIndex} class="w-48 rounded bg-gray-800 px-2 py-1 text-gray-200">
							{#each cameras as cam (cam.device_index)}
								<option value={cam.device_index}>{cam.name}</option>
							{/each}
						</select>
					</label>
					<label class="flex items-center justify-between text-xs text-gray-400">
						<span>幅</span>
						<input type="number" bind:value={frameWidth} class="w-20 rounded bg-gray-800 px-2 py-1 text-right text-gray-200" />
					</label>
					<label class="flex items-center justify-between text-xs text-gray-400">
						<span>高さ</span>
						<input type="number" bind:value={frameHeight} class="w-20 rounded bg-gray-800 px-2 py-1 text-right text-gray-200" />
					</label>
					<button
						onclick={toggleCamera}
						class="w-full rounded px-3 py-2 text-sm font-medium text-white {cameraOpen ? 'bg-red-600 hover:bg-red-500' : 'bg-blue-600 hover:bg-blue-500'}"
					>
						{cameraOpen ? 'カメラを閉じる' : 'カメラを開く'}
					</button>
				</div>
			</div>

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

		<div class="rounded border border-gray-700 bg-gray-900 p-3">
			<h3 class="mb-2 text-sm font-medium text-gray-200">プレビュー</h3>
			<div class="flex aspect-video items-center justify-center rounded bg-gray-800">
				{#if frameSrc}
					<img src={frameSrc} alt="Camera preview" class="max-h-full max-w-full rounded" />
				{:else}
					<span class="text-xs text-gray-500">カメラを開くとプレビューが表示されます</span>
				{/if}
			</div>
		</div>

		{#if showRegionSelector}
			<div class="rounded border border-gray-700 bg-gray-900 p-3">
				<h3 class="mb-2 text-sm font-medium text-gray-200">キャプチャ範囲選択</h3>
				<CaptureRegion
					width={frameWidth}
					height={frameHeight}
					imageSrc={frameSrc}
				/>
			</div>
		{/if}
	</div>

	<OutputPanel title="カメラ出力" />
	<LogPanel />
</div>
