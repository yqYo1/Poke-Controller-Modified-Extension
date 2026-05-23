<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type CameraDevice } from '$lib/api/client';

	// eslint-disable-next-line @typescript-eslint/no-unused-vars
	interface CameraSettings {
		width: number;
		height: number;
		fps: number;
		flip: string;
	}

	let {
		deviceIndex = 0,
		onDeviceChange = (_index: number) => {}, // eslint-disable-line @typescript-eslint/no-unused-vars
		disabled = false,
	}: {
		deviceIndex?: number;
		onDeviceChange?: (index: number) => void;
		disabled?: boolean;
	} = $props();

	// ── Reactive state ─────────────────────────────────────────────────
	let cameras = $state<CameraDevice[]>([]);
	let selectedDevice = $state(deviceIndex);

	// Camera config settings
	let fps = $state(30);
	let flipH = $state(false);
	let flipV = $state(false);
	let resolution = $state<'640x480' | '800x600' | '1280x720' | '1920x1080'>('1280x720');
	let width = $state(1280);
	let height = $state(720);

	let saving = $state(false);
	let error = $state<string | null>(null);
	let success = $state<string | null>(null);

	// ── Resolution presets ─────────────────────────────────────────────
	const resolutionPresets: Record<string, { w: number; h: number }> = {
		'640x480': { w: 640, h: 480 },
		'800x600': { w: 800, h: 600 },
		'1280x720': { w: 1280, h: 720 },
		'1920x1080': { w: 1920, h: 1080 },
	};

	// ── Derived flip string ────────────────────────────────────────────
	const flipString = $derived.by(() => {
		if (flipH && flipV) return 'both';
		if (flipH) return 'horizontal';
		if (flipV) return 'vertical';
		return 'none';
	});

	// ── Lifecycle ──────────────────────────────────────────────────────
	onMount(() => {
		loadCameras();
		loadCurrentSettings();
	});

	// ── Methods ────────────────────────────────────────────────────────

	async function loadCameras() {
		try {
			cameras = await api.getCameras();
		} catch (e) {
			console.warn('Failed to list cameras:', e);
		}
	}

	async function loadCurrentSettings() {
		try {
			const status = await api.getCameraStatus();
			if (status.opened) {
				// Try to get current config — we don't have a GET config endpoint,
				// but we can infer from open request params if available
			}
		} catch (e) {
			console.warn('Failed to load camera status:', e);
		}
	}

	function handleDeviceChange(e: Event) {
		const target = e.target as HTMLSelectElement;
		selectedDevice = Number(target.value);
		onDeviceChange(selectedDevice);
	}

	function handleResolutionChange(e: Event) {
		const target = e.target as HTMLSelectElement;
		resolution = target.value as typeof resolution;
		const preset = resolutionPresets[resolution];
		if (preset) {
			width = preset.w;
			height = preset.h;
		}
	}

	async function saveSettings() {
		saving = true;
		error = null;
		success = null;

		try {
			await api.updateCameraConfig({
				width,
				height,
				fps,
				flip: flipString,
			});
			success = '設定を保存しました';
		} catch (e) {
			error = `保存に失敗しました: ${e instanceof Error ? e.message : String(e)}`;
		} finally {
			saving = false;
			// Clear success message after 3 seconds
			setTimeout(() => {
				success = null;
			}, 3000);
		}
	}

	// eslint-disable-next-line @typescript-eslint/no-unused-vars
	function flipFromString(val: string) {
		switch (val) {
			case 'horizontal':
				flipH = true;
				flipV = false;
				break;
			case 'vertical':
				flipH = false;
				flipV = true;
				break;
			case 'both':
				flipH = true;
				flipV = true;
				break;
			default:
				flipH = false;
				flipV = false;
				break;
		}
	}

	// Sync external deviceIndex changes
	$effect(() => {
		if (deviceIndex !== selectedDevice && cameras.some((c) => c.device_index === deviceIndex)) {
			selectedDevice = deviceIndex;
		}
	});
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<h3 class="mb-3 text-sm font-medium text-gray-200">カメラ設定</h3>

	<!-- Device selection -->
	<div class="mb-3">
		<label class="mb-1 block text-xs text-gray-400">デバイス</label>
		<select
			value={selectedDevice}
			onchange={handleDeviceChange}
			disabled={disabled || cameras.length === 0}
			class="w-full rounded bg-gray-800 px-2 py-1.5 text-xs text-gray-200 disabled:opacity-50"
		>
			{#each cameras as cam (cam.device_index)}
				<option value={cam.device_index}>{cam.name}</option>
			{/each}
		</select>
	</div>

	<!-- Resolution -->
	<div class="mb-3">
		<label class="mb-1 block text-xs text-gray-400">解像度</label>
		<select
			value={resolution}
			onchange={handleResolutionChange}
			disabled={disabled}
			class="w-full rounded bg-gray-800 px-2 py-1.5 text-xs text-gray-200 disabled:opacity-50"
		>
			{#each Object.keys(resolutionPresets) as res (res)}
				<option value={res}>{res}</option>
			{/each}
		</select>
	</div>

	<!-- FPS -->
	<div class="mb-3">
		<span class="mb-1 block text-xs text-gray-400">FPS</span>
		<select
			bind:value={fps}
			disabled={disabled}
			class="w-full rounded bg-gray-800 px-2 py-1.5 text-xs text-gray-200 disabled:opacity-50"
		>
			<option value={5}>5</option>
			<option value={10}>10</option>
			<option value={15}>15</option>
			<option value={24}>24</option>
			<option value={30}>30</option>
			<option value={60}>60</option>
		</select>
	</div>

	<!-- Flip controls -->
	<div class="mb-3 space-y-2">
		<span class="mb-1 block text-xs text-gray-400">反転</span>
		<label class="flex cursor-pointer items-center gap-2">
			<input
				type="checkbox"
				bind:checked={flipH}
				disabled={disabled}
				class="accent-blue-500"
			/>
			<span class="text-xs text-gray-300">水平反転</span>
		</label>
		<label class="flex cursor-pointer items-center gap-2">
			<input
				type="checkbox"
				bind:checked={flipV}
				disabled={disabled}
				class="accent-blue-500"
			/>
			<span class="text-xs text-gray-300">垂直反転</span>
		</label>
	</div>

	<!-- Save button -->
	<div class="flex items-center gap-2">
		<button
			onclick={saveSettings}
			disabled={disabled || saving}
			class="rounded bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500 disabled:opacity-50"
		>
			{saving ? '保存中...' : '設定を保存'}
		</button>

		{#if success}
			<span class="text-xs text-green-400">{success}</span>
		{/if}

		{#if error}
			<span class="text-xs text-red-400">{error}</span>
		{/if}
	</div>
</div>
