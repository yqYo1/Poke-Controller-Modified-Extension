<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type GamepadInfo } from '$lib/api/client';

	// ── Controller type radio selection ──────────────────────────────────
	// SPEC §4.3.2: ProController and Xinput as radio buttons
	let controllerType = $state<'ProController' | 'Xinput'>('ProController');

	// ── Connection state ──────────────────────────────────────────────────
	let connected = $state(false);
	let connectionStatus = $state<string>('未接続');
	let isConnecting = $state(false);

	// ── Gamepad list ──────────────────────────────────────────────────────
	let gamepads = $state<GamepadInfo[]>([]);

	// ── Recording ─────────────────────────────────────────────────────────
	let recording = $state(false);

	// ── Vibration ─────────────────────────────────────────────────────────
	let vibrationEnabled = $state(true);

	onMount(() => {
		// Load current controller type
		api.getControllerType()
			.then((r) => {
				const t = r.gamepad_type;
				if (t === 'ProController' || t === 'Xinput') {
					controllerType = t;
				}
			})
			.catch(console.warn);

		// Load gamepad list
		refreshGamepadList();

		// Try to get controller status
		api.getControllerStatus()
			.then((status) => {
				connected = status.connected;
				connectionStatus = status.connected ? '接続済み' : '未接続';
				vibrationEnabled = status.vibration_enabled;
				recording = status.recording;
			})
			.catch(() => {
				// Status endpoint may not exist yet — use gamepad list instead
				connectionStatus = gamepads.some((g) => g.connected) ? '接続済み' : '未接続';
			});
	});

	async function refreshGamepadList() {
		try {
			gamepads = await api.gamepadList();
		} catch {
			// Gamepad list API may not be available; use browser gamepad API as fallback
			const browserGamepads = navigator.getGamepads?.() ?? [];
			gamepads = Array.from(browserGamepads)
				.filter((g): g is Gamepad => g !== null)
				.map((g) => ({
					index: g.index,
					name: g.id,
					connected: g.connected,
				}));
		}
	}

	async function handleConnect() {
		isConnecting = true;
		try {
			if (connected) {
				await api.gamepadDisconnect();
				connected = false;
				connectionStatus = '未接続';
			} else {
				await api.gamepadConnect(controllerType);
				connected = true;
				connectionStatus = `${controllerType} 接続済み`;
				await refreshGamepadList();
			}
		} catch (e) {
			console.warn('Gamepad connection error:', e);
			connectionStatus = `エラー: ${e instanceof Error ? e.message : String(e)}`;
		} finally {
			isConnecting = false;
		}
	}

	async function handleTypeChange(type: 'ProController' | 'Xinput') {
		controllerType = type;
		try {
			await api.setControllerType(type);
			// If already connected, reconnect with new type
			if (connected) {
				await api.gamepadDisconnect();
				connected = false;
				connectionStatus = '未接続';
			}
		} catch (e) {
			console.warn('Failed to set controller type:', e);
		}
	}

	async function toggleRecording() {
		try {
			if (recording) {
				await api.stopRecording();
				recording = false;
			} else {
				await api.startRecording();
				recording = true;
			}
		} catch (e) {
			console.warn('Recording error:', e);
		}
	}

	async function toggleVibration() {
		const newVal = !vibrationEnabled;
		try {
			await api.setVibrationEnabled(newVal);
			vibrationEnabled = newVal;
		} catch (e) {
			console.warn('Vibration toggle error:', e);
		}
	}

	// ── Connection status indicator ──────────────────────────────────────
	let statusColor = $derived(
		connected ? 'bg-green-500' : 'bg-red-500'
	);
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-4">
	<h3 class="mb-3 text-sm font-medium text-gray-200">ハードウェアコントロール</h3>

	<!-- ── Controller type radio buttons (SPEC §4.3.2) ─────────────────── -->
	<div class="mb-3">
		<label class="mb-1 block text-xs text-gray-400">コントローラータイプ</label>
		<div class="flex gap-4 rounded bg-gray-800 px-3 py-2">
			<label class="flex items-center gap-2 text-xs text-gray-300 cursor-pointer">
				<input
					type="radio"
					name="controllerType"
					value="ProController"
					checked={controllerType === 'ProController'}
					onchange={() => handleTypeChange('ProController')}
					class="accent-blue-500"
				/>
				<span>ProController</span>
			</label>
			<label class="flex items-center gap-2 text-xs text-gray-300 cursor-pointer">
				<input
					type="radio"
					name="controllerType"
					value="Xinput"
					checked={controllerType === 'Xinput'}
					onchange={() => handleTypeChange('Xinput')}
					class="accent-blue-500"
				/>
				<span>Xinput</span>
			</label>
		</div>
	</div>

	<!-- ── Connection status ────────────────────────────────────────────── -->
	<div class="mb-3 flex items-center justify-between rounded bg-gray-800 px-3 py-2">
		<div class="flex items-center gap-2">
			<span class="inline-block h-2.5 w-2.5 rounded-full {statusColor}"></span>
			<div>
				<span class="text-xs font-medium text-gray-200">{connectionStatus}</span>
				<p class="text-[10px] text-gray-500">
					{connected ? 'コントローラー接続中' : 'コントローラー未接続'}
				</p>
			</div>
		</div>
		<button
			onclick={handleConnect}
			disabled={isConnecting}
			class="rounded px-3 py-1 text-xs font-medium text-white disabled:opacity-50 {connected
				? 'bg-red-600 hover:bg-red-500'
				: 'bg-blue-600 hover:bg-blue-500'}"
		>
			{isConnecting ? '接続中...' : connected ? '切断' : '接続'}
		</button>
	</div>

	<!-- ── Connected gamepad list ───────────────────────────────────────── -->
	<div class="mb-3">
		<div class="mb-1 flex items-center justify-between">
			<span class="text-xs text-gray-400">接続済みゲームパッド</span>
			<button
				onclick={refreshGamepadList}
				class="rounded bg-gray-700 px-2 py-0.5 text-[10px] text-gray-400 hover:bg-gray-600"
			>
				更新
			</button>
		</div>
		<div class="max-h-24 overflow-y-auto rounded bg-gray-800 p-1">
			{#if gamepads.length === 0}
				<p class="px-2 py-3 text-center text-[10px] text-gray-500">接続済みのゲームパッドはありません</p>
			{:else}
				{#each gamepads as pad (pad.index)}
					<div class="flex items-center justify-between px-2 py-1 text-xs">
						<div class="flex items-center gap-2">
							<span
								class="inline-block h-2 w-2 rounded-full {pad.connected ? 'bg-green-500' : 'bg-gray-600'}"
							></span>
							<span class="text-gray-300">{pad.name}</span>
						</div>
						<span class="text-[10px] text-gray-500">#{pad.index}</span>
					</div>
				{/each}
			{/if}
		</div>
	</div>

	<!-- ── Recording controls (SPEC §4.3.2) ─────────────────────────────── -->
	<div class="mb-3 flex items-center justify-between rounded bg-gray-800 px-3 py-2">
		<div>
			<span class="text-xs font-medium text-gray-200">入力記録</span>
			<p class="text-[10px] text-gray-500">
				{recording ? '入力を記録中...' : 'クリックして入力の記録を開始'}
			</p>
		</div>
		<button
			onclick={toggleRecording}
			class="rounded px-3 py-1 text-xs font-medium {recording
				? 'bg-red-600 text-white hover:bg-red-500'
				: 'border border-gray-600 text-gray-300 hover:bg-gray-700'}"
		>
			{recording ? '■ 停止' : '● 録画'}
		</button>
	</div>

	<!-- ── Vibration toggle ─────────────────────────────────────────────── -->
	<div class="flex items-center justify-between rounded bg-gray-800 px-3 py-2">
		<div>
			<span class="text-xs font-medium text-gray-200">バイブレーション</span>
			<p class="text-[10px] text-gray-500">コントローラーの振動を有効にする</p>
		</div>
		<label class="relative inline-flex cursor-pointer items-center">
			<input
				type="checkbox"
				checked={vibrationEnabled}
				onchange={toggleVibration}
				class="peer sr-only"
			/>
			<div class="h-5 w-9 rounded-full bg-gray-600 after:absolute after:left-[2px] after:top-[2px] after:h-4 after:w-4 after:rounded-full after:bg-white after:transition-all peer-checked:bg-blue-600 peer-checked:after:translate-x-full"></div>
		</label>
	</div>
</div>

<style>
	input[type="checkbox"].sr-only {
		position: absolute;
		width: 1px;
		height: 1px;
		padding: 0;
		margin: -1px;
		overflow: hidden;
		clip: rect(0, 0, 0, 0);
		white-space: nowrap;
		border-width: 0;
	}
</style>
