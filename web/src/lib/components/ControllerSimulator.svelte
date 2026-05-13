<script lang="ts">
	import { api } from '$lib/api/client';
	import { SvelteSet } from 'svelte/reactivity';

	// ── Props ─────────────────────────────────────────────────────────────
	let { visible = true }: { visible?: boolean } = $props();

	// ── State ─────────────────────────────────────────────────────────────
	let pressedButtons = new SvelteSet<string>();

	// Left stick position (normalized -1..1)
	let lx = $state(0);
	let ly = $state(0);
	// Right stick position
	let rx = $state(0);
	let ry = $state(0);

	// ── Button layout definitions ─────────────────────────────────────────
	const shoulderButtons = [
		{ id: 'l', label: 'L', row: 0, col: 0 },
		{ id: 'zl', label: 'ZL', row: 0, col: 1 },
		{ id: 'r', label: 'R', row: 0, col: 4 },
		{ id: 'zr', label: 'ZR', row: 0, col: 3 },
	];

	const dpadButtons = [
		{ id: 'up', label: '↑', row: 0, col: 1 },
		{ id: 'down', label: '↓', row: 2, col: 1 },
		{ id: 'left', label: '←', row: 1, col: 0 },
		{ id: 'right', label: '→', row: 1, col: 2 },
	];

	const faceButtons = [
		{ id: 'a', label: 'A', row: 0, col: 3 },
		{ id: 'b', label: 'B', row: 1, col: 2 },
		{ id: 'x', label: 'X', row: 0, col: 2 },
		{ id: 'y', label: 'Y', row: 1, col: 3 },
	];

	const systemButtons = [
		{ id: 'minus', label: '-', row: 0, col: 0 },
		{ id: 'capture', label: '📷', row: 0, col: 1 },
		{ id: 'home', label: '🏠', row: 0, col: 2 },
		{ id: 'plus', label: '+', row: 0, col: 3 },
	];

	// ── Input handlers ────────────────────────────────────────────────────
	function sendButtonInput(button: string, pressed: boolean) {
		if (pressed) {
			pressedButtons.add(button);
		} else {
			pressedButtons.delete(button);
		}
		api.sendInput('button', { button, pressed }).catch(console.warn);
	}

	function sendStickInput(stick: string, direction: string) {
		api.sendInput('stick', { stick, direction }).catch(console.warn);
	}

	function handleLeftStickClick(dx: number, dy: number) {
		lx = dx;
		ly = dy;
		const direction = resolveStickDirection(dx, dy);
		sendStickInput('left_stick', direction);
	}

	function handleRightStickClick(dx: number, dy: number) {
		rx = dx;
		ry = dy;
		const direction = resolveStickDirection(dx, dy);
		sendStickInput('right_stick', direction);
	}

	function resolveStickDirection(dx: number, dy: number): string {
		if (dx === 0 && dy === 0) return 'neutral';
		if (Math.abs(dx) > Math.abs(dy)) {
			return dx > 0 ? 'right' : 'left';
		}
		return dy > 0 ? 'down' : 'up';
	}

	function resetLeftStick() {
		lx = 0;
		ly = 0;
		sendStickInput('left_stick', 'neutral');
	}

	function resetRightStick() {
		rx = 0;
		ry = 0;
		sendStickInput('right_stick', 'neutral');
	}

	// ── Derived visual helpers ────────────────────────────────────────────
	let isPressed = (id: string) => pressedButtons.has(id);
</script>

{#if visible}
	<div class="rounded border border-gray-700 bg-gray-900 p-3">
		<div class="mb-2 flex items-center justify-between">
			<h3 class="text-sm font-medium text-gray-200">コントローラーシミュレーター</h3>
		</div>

		<div class="mx-auto max-w-xs">
			<!-- Grid-based Joy-Con layout -->
			<div class="grid grid-cols-5 gap-1">
				<!-- Shoulder buttons (row 1) -->
				{#each shoulderButtons as btn (btn.id)}
					<button
						onmousedown={() => sendButtonInput(btn.id, true)}
						onmouseup={() => sendButtonInput(btn.id, false)}
						onmouseleave={() => sendButtonInput(btn.id, false)}
						ontouchstart={(e) => { e.preventDefault(); sendButtonInput(btn.id, true); }}
						ontouchend={(e) => { e.preventDefault(); sendButtonInput(btn.id, false); }}
						style="grid-column: {btn.col + 1}"
						class="rounded bg-gray-700 px-2 py-1 text-xs font-bold text-gray-200 transition-all hover:bg-gray-600 active:bg-blue-600 active:scale-95 {isPressed(btn.id) ? 'bg-blue-600 ring-1 ring-blue-400' : ''}"
					>
						{btn.label}
					</button>
				{/each}

				<!-- Left stick visualization (row 2, cols 1-2) -->
				<div style="grid-column: 1 / 3" class="flex items-center justify-center rounded bg-gray-800 p-2">
					<div class="relative flex h-16 w-16 items-center justify-center rounded-full bg-gray-700">
						<!-- 8-directional click zones -->
						<button onclick={() => handleLeftStickClick(0, -1)} class="absolute top-0 left-1/2 -translate-x-1/2 text-[10px] text-gray-400 hover:text-white" title="上">↑</button>
						<button onclick={() => handleLeftStickClick(-1, 0)} class="absolute left-0 top-1/2 -translate-y-1/2 text-[10px] text-gray-400 hover:text-white" title="左">←</button>
						<button onclick={() => handleLeftStickClick(0, 0)} class="text-[10px] text-gray-400 hover:text-white" title="ニュートラル">●</button>
						<button onclick={() => handleLeftStickClick(1, 0)} class="absolute right-0 top-1/2 -translate-y-1/2 text-[10px] text-gray-400 hover:text-white" title="右">→</button>
						<button onclick={() => handleLeftStickClick(0, 1)} class="absolute bottom-0 left-1/2 -translate-x-1/2 text-[10px] text-gray-400 hover:text-white" title="下">↓</button>
						<!-- Stick position indicator -->
						<div
							class="pointer-events-none absolute h-4 w-4 rounded-full bg-blue-500/50 transition-all"
							style="transform: translate(calc({lx} * 12px), calc({ly} * 12px));"
						></div>
					</div>
					<div class="ml-1 flex flex-col items-center gap-0.5">
						<span class="text-[10px] text-gray-500">L</span>
						<button onclick={resetLeftStick} class="rounded bg-gray-700 px-1 py-0.5 text-[9px] text-gray-400 hover:bg-gray-600">RST</button>
					</div>
				</div>

				<!-- D-Pad (row 2, col 3) -->
				<div style="grid-column: 3" class="grid grid-cols-3 gap-0.5 place-content-center rounded bg-gray-800 p-1">
					{#each dpadButtons as btn (btn.id)}
						<button
							onmousedown={() => sendButtonInput(btn.id, true)}
							onmouseup={() => sendButtonInput(btn.id, false)}
							onmouseleave={() => sendButtonInput(btn.id, false)}
							ontouchstart={(e) => { e.preventDefault(); sendButtonInput(btn.id, true); }}
							ontouchend={(e) => { e.preventDefault(); sendButtonInput(btn.id, false); }}
							style="grid-row: {btn.row + 1}; grid-column: {btn.col + 1}"
							class="rounded bg-gray-700 px-1 py-0.5 text-xs text-gray-200 transition-all hover:bg-gray-600 active:bg-blue-600 active:scale-90 {isPressed(btn.id) ? 'bg-blue-600 ring-1 ring-blue-400' : ''}"
						>
							{btn.label}
						</button>
					{/each}
				</div>

				<!-- Right stick visualization (row 2, cols 4-5) -->
				<div style="grid-column: 4 / 6" class="flex items-center justify-center rounded bg-gray-800 p-2">
					<div class="mr-1 flex flex-col items-center gap-0.5">
						<span class="text-[10px] text-gray-500">R</span>
						<button onclick={resetRightStick} class="rounded bg-gray-700 px-1 py-0.5 text-[9px] text-gray-400 hover:bg-gray-600">RST</button>
					</div>
					<div class="relative flex h-16 w-16 items-center justify-center rounded-full bg-gray-700">
						<button onclick={() => handleRightStickClick(0, -1)} class="absolute top-0 left-1/2 -translate-x-1/2 text-[10px] text-gray-400 hover:text-white" title="上">↑</button>
						<button onclick={() => handleRightStickClick(-1, 0)} class="absolute left-0 top-1/2 -translate-y-1/2 text-[10px] text-gray-400 hover:text-white" title="左">←</button>
						<button onclick={() => handleRightStickClick(0, 0)} class="text-[10px] text-gray-400 hover:text-white" title="ニュートラル">●</button>
						<button onclick={() => handleRightStickClick(1, 0)} class="absolute right-0 top-1/2 -translate-y-1/2 text-[10px] text-gray-400 hover:text-white" title="右">→</button>
						<button onclick={() => handleRightStickClick(0, 1)} class="absolute bottom-0 left-1/2 -translate-x-1/2 text-[10px] text-gray-400 hover:text-white" title="下">↓</button>
						<div
							class="pointer-events-none absolute h-4 w-4 rounded-full bg-green-500/50 transition-all"
							style="transform: translate(calc({rx} * 12px), calc({ry} * 12px));"
						></div>
					</div>
				</div>

				<!-- System buttons (row 3, cols 1-2) -->
				<div style="grid-column: 1 / 3" class="grid grid-cols-4 gap-1">
					{#each systemButtons as btn (btn.id)}
						<button
							onmousedown={() => sendButtonInput(btn.id, true)}
							onmouseup={() => sendButtonInput(btn.id, false)}
							onmouseleave={() => sendButtonInput(btn.id, false)}
							ontouchstart={(e) => { e.preventDefault(); sendButtonInput(btn.id, true); }}
							ontouchend={(e) => { e.preventDefault(); sendButtonInput(btn.id, false); }}
							class="rounded bg-gray-700 px-1 py-1 text-[11px] transition-all hover:bg-gray-600 active:bg-blue-600 active:scale-90 {isPressed(btn.id) ? 'bg-blue-600 ring-1 ring-blue-400' : ''}"
						>
							{btn.label}
						</button>
					{/each}
				</div>

				<!-- Face buttons A/B/X/Y (row 3, cols 4-5) -->
				<div style="grid-column: 4 / 6" class="grid grid-cols-2 gap-1 place-content-center rounded bg-gray-800 p-2">
					{#each faceButtons as btn (btn.id)}
						<button
							onmousedown={() => sendButtonInput(btn.id, true)}
							onmouseup={() => sendButtonInput(btn.id, false)}
							onmouseleave={() => sendButtonInput(btn.id, false)}
							ontouchstart={(e) => { e.preventDefault(); sendButtonInput(btn.id, true); }}
							ontouchend={(e) => { e.preventDefault(); sendButtonInput(btn.id, false); }}
							class="rounded px-3 py-1.5 text-sm font-bold text-white transition-all hover:brightness-110 active:brightness-125 active:scale-95 {isPressed(btn.id) ? 'ring-2 ring-white/50' : ''} {btn.id === 'a' ? 'bg-green-600' : btn.id === 'b' ? 'bg-red-600' : btn.id === 'x' ? 'bg-blue-600' : 'bg-yellow-600'}"
						>
							{btn.label}
						</button>
					{/each}
				</div>
			</div>

			<!-- Pressed buttons display -->
			<div class="mt-2 flex flex-wrap gap-1 rounded bg-gray-800 p-1.5">
				<span class="text-[10px] text-gray-500">押下中:</span>
				{#if pressedButtons.size === 0}
					<span class="text-[10px] text-gray-600">なし</span>
				{:else}
					{#each [...pressedButtons] as btn (btn)}
						<span class="rounded bg-blue-800 px-1.5 py-0.5 text-[10px] text-blue-200">{btn}</span>
					{/each}
				{/if}
			</div>
		</div>
	</div>
{/if}
