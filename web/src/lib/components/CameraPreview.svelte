<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { wsClient } from '$lib/api/websocket';
	import { WebRTCVideoClient, type VideoTrackState } from '$lib/api/webrtc-video';
	import { api } from '$lib/api/client';

	interface CameraInfo {
		device_index: number;
		name: string;
	}

	let { height = 360 }: { height?: number } = $props();

	// ── Reactive state ─────────────────────────────────────────────────
	let cameras = $state<CameraInfo[]>([]);
	let selectedDevice = $state(0);
	let cameraOpen = $state(false);
	let trackState = $state<VideoTrackState>('new');
	let useWebRTC = $state(false);
	let webrtcStream = $state<MediaStream | null>(null);
	let mjpegUrl = $state<string | null>(null);
	let error = $state<string | null>(null);

	// ── Client instance ───────────────────────────────────────────────
	let videoClient: WebRTCVideoClient | null = $state(null);

	// Element refs
	let videoEl: HTMLVideoElement | undefined = $state();

	onMount(() => {
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

	// ── Video client management ───────────────────────────────────────

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
			// Stop all media tracks to release camera resources
			if (videoEl.srcObject instanceof MediaStream) {
				videoEl.srcObject.getTracks().forEach((t) => t.stop());
			}
			videoEl.srcObject = null;
		}
		webrtcStream = null;
		mjpegUrl = null;
		trackState = 'new';
	}

	// ── Camera controls ───────────────────────────────────────────────

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
					width: 640,
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
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
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

	<!-- Preview area -->
	<div class="flex aspect-video items-center justify-center overflow-hidden rounded bg-gray-800">
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
	</div>
</div>

<style>
	video::-webkit-media-controls {
		display: none !important;
	}
</style>
