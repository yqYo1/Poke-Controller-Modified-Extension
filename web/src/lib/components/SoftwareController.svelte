<script lang="ts">
	import { api } from '$lib/api/client';

	let controllerType = $state('Pro Controller');
	let keyboardEnabled = $state(false);

	let pressedButtons = $state<Set<string>>(new Set());

	const faceButtons = [
		{ id: 'a', label: 'A', row: 0, col: 3 },
		{ id: 'b', label: 'B', row: 1, col: 2 },
		{ id: 'x', label: 'X', row: 0, col: 2 },
		{ id: 'y', label: 'Y', row: 1, col: 3 },
	];

	const dpadButtons = [
		{ id: 'up', label: '↑', row: 0, col: 1 },
		{ id: 'down', label: '↓', row: 2, col: 1 },
		{ id: 'left', label: '←', row: 1, col: 0 },
		{ id: 'right', label: '→', row: 1, col: 2 },
	];

	const shoulderButtons = [
		{ id: 'l', label: 'L', row: 0, col: 0 },
		{ id: 'r', label: 'R', row: 0, col: 4 },
		{ id: 'zl', label: 'ZL', row: 0, col: 1 },
		{ id: 'zr', label: 'ZR', row: 0, col: 3 },
	];

	const systemButtons = [
		{ id: 'minus', label: '-', row: 0, col: 0 },
		{ id: 'plus', label: '+', row: 0, col: 2 },
		{ id: 'home', label: 'HOME', row: 0, col: 1 },
		{ id: 'capture', label: '撮影', row: 0, col: 0 },
	];

	const leftStick = { id: 'left_stick', label: 'Lスティック' };
	const rightStick = { id: 'right_stick', label: 'Rスティック' };

	function sendButtonInput(button: string, pressed: boolean) {
		if (pressed) {
			pressedButtons.add(button);
		} else {
			pressedButtons.delete(button);
		}
		pressedButtons = new Set(pressedButtons);
		api.sendInput('button', { button, pressed }).catch(console.warn);
	}

	function sendStickInput(stick: string, direction: string) {
		api.sendInput('stick', { stick, direction }).catch(console.warn);
	}
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-4">
	<div class="mb-3 flex items-center justify-between">
		<h3 class="text-sm font-medium text-gray-200">ソフトウェアコントローラー</h3>
		<div class="flex items-center gap-2">
			<span class="text-xs text-gray-400">タイプ:</span>
			<select
				bind:value={controllerType}
				onchange={() => api.setControllerType(controllerType).catch(console.warn)}
				class="rounded bg-gray-800 px-2 py-1 text-xs text-gray-200"
			>
				<option>Pro Controller</option>
				<option>Joy-Con L</option>
				<option>Joy-Con R</option>
			</select>
		</div>
	</div>

	<div class="mx-auto max-w-md">
		<div class="grid grid-cols-5 gap-1">
			<!-- ショルダーボタン -->
			{#each shoulderButtons as btn}
				<button
					onmousedown={() => sendButtonInput(btn.id, true)}
					onmouseup={() => sendButtonInput(btn.id, false)}
					onmouseleave={() => sendButtonInput(btn.id, false)}
					style="grid-row: {btn.row + 1}; grid-column: {btn.col + 1}"
					class="rounded bg-gray-700 px-2 py-1 text-xs font-bold text-gray-200 transition-colors hover:bg-gray-600 active:bg-blue-600"
				>
					{btn.label}
				</button>
			{/each}

			<!-- 左スティック -->
			<div style="grid-row: 2; grid-column: 1 / 3" class="flex items-center justify-center rounded bg-gray-800 p-2">
				<div class="grid grid-cols-3 gap-0.5">
					<button onclick={() => sendStickInput(leftStick.id, 'up')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">↑</button>
					<button onclick={() => sendStickInput(leftStick.id, 'left')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">←</button>
					<button onclick={() => sendStickInput(leftStick.id, 'neutral')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">○</button>
					<button onclick={() => sendStickInput(leftStick.id, 'right')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">→</button>
					<button onclick={() => sendStickInput(leftStick.id, 'down')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">↓</button>
				</div>
				<span class="ml-1 text-[10px] text-gray-500">L</span>
			</div>

			<!-- D-Pad -->
			<div style="grid-row: 2; grid-column: 3" class="grid grid-cols-3 gap-0.5 place-content-center rounded bg-gray-800 p-1">
				{#each dpadButtons as btn}
					<button
						onmousedown={() => sendButtonInput(btn.id, true)}
						onmouseup={() => sendButtonInput(btn.id, false)}
						onmouseleave={() => sendButtonInput(btn.id, false)}
						style="grid-row: {btn.row + 1}; grid-column: {btn.col + 1}"
						class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-200 hover:bg-gray-600 active:bg-blue-600"
					>
						{btn.label}
					</button>
				{/each}
			</div>

			<!-- システムボタン -->
			<div style="grid-row: 3; grid-column: 1 / 3" class="flex items-center justify-center gap-1">
				<button
					onmousedown={() => sendButtonInput('minus', true)}
					onmouseup={() => sendButtonInput('minus', false)}
					class="rounded bg-gray-700 px-2 py-1 text-xs text-gray-200 hover:bg-gray-600 active:bg-blue-600"
				>-</button>
				<button
					onmousedown={() => sendButtonInput('capture', true)}
					onmouseup={() => sendButtonInput('capture', false)}
					class="rounded bg-gray-700 px-2 py-1 text-xs text-gray-200 hover:bg-gray-600 active:bg-blue-600"
				>撮影</button>
				<button
					onmousedown={() => sendButtonInput('home', true)}
					onmouseup={() => sendButtonInput('home', false)}
					class="rounded bg-gray-700 px-2 py-1 text-xs text-gray-200 hover:bg-gray-600 active:bg-blue-600"
				>HOME</button>
				<button
					onmousedown={() => sendButtonInput('plus', true)}
					onmouseup={() => sendButtonInput('plus', false)}
					class="rounded bg-gray-700 px-2 py-1 text-xs text-gray-200 hover:bg-gray-600 active:bg-blue-600"
				>+</button>
			</div>

			<!-- 右スティック -->
			<div style="grid-row: 2; grid-column: 4 / 6" class="flex items-center justify-center rounded bg-gray-800 p-2">
				<span class="mr-1 text-[10px] text-gray-500">R</span>
				<div class="grid grid-cols-3 gap-0.5">
					<button onclick={() => sendStickInput(rightStick.id, 'up')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">↑</button>
					<button onclick={() => sendStickInput(rightStick.id, 'left')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">←</button>
					<button onclick={() => sendStickInput(rightStick.id, 'neutral')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">○</button>
					<button onclick={() => sendStickInput(rightStick.id, 'right')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">→</button>
					<button onclick={() => sendStickInput(rightStick.id, 'down')} class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-300 hover:bg-gray-600">↓</button>
				</div>
			</div>

			<!-- フェイスボタン A/B/X/Y -->
			<div style="grid-row: 3; grid-column: 4 / 6" class="grid grid-cols-2 gap-1 place-content-center rounded bg-gray-800 p-2">
				{#each faceButtons as btn}
					<button
						onmousedown={() => sendButtonInput(btn.id, true)}
						onmouseup={() => sendButtonInput(btn.id, false)}
						onmouseleave={() => sendButtonInput(btn.id, false)}
						class="rounded px-3 py-1.5 text-sm font-bold text-white transition-colors hover:brightness-110 active:brightness-125 {btn.id === 'a' ? 'bg-green-600' : btn.id === 'b' ? 'bg-red-600' : btn.id === 'x' ? 'bg-blue-600' : 'bg-yellow-600'}"
					>
						{btn.label}
					</button>
				{/each}
			</div>
		</div>
	</div>

	<div class="mt-3 flex items-center gap-2">
		<label class="flex items-center gap-1 text-xs text-gray-400">
			<input type="checkbox" bind:checked={keyboardEnabled} onchange={() => api.setKeyboardEnabled(keyboardEnabled).catch(console.warn)} class="accent-blue-500" />
			キーボード入力有効
		</label>
	</div>
</div>
