<script lang="ts">
	import CameraPreview from '$lib/components/CameraPreview.svelte';
	import CameraSettings from '$lib/components/CameraSettings.svelte';
	import CaptureRegion from '$lib/components/CaptureRegion.svelte';
	import OutputPanel from '$lib/components/OutputPanel.svelte';
	import LogPanel from '$lib/components/LogPanel.svelte';
	import { onMount } from 'svelte';
	import { api } from '$lib/api/client';

	// ── Types ────────────────────────────────────────────────────────────
	type StickMode = 'left' | 'right';
	type CaptureFormat = 'png' | 'jpeg';

	// ── Reactive state ───────────────────────────────────────────────────
	let cameraOpen = $state(false);
	let showRegionSelector = $state(false);
	let captureName = $state('capture.png');
	let captureFormat = $state<CaptureFormat>('png');
	let previewWidth = $state(640);
	let previewHeight = $state(360);
	let stickMode = $state<StickMode>('left');
	let lStickEnabled = $state(true);

	// Display mode toggles
	let showRealtime = $state(true);
	let showValue = $state(false);
	let showGuide = $state(false);

	let statusMessage = $state<string | null>(null);
	let colorPickResult = $state<string | null>(null);

	// ── Lifecycle ────────────────────────────────────────────────────────
	onMount(() => {
		const handler = (e: Event) => handleCameraState(e as CustomEvent<{ opened: boolean }>);
		const colorHandler = (e: Event) => handleColorPick(e as CustomEvent);
		const captureHandler = (e: Event) => handleCaptureEvent(e as CustomEvent);
		window.addEventListener('camera-state', handler);
		window.addEventListener('camera-colorpick', colorHandler);
		window.addEventListener('camera-capture', captureHandler);
		loadMouseStickSettings();
		return () => {
			window.removeEventListener('camera-state', handler);
			window.removeEventListener('camera-colorpick', colorHandler);
			window.removeEventListener('camera-capture', captureHandler);
		};
	});

	// ── Handlers ─────────────────────────────────────────────────────────
	function handleCameraState(e: CustomEvent<{ opened: boolean }>) {
		cameraOpen = e.detail.opened;
	}

	function handleColorPick(e: CustomEvent<{ hex: string | null; rgb: string | null; x: number; y: number }>) {
		const d = e.detail;
		if (d.hex && d.rgb) {
			colorPickResult = `${d.hex} / ${d.rgb}`;
			showStatus(`Color at (${d.x}, ${d.y}): ${d.hex}`, 5000);
		} else {
			showStatus(`Color picker: position (${d.x}, ${d.y})`, 3000);
		}
	}

	function handleCaptureEvent(e: CustomEvent<{ filename: string; type: string }>) {
		showStatus(`Captured: ${e.detail.filename} (${e.detail.type})`);
	}

	async function loadMouseStickSettings() {
		try {
			const cfg = await api.getMouseStick();
			lStickEnabled = cfg.left_enabled;
			if (cfg.left_enabled) stickMode = 'left';
			else if (cfg.right_enabled) stickMode = 'right';
		} catch {
			// ignore
		}
	}

	async function toggleStickMode() {
		const newMode: StickMode = stickMode === 'left' ? 'right' : 'left';
		stickMode = newMode;
		try {
			await api.setMouseStick({
				stick: newMode,
				enabled: true,
			});
		} catch {
			// ignore
		}
	}

	function showStatus(msg: string, durationMs = 3000) {
		statusMessage = msg;
		setTimeout(() => { statusMessage = null; }, durationMs);
	}

	async function handleCapture() {
		try {
			const ext = captureFormat;
			const name = captureName.includes('.')
				? captureName.replace(/\.(png|jpe?g)$/i, `.${ext}`)
				: `${captureName}.${ext}`;
			await api.captureCamera(name);
			showStatus(`Captured: ${name}`);
		} catch (e) {
			showStatus(`Capture failed: ${e instanceof Error ? e.message : String(e)}`, 5000);
		}
	}

	// ── Format change handler ────────────────────────────────────────────
	function handleFormatChange() {
		const base = captureName.replace(/\.(png|jpe?g)$/i, '');
		captureName = `${base}.${captureFormat}`;
	}
</script>

<div class="tab-content">
	<div class="flex items-center justify-between px-1 py-0.5">
		<h2 class="text-sm font-bold text-gray-200">Camera</h2>
		{#if statusMessage}
			<span class="rounded bg-gray-800 px-2 py-0.5 text-[10px] text-gray-400">{statusMessage}</span>
		{/if}
	</div>

	<div class="grid grid-cols-1 gap-2 lg:grid-cols-5">
		<!-- ── LEFT: Video Preview ──────────────────────────────────── -->
		<div class="space-y-2 lg:col-span-3">
			<CameraPreview
				height={previewHeight}
				{previewWidth}
				{stickMode}
				{lStickEnabled}
			/>

			<!-- Mouse operation bar -->
			<div class="rounded border border-gray-700 bg-gray-900 px-2 py-1.5">
				<div class="flex flex-wrap items-center gap-2">
					<div class="flex items-center gap-1.5 rounded bg-gray-800 px-2 py-0.5">
						<span class="text-[10px] text-gray-500">Stick:</span>
						<button
							onclick={toggleStickMode}
							class="rounded px-2 py-0.5 text-[10px] font-medium transition-colors {stickMode === 'left'
								? 'bg-cyan-800 text-cyan-200'
								: 'bg-red-800 text-red-200'}"
						>
							{stickMode === 'left' ? 'LSTICK' : 'RSTICK'}
						</button>
					</div>
					<span class="text-[9px] text-gray-600">
						Drag=Stick | Ctrl+Click=Color | Ctrl+Shift+Drag=Region SS | Ctrl+Alt+Drag=Named Save
					</span>
				</div>
				{#if colorPickResult}
					<div class="mt-1 text-[10px] text-gray-500">
						Color: <span class="font-mono text-gray-300">{colorPickResult}</span>
					</div>
				{/if}
			</div>
		</div>

		<!-- ── RIGHT: Settings + Capture ────────────────────────────── -->
		<div class="space-y-2 lg:col-span-2">
			<CameraSettings disabled={cameraOpen} />

			<!-- Display Settings -->
			<div class="tk-labelframe">
				<div class="tk-labelframe-label">Display Settings</div>
				<div class="tk-labelframe-content">
					<div class="form-row">
						<label class="tk-checkbox-label">
							<input type="checkbox" bind:checked={showRealtime} class="tk-checkbox" />
							<span>Realtime</span>
						</label>
						<label class="tk-checkbox-label">
							<input type="checkbox" bind:checked={showValue} class="tk-checkbox" />
							<span>Value</span>
						</label>
						<label class="tk-checkbox-label">
							<input type="checkbox" bind:checked={showGuide} class="tk-checkbox" />
							<span>Guide</span>
						</label>
					</div>
				</div>
			</div>

			<!-- Capture Controls -->
			<div class="tk-labelframe">
				<div class="tk-labelframe-label">Capture</div>
				<div class="tk-labelframe-content">
					<div class="form-row">
						<input
							bind:value={captureName}
							class="tk-input flex-1"
							placeholder="capture.png"
						/>
						<select
							bind:value={captureFormat}
							onchange={handleFormatChange}
							class="tk-select"
						>
							<option value="png">PNG</option>
							<option value="jpeg">JPEG</option>
						</select>
						<button onclick={handleCapture} class="tk-btn">Capture</button>
					</div>
					<div class="form-row" style="padding-top: 4px;">
						<button
							onclick={() => (showRegionSelector = !showRegionSelector)}
							class="tk-btn"
						>
							{showRegionSelector ? 'Close Region Selector' : 'Select Capture Region'}
						</button>
					</div>
				</div>
			</div>
		</div>
	</div>

	{#if showRegionSelector}
		<div class="rounded border border-gray-700 bg-gray-900 p-2">
			<h3 class="mb-1 text-[11px] font-medium text-gray-500">Capture Region Selection</h3>
			<CaptureRegion width={previewWidth} height={previewHeight} />
		</div>
	{/if}

	<div class="grid grid-cols-1 gap-2 lg:grid-cols-2">
		<OutputPanel title="Camera Output" />
		<LogPanel />
	</div>
</div>

<style>
	/* No page-level canvas styles needed anymore — CameraPreview owns the overlay */
</style>
