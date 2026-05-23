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
	type MouseAction = 'none' | 'stick' | 'colorpicker' | 'rangescreenshot' | 'namedsave';

	// ── Reactive state ───────────────────────────────────────────────────
	let cameraOpen = $state(false);
	let showRegionSelector = $state(false);
	let captureName = $state('capture.png');
	let captureFormat = $state<CaptureFormat>('png');
	let previewWidth = $state(640);
	let previewHeight = $state(360);

	// Mouse operation mode
	let stickMode = $state<StickMode>('left');
	let lStickEnabled = $state(true);

	// Display mode toggles
	let showRealtime = $state(true);
	let showValue = $state(false);
	let showGuide = $state(false);

	// Canvas overlay state
	let canvasEl = $state<HTMLCanvasElement | null>(null);

	// Drag state (individual reactive vars to avoid complex object typing issues)
	let dragActive = $state(false);
	let dragStartX = $state(0);
	let dragStartY = $state(0);
	let dragCurX = $state(0);
	let dragCurY = $state(0);
	let dragAction = $state<MouseAction>('none');

	let colorPickResult = $state<string | null>(null);
	let statusMessage = $state<string | null>(null);

	// ── Lifecycle ────────────────────────────────────────────────────────
	onMount(() => {
		const handler = (e: Event) => handleCameraState(e as CustomEvent<{ opened: boolean }>);
		window.addEventListener('camera-state', handler);
		loadMouseStickSettings();
		return () => {
			window.removeEventListener('camera-state', handler);
		};
	});

	// ── Handlers ─────────────────────────────────────────────────────────
	function handleCameraState(e: CustomEvent<{ opened: boolean }>) {
		cameraOpen = e.detail.opened;
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

	// ── Canvas operations ────────────────────────────────────────────────
	function getCanvasPos(e: MouseEvent): { x: number; y: number } {
		if (!canvasEl) return { x: 0, y: 0 };
		const rect = canvasEl.getBoundingClientRect();
		return {
			x: (e.clientX - rect.left) * (canvasEl.width / rect.width),
			y: (e.clientY - rect.top) * (canvasEl.height / rect.height),
		};
	}

	function resetDrag() {
		dragActive = false;
		dragStartX = 0;
		dragStartY = 0;
		dragCurX = 0;
		dragCurY = 0;
		dragAction = 'none';
	}

	function handleCanvasMouseDown(e: MouseEvent) {
		if (!canvasEl || !cameraOpen) return;

		const pos = getCanvasPos(e);

		// Determine action based on modifier keys
		if (e.ctrlKey && e.shiftKey) {
			dragAction = 'rangescreenshot';
		} else if (e.ctrlKey && e.altKey) {
			dragAction = 'namedsave';
		} else if (e.ctrlKey) {
			// Color picker — single click, no drag
			pickColor(pos.x, pos.y);
			return;
		} else {
			dragAction = 'stick';
			if (!lStickEnabled) return;
		}

		dragActive = true;
		dragStartX = pos.x;
		dragStartY = pos.y;
		dragCurX = pos.x;
		dragCurY = pos.y;

		if (dragAction === 'stick') {
			// Send initial center position
			api.sendInput('stick', { stick: stickMode, x: 128, y: 128 }).catch(() => {});
		}

		drawOverlay();
	}

	function handleCanvasMouseMove(e: MouseEvent) {
		if (!dragActive || !canvasEl) return;
		const pos = getCanvasPos(e);
		dragCurX = pos.x;
		dragCurY = pos.y;

		if (dragAction === 'stick') {
			// Calculate stick values based on drag offset from start
			const dx = (pos.x - dragStartX) / (canvasEl.width / 2);
			const dy = (pos.y - dragStartY) / (canvasEl.height / 2);
			const stickX = Math.round(Math.min(255, Math.max(0, 128 + dx * 127)));
			const stickY = Math.round(Math.min(255, Math.max(0, 128 + dy * 127)));
			api.sendInput('stick', { stick: stickMode, x: stickX, y: stickY }).catch(() => {});
		}

		drawOverlay();
	}

	function handleCanvasMouseUp() {
		if (!dragActive) return;

		if (dragAction === 'rangescreenshot' || dragAction === 'namedsave') {
			const w = Math.abs(dragCurX - dragStartX);
			const h = Math.abs(dragCurY - dragStartY);
			if (w > 10 && h > 10) {
				if (dragAction === 'namedsave') {
					const name = prompt('Save capture as:', 'capture');
					if (name) {
						api.captureCamera(`${name}.${captureFormat}`).catch(console.warn);
						showStatus(`Named capture: ${name}.${captureFormat}`);
					}
				} else {
					const filename = `capture_region_${Date.now()}.${captureFormat}`;
					api.captureCamera(filename).catch(console.warn);
					showStatus(`Region captured: ${filename}`);
				}
			}
		}

		// Reset stick to center on release
		if (dragAction === 'stick') {
			api.sendInput('stick', { stick: stickMode, x: 128, y: 128 }).catch(() => {});
		}

		resetDrag();
		drawOverlay();
	}

	// ── Color picker ─────────────────────────────────────────────────────
	function pickColor(x: number, y: number) {
		if (!canvasEl) return;
		const ctx = canvasEl.getContext('2d');
		if (!ctx) return;
		try {
			const pixel = ctx.getImageData(Math.round(x), Math.round(y), 1, 1).data;
			const hex = `#${pixel[0].toString(16).padStart(2, '0')}${pixel[1].toString(16).padStart(2, '0')}${pixel[2].toString(16).padStart(2, '0')}`;
			const rgb = `rgb(${pixel[0]}, ${pixel[1]}, ${pixel[2]})`;
			colorPickResult = `${hex} / ${rgb}`;
			showStatus(`Color at (${Math.round(x)}, ${Math.round(y)}): ${hex}`, 5000);
		} catch {
			colorPickResult = 'Cross-origin canvas — color picker unavailable';
		}
		drawOverlay();
	}

	// ── Canvas overlay drawing ───────────────────────────────────────────
	function drawOverlay() {
		if (!canvasEl) return;
		const ctx = canvasEl.getContext('2d');
		if (!ctx) return;

		ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
		if (!dragActive) return;

		const sx = dragStartX;
		const sy = dragStartY;
		const cx = dragCurX;
		const cy = dragCurY;

		if (dragAction === 'stick') {
			// Draw stick indicator at current position
			ctx.beginPath();
			ctx.arc(cx, cy, 16, 0, Math.PI * 2);
			const color = stickMode === 'left' ? 'rgba(86, 204, 242, 0.35)' : 'rgba(233, 81, 78, 0.35)';
			const stroke = stickMode === 'left' ? '#56CCF2' : '#E9514E';
			ctx.fillStyle = color;
			ctx.fill();
			ctx.strokeStyle = stroke;
			ctx.lineWidth = 2;
			ctx.stroke();

			// Crosshair
			ctx.beginPath();
			ctx.moveTo(cx - 10, cy);
			ctx.lineTo(cx + 10, cy);
			ctx.moveTo(cx, cy - 10);
			ctx.lineTo(cx, cy + 10);
			ctx.strokeStyle = '#fff';
			ctx.lineWidth = 1.5;
			ctx.stroke();

			// Label
			ctx.fillStyle = 'rgba(255,255,255,0.85)';
			ctx.font = '11px sans-serif';
			ctx.textAlign = 'left';
			ctx.fillText(stickMode === 'left' ? 'LSTICK' : 'RSTICK', cx + 20, cy + 4);
		} else {
			const x = Math.min(sx, cx);
			const y = Math.min(sy, cy);
			const w = Math.abs(cx - sx);
			const h = Math.abs(cy - sy);
			if (w < 2 || h < 2) return;

			// Dim outside region
			ctx.fillStyle = 'rgba(0,0,0,0.35)';
			ctx.fillRect(0, 0, canvasEl.width, y);
			ctx.fillRect(0, y, x, h);
			ctx.fillRect(x + w, y, canvasEl.width - x - w, h);
			ctx.fillRect(0, y + h, canvasEl.width, canvasEl.height - y - h);

			const borderColor = dragAction === 'namedsave' ? '#22c55e' : '#3b82f6';
			ctx.strokeStyle = borderColor;
			ctx.lineWidth = 2;
			ctx.strokeRect(x, y, w, h);

			// Dimension label
			if (w > 50 && h > 18) {
				ctx.fillStyle = dragAction === 'namedsave' ? 'rgba(34,197,94,0.9)' : 'rgba(59,130,246,0.9)';
				ctx.font = '11px monospace';
				ctx.textAlign = 'left';
				ctx.fillText(`${Math.round(w)}\u00D7${Math.round(h)}`, x + 4, y + 14);
			}
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
			<div class="relative overflow-hidden rounded border border-gray-700 bg-gray-900">
				<CameraPreview height={previewHeight} />

				<!-- Transparent Canvas overlay for mouse ops -->
				<canvas
					bind:this={canvasEl}
					width={previewWidth}
					height={previewHeight}
					onmousedown={handleCanvasMouseDown}
					onmousemove={handleCanvasMouseMove}
					onmouseup={handleCanvasMouseUp}
					onmouseleave={handleCanvasMouseUp}
					class="absolute inset-0"
					class:pointer-events-none={!cameraOpen}
					role="img"
					aria-label="Camera mouse operations"
				/>
			</div>

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
						Drag=Stick | Ctrl+Click=Color | Ctrl+Shift+Click=Region SS | Ctrl+Alt+Click=Named Save
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
	canvas.pointer-events-none {
		pointer-events: none;
	}
</style>
