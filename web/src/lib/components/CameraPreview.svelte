<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { wsClient } from '$lib/api/websocket';
	import { WebRTCVideoClient, type VideoTrackState } from '$lib/api/webrtc-video';
	import { api } from '$lib/api/client';

	interface CameraInfo {
		device_index: number;
		name: string;
	}

	// ── Props ───────────────────────────────────────────────────────────────────
	let {
		height = 360,
		previewWidth = 640,
		stickMode = 'left',
		lStickEnabled = true,
	}: {
		height?: number;
		previewWidth?: number;
		stickMode?: string;
		lStickEnabled?: boolean;
	} = $props();

	// ── Reactive state ──────────────────────────────────────────────────────────
	let cameras = $state<CameraInfo[]>([]);
	let selectedDevice = $state(0);
	let cameraOpen = $state(false);
	let trackState = $state<VideoTrackState>('new');
	let useWebRTC = $state(false);
	let webrtcStream = $state<MediaStream | null>(null);
	let mjpegUrl = $state<string | null>(null);
	let error = $state<string | null>(null);

	// Canvas overlay state
	let canvasEl = $state<HTMLCanvasElement | null>(null);
	let dragActive = $state(false);
	let dragStartX = $state(0);
	let dragStartY = $state(0);
	let dragCurX = $state(0);
	let dragCurY = $state(0);
	let dragAction: 'none' | 'stick' | 'colorpicker' | 'rangescreenshot' | 'namedsave' = $state('none');
	let colorPickResult = $state<string | null>(null);
	let statusMessage = $state<string | null>(null);

	// Offscreen canvas for color picking from video frames
	let offscreenCanvas: HTMLCanvasElement | undefined = $state();

	// ── Client instance ─────────────────────────────────────────────────────────
	let videoClient: WebRTCVideoClient | null = $state(null);

	// Element refs
	let videoEl: HTMLVideoElement | undefined = $state();
	let containerEl: HTMLDivElement | undefined = $state();

	// ── Lifecycle ───────────────────────────────────────────────────────────────
	onMount(() => {
		// Create offscreen canvas for color picking
		offscreenCanvas = document.createElement('canvas');

		// Load camera list
		api.getCameras()
			.then((list) => {
				cameras = list;
				if (list.length > 0) selectedDevice = list[0].device_index;
			})
			.catch((e) => {
				console.warn('Failed to list cameras:', e);
			});

		// Check if camera is already open
		api.getCameraStatus()
			.then((status) => {
				if (status.opened) {
					cameraOpen = true;
					connectVideo();
				}
			})
			.catch(() => {});
	});

	onDestroy(() => {
		disconnectVideo();
	});

	// ── Video client management ────────────────────────────────────────────────
	function connectVideo() {
		if (videoClient) {
			videoClient.disconnect();
		}

		error = null;
		webrtcStream = null;
		mjpegUrl = null;
		useWebRTC = false;

		videoClient = new WebRTCVideoClient(wsClient, {
			mjpegUrl: '/camera/stream',
		});

		videoClient.on('state', (state: VideoTrackState) => {
			trackState = state;
		});

		videoClient.on('stream', (stream: MediaStream) => {
			webrtcStream = stream;
			useWebRTC = true;
			mjpegUrl = null;
			// Attach to video element after next tick
			requestAnimationFrame(() => {
				if (videoEl) {
					videoEl.srcObject = stream;
					videoEl.play().catch((e) => {
						console.warn('[CameraPreview] Video play error:', e);
					});
				}
			});
		});

		videoClient.on('fallback', (url: string) => {
			mjpegUrl = url;
			useWebRTC = false;
			webrtcStream = null;
		});

		videoClient.on('error', (msg: string) => {
			error = msg;
		});

		videoClient.connect();
	}

	function disconnectVideo() {
		if (videoClient) {
			videoClient.disconnect();
			videoClient = null;
		}
		if (videoEl) {
			if (videoEl.srcObject instanceof MediaStream) {
				videoEl.srcObject.getTracks().forEach((t) => t.stop());
			}
			videoEl.srcObject = null;
		}
		webrtcStream = null;
		mjpegUrl = null;
		trackState = 'new';
	}

	// ── Camera controls ─────────────────────────────────────────────────────────
	async function toggleCamera() {
		if (cameraOpen) {
			disconnectVideo();
			try {
				await api.closeCamera();
			} catch (e) {
				console.warn('Failed to close camera:', e);
			}
			cameraOpen = false;
			dispatchCameraState(false);
		} else {
			try {
				await api.openCamera({
					device_index: selectedDevice,
					width: previewWidth,
					height,
				});
				cameraOpen = true;
				connectVideo();
				dispatchCameraState(true);
			} catch (e) {
				error = `Failed to open camera: ${e instanceof Error ? e.message : String(e)}`;
				cameraOpen = false;
			}
		}
	}

	function dispatchCameraState(opened: boolean) {
		window.dispatchEvent(new CustomEvent('camera-state', { detail: { opened } }));
	}

	// ── Mouse operations ────────────────────────────────────────────────────────

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

	function showStatus(msg: string, durationMs = 3000) {
		statusMessage = msg;
		setTimeout(() => { statusMessage = null; }, durationMs);
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
			if (!lStickEnabled) return;
			dragAction = 'stick';
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
			// Dispatch stick event for parent to track
			window.dispatchEvent(new CustomEvent('camera-stick', {
				detail: { stick: stickMode, x: stickX, y: stickY },
			}));
		}

		drawOverlay();
	}

	function handleCanvasMouseUp() {
		if (!dragActive) return;

		if (dragAction === 'rangescreenshot' || dragAction === 'namedsave') {
			const w = Math.abs(dragCurX - dragStartX);
			const h = Math.abs(dragCurY - dragStartY);
			if (w > 10 && h > 10) {
				const format = 'png';
				if (dragAction === 'namedsave') {
					const name = prompt('Save capture as:', 'capture');
					if (name) {
						const filename = `${name}.${format}`;
						api.captureCamera(filename).catch(console.warn);
						showStatus(`Named capture: ${filename}`);
						window.dispatchEvent(new CustomEvent('camera-capture', {
							detail: { filename, type: 'named' },
						}));
					}
				} else {
					const filename = `capture_region_${Date.now()}.${format}`;
					api.captureCamera(filename).catch(console.warn);
					showStatus(`Region captured: ${filename}`);
					window.dispatchEvent(new CustomEvent('camera-capture', {
						detail: { filename, type: 'region' },
					}));
				}
			}
		}

		// Reset stick to center on release
		if (dragAction === 'stick') {
			api.sendInput('stick', { stick: stickMode, x: 128, y: 128 }).catch(() => {});
			window.dispatchEvent(new CustomEvent('camera-stick', {
				detail: { stick: stickMode, x: 128, y: 128, released: true },
			}));
		}

		resetDrag();
		drawOverlay();
	}

	// ── Color picker ────────────────────────────────────────────────────────────
	function pickColor(x: number, y: number) {
		if (!canvasEl || !offscreenCanvas) return;

		// Try to read pixel from video element via offscreen canvas
		const videoSource = videoEl;
		if (videoSource && videoSource.readyState >= 2) {
			offscreenCanvas.width = videoSource.videoWidth || previewWidth;
			offscreenCanvas.height = videoSource.videoHeight || height;
			const offCtx = offscreenCanvas.getContext('2d');
			if (offCtx) {
				try {
					// Scale coordinates to video dimensions
					const scaleX = offscreenCanvas.width / canvasEl.width;
					const scaleY = offscreenCanvas.height / canvasEl.height;
					const srcX = Math.round(x * scaleX);
					const srcY = Math.round(y * scaleY);

					offCtx.drawImage(videoSource, 0, 0);
					const pixel = offCtx.getImageData(srcX, srcY, 1, 1).data;
					const hex = `#${pixel[0].toString(16).padStart(2, '0')}${pixel[1].toString(16).padStart(2, '0')}${pixel[2].toString(16).padStart(2, '0')}`;
					const rgb = `rgb(${pixel[0]}, ${pixel[1]}, ${pixel[2]})`;
					colorPickResult = `${hex} / ${rgb}`;
					showStatus(`Color at (${Math.round(x)}, ${Math.round(y)}): ${hex}`, 5000);
					window.dispatchEvent(new CustomEvent('camera-colorpick', {
						detail: { hex, rgb, x: Math.round(x), y: Math.round(y) },
					}));
					return;
				} catch {
					// CORS or tainted canvas — fall through to API fallback
				}
			}
		}

		// Fallback: use API to get frame and read pixel server-side
		api.getCameraFrame()
			.then(({ frame }) => {
				// frame is a base64 JPEG — show as status but can't read pixel without decoding
				colorPickResult = `Frame returned (use server-side picker)`;
				showStatus(`Color picker: got frame at (${Math.round(x)}, ${Math.round(y)})`, 5000);
				window.dispatchEvent(new CustomEvent('camera-colorpick', {
					detail: { hex: null, rgb: null, x: Math.round(x), y: Math.round(y), frame },
				}));
			})
			.catch((err) => {
				colorPickResult = `Color picker unavailable: ${err.message}`;
				showStatus('Color picker failed', 5000);
			});
	}

	// ── Canvas overlay drawing ──────────────────────────────────────────────────
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
</script>

<div
	bind:this={containerEl}
	class="relative rounded border border-gray-700 bg-gray-900 p-3"
>
	<h3 class="mb-2 text-sm font-medium text-gray-200">カメラプレビュー</h3>

	<!-- Camera selector and toggle -->
	<div class="mb-2 flex items-center gap-2">
		<select
			bind:value={selectedDevice}
			disabled={cameraOpen}
			class="flex-1 rounded bg-gray-800 px-2 py-1 text-xs text-gray-200 disabled:opacity-50"
		>
			{#each cameras as cam (cam.device_index)}
				<option value={cam.device_index}>{cam.name}</option>
			{/each}
		</select>
		<button
			onclick={toggleCamera}
			disabled={cameras.length === 0}
			class="rounded px-3 py-1 text-xs font-medium text-white disabled:opacity-50 {cameraOpen
				? 'bg-red-600 hover:bg-red-500'
				: 'bg-blue-600 hover:bg-blue-500'}"
		>
			{cameraOpen ? '閉じる' : '開く'}
		</button>
	</div>

	<!-- Connection status indicator -->
	{#if cameraOpen}
		<div class="mb-1 flex items-center gap-1.5 text-xs">
			<span
				class="inline-block h-2 w-2 rounded-full {trackState === 'connected'
					? 'bg-green-500'
					: trackState === 'connecting'
						? 'bg-yellow-500'
						: 'bg-red-500'}"
			></span>
			<span class="text-gray-400">
				{#if trackState === 'connected'}
					{useWebRTC ? 'WebRTC' : 'MJPEG'}
				{:else if trackState === 'connecting'}
					接続中...
				{:else if trackState === 'failed'}
					接続失敗
				{:else}
					{trackState}
				{/if}
			</span>
			{#if useWebRTC}
				<span class="ml-1 rounded bg-green-800 px-1 py-0.5 text-[10px] text-green-300">WebRTC</span>
			{:else if mjpegUrl}
				<span class="ml-1 rounded bg-yellow-800 px-1 py-0.5 text-[10px] text-yellow-300">MJPEG</span>
			{/if}
		</div>
	{/if}

	<!-- Status message bar -->
	{#if statusMessage}
		<div class="mb-1 text-[10px] text-gray-500">{statusMessage}</div>
	{/if}

	<!-- Preview area with canvas overlay -->
	<div
		class="relative flex aspect-video items-center justify-center overflow-hidden rounded bg-gray-800"
	>
		{#if error}
			<div class="p-4 text-center text-xs text-red-400">
				<p>{error}</p>
				<button
					onclick={() => {
						error = null;
						if (cameraOpen) connectVideo();
					}}
					class="mt-2 rounded bg-gray-700 px-3 py-1 text-gray-300 hover:bg-gray-600"
				>
					再試行
				</button>
			</div>
		{:else if webrtcStream && useWebRTC}
			<!-- WebRTC path: use native <video> with MediaStream -->
			<video
				bind:this={videoEl}
				autoplay
				muted
				playsinline
				class="max-h-full max-w-full rounded"
			/>
		{:else if mjpegUrl && !useWebRTC}
			<!-- MJPEG fallback path: use <img> with stream URL -->
			<img
				src={mjpegUrl}
				alt="Camera preview"
				class="max-h-full max-w-full rounded"
				onerror={() => {
					error = 'MJPEG stream connection failed';
				}}
			/>
		{:else if cameraOpen && trackState === 'connecting'}
			<div class="flex items-center gap-2 text-xs text-gray-500">
				<span class="inline-block h-3 w-3 animate-spin rounded-full border-2 border-gray-500 border-t-transparent"></span>
				接続中...
			</div>
		{:else}
			<div class="flex flex-col items-center justify-center text-gray-500">
				<svg
					class="mb-2 h-12 w-12 opacity-50"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="1.5"
				>
					<path d="M15.75 10.5l4.72-4.72a.75.75 0 011.28.53v11.38a.75.75 0 01-1.28.53l-4.72-4.72M4.5 18.75h9a2.25 2.25 0 002.25-2.25v-9a2.25 2.25 0 00-2.25-2.25h-9A2.25 2.25 0 002.25 7.5v9a2.25 2.25 0 002.25 2.25z" />
					<path d="M9.75 9.75l4.5 4.5M14.25 9.75l-4.5 4.5" stroke-linecap="round" />
				</svg>
				<span class="text-xs">{cameraOpen ? '準備中...' : 'No image'}</span>
			</div>
		{/if}

		<!-- Transparent Canvas overlay for mouse operations -->
		<canvas
			bind:this={canvasEl}
			width={previewWidth}
			height={height}
			onmousedown={handleCanvasMouseDown}
			onmousemove={handleCanvasMouseMove}
			onmouseup={handleCanvasMouseUp}
			onmouseleave={handleCanvasMouseUp}
			class="pointer-events-none absolute inset-0"
			class:pointer-events-auto={cameraOpen}
			role="img"
			aria-label="Camera mouse operations"
		/>
	</div>

	<!-- Color picker result -->
	{#if colorPickResult}
		<div class="mt-1 text-[10px] text-gray-500">
			Color: <span class="font-mono text-gray-300">{colorPickResult}</span>
		</div>
	{/if}

	<!-- Mouse operation hint -->
	<div class="mt-1 text-[9px] text-gray-600">
		Drag=Stick | Ctrl+Click=Color | Ctrl+Shift+Drag=Region SS | Ctrl+Alt+Drag=Named Save
	</div>
</div>

<style>
	video::-webkit-media-controls {
		display: none !important;
	}

	canvas.pointer-events-none {
		pointer-events: none;
	}

	canvas.pointer-events-auto {
		pointer-events: auto;
	}
</style>
