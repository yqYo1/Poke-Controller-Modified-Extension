<script lang="ts">
	import { onMount } from 'svelte';
	import { api, type MouseStickConfig } from '$lib/api/client';

	let keyboardEnabled = $state(false);
	let mouseStickLEnabled = $state(false);
	let mouseStickREnabled = $state(false);
	let mouseSensitivity = $state(1.0);
	let mouseDeadzone = $state(0.1);

	// ── Key mapping display ───────────────────────────────────────────────
	const keyMappings = [
		{ key: 'W / ↑', action: 'D-Pad Up / L-Stick Up' },
		{ key: 'S / ↓', action: 'D-Pad Down / L-Stick Down' },
		{ key: 'A / ←', action: 'D-Pad Left / L-Stick Left' },
		{ key: 'D / →', action: 'D-Pad Right / L-Stick Right' },
		{ key: 'Z', action: 'L Button' },
		{ key: 'X', action: 'R Button' },
		{ key: 'Q', action: 'ZL Button' },
		{ key: 'E', action: 'ZR Button' },
		{ key: 'J', action: 'B Button' },
		{ key: 'K', action: 'A Button' },
		{ key: 'U', action: 'X Button' },
		{ key: 'I', action: 'Y Button' },
		{ key: 'Enter', action: 'Start (+) Button' },
		{ key: 'Backspace', action: 'Select (-) Button' },
		{ key: 'Space', action: 'Home Button' },
		{ key: 'C', action: 'Capture Button' },
	];

	onMount(() => {
		// Load current keyboard state
		api.getKeyboardEnabled()
			.then((r) => { keyboardEnabled = r.enabled; })
			.catch(console.warn);

		// Load current mouse stick config
		api.getMouseStick()
			.then((config: MouseStickConfig) => {
				mouseStickLEnabled = config.left_enabled;
				mouseStickREnabled = config.right_enabled;
				mouseSensitivity = config.sensitivity;
			})
			.catch(console.warn);
	});

	async function toggleKeyboard() {
		const enabled = !keyboardEnabled;
		try {
			if (enabled) {
				await api.enableKeyboard();
			} else {
				await api.disableKeyboard();
			}
			keyboardEnabled = enabled;
		} catch (e) {
			console.warn('Failed to toggle keyboard:', e);
		}
	}

	async function applyMouseStick(stick: 'left' | 'right', enabled: boolean) {
		try {
			await api.setMouseStick({
				stick,
				enabled,
				sensitivity: mouseSensitivity,
			});
		} catch (e) {
			console.warn('Failed to set mouse stick:', e);
		}
	}

	async function handleSensitivityChange() {
		try {
			await api.setMouseStick({
				stick: 'left',
				enabled: mouseStickLEnabled,
				sensitivity: mouseSensitivity,
			});
			if (mouseStickREnabled) {
				await api.setMouseStick({
					stick: 'right',
					enabled: mouseStickREnabled,
					sensitivity: mouseSensitivity,
				});
			}
		} catch (e) {
			console.warn('Failed to update sensitivity:', e);
		}
	}
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-4">
	<h3 class="mb-3 text-sm font-medium text-gray-200">ソフトウェアコントロール設定</h3>

	<!-- ── Keyboard enable/disable toggle ───────────────────────────────── -->
	<div class="mb-3 flex items-center justify-between rounded bg-gray-800 px-3 py-2">
		<div>
			<span class="text-xs font-medium text-gray-200">キーボード入力</span>
			<p class="text-[10px] text-gray-500">キーボードを使用したコントローラー入力を有効にします</p>
		</div>
		<label class="relative inline-flex cursor-pointer items-center">
			<input
				type="checkbox"
				checked={keyboardEnabled}
				onchange={toggleKeyboard}
				class="peer sr-only"
			/>
			<div class="h-5 w-9 rounded-full bg-gray-600 after:absolute after:left-[2px] after:top-[2px] after:h-4 after:w-4 after:rounded-full after:bg-white after:transition-all peer-checked:bg-blue-600 peer-checked:after:translate-x-full"></div>
		</label>
	</div>

	<!-- ── Key mapping display ──────────────────────────────────────────── -->
	<div class="mb-3">
		<details class="group">
			<summary class="cursor-pointer text-xs font-medium text-gray-400 hover:text-gray-200">
				<span class="group-open:after:content-['▼'] after:content-['▶'] after:ml-1"></span>
				キーマッピング一覧
			</summary>
			<div class="mt-2 max-h-48 overflow-y-auto rounded bg-gray-800 p-2">
				<table class="w-full text-left text-xs">
					<thead>
						<tr class="border-b border-gray-700 text-gray-400">
							<th class="py-1 pr-3">キー</th>
							<th class="py-1">アクション</th>
						</tr>
					</thead>
					<tbody>
						{#each keyMappings as mapping (mapping.key)}
							<tr class="border-b border-gray-800">
								<td class="py-1 pr-3 font-mono font-bold text-blue-400">{mapping.key}</td>
								<td class="py-1 text-gray-300">{mapping.action}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</details>
	</div>

	<!-- ── Mouse control settings ───────────────────────────────────────── -->
	<div class="rounded border border-gray-700 p-3">
		<h4 class="mb-2 text-xs font-medium text-gray-300">マウススティック制御</h4>

		<div class="space-y-2">
			<label class="flex items-center gap-2 text-xs text-gray-400">
				<input
					type="checkbox"
					checked={mouseStickLEnabled}
					onchange={() => {
						mouseStickLEnabled = !mouseStickLEnabled;
						applyMouseStick('left', mouseStickLEnabled);
					}}
					class="accent-blue-500"
				/>
				左スティックをマウスで制御
			</label>

			<label class="flex items-center gap-2 text-xs text-gray-400">
				<input
					type="checkbox"
					checked={mouseStickREnabled}
					onchange={() => {
						mouseStickREnabled = !mouseStickREnabled;
						applyMouseStick('right', mouseStickREnabled);
					}}
					class="accent-blue-500"
				/>
				右スティックをマウスで制御
			</label>

			<div class="flex items-center justify-between text-xs text-gray-400">
				<span>感度</span>
				<div class="flex items-center gap-2">
					<input
						type="range"
						min="0.1"
						max="3.0"
						step="0.1"
						value={mouseSensitivity}
						oninput={(e) => {
							mouseSensitivity = parseFloat((e.target as HTMLInputElement).value);
						}}
						onchange={handleSensitivityChange}
						class="w-24"
					/>
					<span class="w-8 text-right text-gray-300">{mouseSensitivity.toFixed(1)}</span>
				</div>
			</div>

			<div class="flex items-center justify-between text-xs text-gray-400">
				<span>デッドゾーン</span>
				<div class="flex items-center gap-2">
					<input
						type="range"
						min="0.0"
						max="0.5"
						step="0.05"
						bind:value={mouseDeadzone}
						class="w-24"
					/>
					<span class="w-8 text-right text-gray-300">{mouseDeadzone.toFixed(2)}</span>
				</div>
			</div>

			<div class="mt-2 rounded bg-gray-800 px-2 py-1.5 text-[10px] text-gray-500">
				マウス制御を有効にすると、マウス移動がスティック入力として送信されます。
				感度が高いほど、マウスの小さな動きで大きくスティックが傾きます。
			</div>
		</div>
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
