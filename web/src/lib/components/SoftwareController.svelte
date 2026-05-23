<script lang="ts">
	import { api } from '$lib/api/client';
	import { SvelteSet } from 'svelte/reactivity';

	// ── Constants ────────────────────────────────────────────────────────────────

	const DEAD_ZONE_MIN = 103;
	const DEAD_ZONE_MAX = 153;

	// ── Reactive state ───────────────────────────────────────────────────────────

	let pressedButtons = new SvelteSet<string>();

	// Left analog stick (0–255)
	let leftStickX = $state(128);
	let leftStickY = $state(128);
	let leftStickDragging = $state(false);
	let leftStickEl: HTMLDivElement | undefined = $state();

	// Right analog stick (0–255)
	let rightStickX = $state(128);
	let rightStickY = $state(128);
	let rightStickDragging = $state(false);
	let rightStickEl: HTMLDivElement | undefined = $state();

	// Touch screen (320×240)
	let touchX = $state<number>(160);
	let touchY = $state<number>(120);
	let isTouchActive = $state(false);
	let touchEl: HTMLDivElement | undefined = $state();

	// ── Helpers ──────────────────────────────────────────────────────────────────

	function clamp(value: number, min: number, max: number): number {
		return Math.max(min, Math.min(max, value));
	}

	function isInDeadZone(value: number): boolean {
		return value >= DEAD_ZONE_MIN && value <= DEAD_ZONE_MAX;
	}

	// ── Button input handlers ────────────────────────────────────────────────────

	function handleButtonDown(id: string): void {
		pressedButtons.add(id);
		api.sendInput('button', { button: id, pressed: true }).catch(console.warn);
	}

	function handleButtonUp(id: string, event: MouseEvent): void {
		if (event.shiftKey) {
			// Shift+Release: holdEndSkip — toggle visual state WITHOUT sending release
			pressedButtons.delete(id);
		} else {
			pressedButtons.delete(id);
			api.sendInput('button', { button: id, pressed: false }).catch(console.warn);
		}
	}

	function handleButtonLeave(id: string): void {
		if (pressedButtons.has(id)) {
			pressedButtons.delete(id);
			api.sendInput('button', { button: id, pressed: false }).catch(console.warn);
		}
	}

	// ── Analog stick input handlers ──────────────────────────────────────────────

	function getStickCoords(event: MouseEvent, element: HTMLElement): { x: number; y: number } {
		const rect = element.getBoundingClientRect();
		const rawX = ((event.clientX - rect.left) / rect.width) * 255;
		const rawY = ((event.clientY - rect.top) / rect.height) * 255;
		return {
			x: clamp(Math.round(rawX), 0, 255),
			y: clamp(Math.round(rawY), 0, 255),
		};
	}

	function startLeftStickDrag(event: MouseEvent): void {
		leftStickDragging = true;
		if (leftStickEl) {
			const { x, y } = getStickCoords(event, leftStickEl);
			leftStickX = x;
			leftStickY = y;
			api.sendInput('stick', { stick: 'LSTICK', x, y }).catch(console.warn);
		}
	}

	function onLeftStickMove(event: MouseEvent): void {
		if (!leftStickDragging || !leftStickEl) return;
		const { x, y } = getStickCoords(event, leftStickEl);
		leftStickX = x;
		leftStickY = y;
		api.sendInput('stick', { stick: 'LSTICK', x, y }).catch(console.warn);
	}

	function endLeftStickDrag(): void {
		if (!leftStickDragging) return;
		leftStickDragging = false;
		// If in dead zone, reset to exact center
		if (isInDeadZone(leftStickX) && isInDeadZone(leftStickY)) {
			leftStickX = 128;
			leftStickY = 128;
			api.sendInput('stick', { stick: 'LSTICK', x: 128, y: 128 }).catch(console.warn);
		}
	}

	function startRightStickDrag(event: MouseEvent): void {
		rightStickDragging = true;
		if (rightStickEl) {
			const { x, y } = getStickCoords(event, rightStickEl);
			rightStickX = x;
			rightStickY = y;
			api.sendInput('stick', { stick: 'RSTICK', x, y }).catch(console.warn);
		}
	}

	function onRightStickMove(event: MouseEvent): void {
		if (!rightStickDragging || !rightStickEl) return;
		const { x, y } = getStickCoords(event, rightStickEl);
		rightStickX = x;
		rightStickY = y;
		api.sendInput('stick', { stick: 'RSTICK', x, y }).catch(console.warn);
	}

	function endRightStickDrag(): void {
		if (!rightStickDragging) return;
		rightStickDragging = false;
		if (isInDeadZone(rightStickX) && isInDeadZone(rightStickY)) {
			rightStickX = 128;
			rightStickY = 128;
			api.sendInput('stick', { stick: 'RSTICK', x: 128, y: 128 }).catch(console.warn);
		}
	}

	function centerLeftStick(): void {
		leftStickX = 128;
		leftStickY = 128;
		api.sendInput('stick', { stick: 'LSTICK', x: 128, y: 128 }).catch(console.warn);
	}

	function centerRightStick(): void {
		rightStickX = 128;
		rightStickY = 128;
		api.sendInput('stick', { stick: 'RSTICK', x: 128, y: 128 }).catch(console.warn);
	}

	// ── Touch screen input handlers ──────────────────────────────────────────────

	function handleTouchDown(event: MouseEvent): void {
		if (!touchEl) return;
		isTouchActive = true;
		const rect = touchEl.getBoundingClientRect();
		const rawX = ((event.clientX - rect.left) / rect.width) * 320;
		const rawY = ((event.clientY - rect.top) / rect.height) * 240;
		const x = clamp(Math.round(rawX), 0, 320);
		const y = clamp(Math.round(rawY), 0, 240);
		touchX = x;
		touchY = y;
		api.sendInput('touch', { x, y }).catch(console.warn);
	}

	function handleTouchMove(event: MouseEvent): void {
		if (!isTouchActive || !touchEl) return;
		const rect = touchEl.getBoundingClientRect();
		const rawX = ((event.clientX - rect.left) / rect.width) * 320;
		const rawY = ((event.clientY - rect.top) / rect.height) * 240;
		const x = clamp(Math.round(rawX), 0, 320);
		const y = clamp(Math.round(rawY), 0, 240);
		touchX = x;
		touchY = y;
		api.sendInput('touch', { x, y }).catch(console.warn);
	}

	function handleTouchUp(): void {
		isTouchActive = false;
	}
</script>

<div class="software-controller">
	<!-- ═══ ROW 1: Shoulder Buttons ═══ -->
	<div class="joycon-row shoulder-row">
		<!-- Joy-Con L shoulder -->
		<div class="joycon-l-section">
			<button
				class="joycon-btn shoulder"
				onmousedown={() => handleButtonDown('zl')}
				onmouseup={(e) => handleButtonUp('zl', e)}
				onmouseleave={() => handleButtonLeave('zl')}
				class:pressed={pressedButtons.has('zl')}
			>ZL</button>
			<button
				class="joycon-btn shoulder"
				onmousedown={() => handleButtonDown('l')}
				onmouseup={(e) => handleButtonUp('l', e)}
				onmouseleave={() => handleButtonLeave('l')}
				class:pressed={pressedButtons.has('l')}
			>L</button>
		</div>
		<!-- Center gap -->
		<div class="joycon-c-section shoulder-gap"></div>
		<!-- Joy-Con R shoulder -->
		<div class="joycon-r-section">
			<button
				class="joycon-btn shoulder"
				onmousedown={() => handleButtonDown('r')}
				onmouseup={(e) => handleButtonUp('r', e)}
				onmouseleave={() => handleButtonLeave('r')}
				class:pressed={pressedButtons.has('r')}
			>R</button>
			<button
				class="joycon-btn shoulder"
				onmousedown={() => handleButtonDown('zr')}
				onmouseup={(e) => handleButtonUp('zr', e)}
				onmouseleave={() => handleButtonLeave('zr')}
				class:pressed={pressedButtons.has('zr')}
			>ZR</button>
		</div>
	</div>

	<!-- ═══ ROW 2: Analog Sticks + System Buttons ═══ -->
	<div class="joycon-row mid-row">
		<!-- Joy-Con L: L-stick -->
		<div class="joycon-l-section">
			<div class="stick-area joycon-l-stick">
				<div class="stick-label">L</div>
				<div
					class="stick-pad"
					bind:this={leftStickEl}
					onmousedown={startLeftStickDrag}
					onmousemove={onLeftStickMove}
					onmouseup={endLeftStickDrag}
					onmouseleave={endLeftStickDrag}
					role="slider"
					aria-label="Left analog stick"
					aria-valuemin={0}
					aria-valuemax={255}
					aria-valuenow={leftStickX}
				>
					<div class="stick-marker" style="left: {(leftStickX / 255) * 100}%; top: {(leftStickY / 255) * 100}%;"></div>
					<div class="stick-deadzone-ring"></div>
					<div class="stick-crosshair-x"></div>
					<div class="stick-crosshair-y"></div>
				</div>
				<div class="stick-coords">
					X:<span class="coord-val">{leftStickX}</span>
					Y:<span class="coord-val">{leftStickY}</span>
					<button class="stick-center-btn" onclick={centerLeftStick} title="Center stick">◎</button>
				</div>
			</div>
		</div>

		<!-- Center: System buttons -->
		<div class="joycon-c-section">
			<div class="sys-buttons">
				<div class="sys-btn-row">
					<button
						class="joycon-btn sys"
						onmousedown={() => handleButtonDown('minus')}
						onmouseup={(e) => handleButtonUp('minus', e)}
						onmouseleave={() => handleButtonLeave('minus')}
						class:pressed={pressedButtons.has('minus')}
						title="MINUS"
					>−</button>
					<button
						class="joycon-btn sys capture"
						onmousedown={() => handleButtonDown('capture')}
						onmouseup={(e) => handleButtonUp('capture', e)}
						onmouseleave={() => handleButtonLeave('capture')}
						class:pressed={pressedButtons.has('capture')}
						title="CAPTURE"
					>●</button>
				</div>
				<div class="sys-btn-row">
					<button
						class="joycon-btn sys home"
						onmousedown={() => handleButtonDown('home')}
						onmouseup={(e) => handleButtonUp('home', e)}
						onmouseleave={() => handleButtonLeave('home')}
						class:pressed={pressedButtons.has('home')}
						title="HOME"
					>⌂</button>
					<button
						class="joycon-btn sys"
						onmousedown={() => handleButtonDown('plus')}
						onmouseup={(e) => handleButtonUp('plus', e)}
						onmouseleave={() => handleButtonLeave('plus')}
						class:pressed={pressedButtons.has('plus')}
						title="PLUS"
					>+</button>
				</div>
			</div>
		</div>

		<!-- Joy-Con R: R-stick -->
		<div class="joycon-r-section">
			<div class="stick-area joycon-r-stick">
				<div class="stick-label">R</div>
				<div
					class="stick-pad"
					bind:this={rightStickEl}
					onmousedown={startRightStickDrag}
					onmousemove={onRightStickMove}
					onmouseup={endRightStickDrag}
					onmouseleave={endRightStickDrag}
					role="slider"
					aria-label="Right analog stick"
					aria-valuemin={0}
					aria-valuemax={255}
					aria-valuenow={rightStickX}
				>
					<div class="stick-marker" style="left: {(rightStickX / 255) * 100}%; top: {(rightStickY / 255) * 100}%;"></div>
					<div class="stick-deadzone-ring"></div>
					<div class="stick-crosshair-x"></div>
					<div class="stick-crosshair-y"></div>
				</div>
				<div class="stick-coords">
					X:<span class="coord-val">{rightStickX}</span>
					Y:<span class="coord-val">{rightStickY}</span>
					<button class="stick-center-btn" onclick={centerRightStick} title="Center stick">◎</button>
				</div>
			</div>
		</div>
	</div>

	<!-- ═══ ROW 3: D-Pad + Touch Screen + Face Buttons ═══ -->
	<div class="joycon-row bottom-row">
		<!-- Joy-Con L: D-Pad -->
		<div class="joycon-l-section">
			<div class="dpad">
				<div class="dpad-grid">
					<!-- Row 0 -->
					<div></div>
					<button
						class="joycon-btn dpad dpad-up"
						onmousedown={() => handleButtonDown('up')}
						onmouseup={(e) => handleButtonUp('up', e)}
						onmouseleave={() => handleButtonLeave('up')}
						class:pressed={pressedButtons.has('up')}
					>↑</button>
					<div></div>
					<!-- Row 1 -->
					<button
						class="joycon-btn dpad dpad-left"
						onmousedown={() => handleButtonDown('left')}
						onmouseup={(e) => handleButtonUp('left', e)}
						onmouseleave={() => handleButtonLeave('left')}
						class:pressed={pressedButtons.has('left')}
					>←</button>
					<div class="dpad-center"></div>
					<button
						class="joycon-btn dpad dpad-right"
						onmousedown={() => handleButtonDown('right')}
						onmouseup={(e) => handleButtonUp('right', e)}
						onmouseleave={() => handleButtonLeave('right')}
						class:pressed={pressedButtons.has('right')}
					>→</button>
					<!-- Row 2 -->
					<div></div>
					<button
						class="joycon-btn dpad dpad-down"
						onmousedown={() => handleButtonDown('down')}
						onmouseup={(e) => handleButtonUp('down', e)}
						onmouseleave={() => handleButtonLeave('down')}
						class:pressed={pressedButtons.has('down')}
					>↓</button>
					<div></div>
				</div>
			</div>
		</div>

		<!-- Center: Touch Screen -->
		<div class="joycon-c-section">
			<div class="touch-container">
				<div class="touch-label">Touch (320×240)</div>
				<div
					class="touch-pad"
					bind:this={touchEl}
					onmousedown={handleTouchDown}
					onmousemove={handleTouchMove}
					onmouseup={handleTouchUp}
					onmouseleave={handleTouchUp}
					role="grid"
					aria-label="Touch screen 320x240"
				>
					{#if isTouchActive}
						<div class="touch-marker" style="left: {(touchX / 320) * 100}%; top: {(touchY / 240) * 100}%;"></div>
					{/if}
				</div>
				<div class="touch-coords">
					{isTouchActive ? `(${touchX}, ${touchY})` : 'Click to touch'}
				</div>
			</div>
		</div>

		<!-- Joy-Con R: Face buttons A/B/X/Y -->
		<div class="joycon-r-section">
			<div class="face-buttons">
				<div class="face-grid">
					<!-- Row 0: X -->
					<div></div>
					<button
						class="joycon-btn face face-x"
						onmousedown={() => handleButtonDown('x')}
						onmouseup={(e) => handleButtonUp('x', e)}
						onmouseleave={() => handleButtonLeave('x')}
						class:pressed={pressedButtons.has('x')}
					>X</button>
					<div></div>
					<!-- Row 1: Y, A -->
					<button
						class="joycon-btn face face-y"
						onmousedown={() => handleButtonDown('y')}
						onmouseup={(e) => handleButtonUp('y', e)}
						onmouseleave={() => handleButtonLeave('y')}
						class:pressed={pressedButtons.has('y')}
					>Y</button>
					<div class="face-center-gap"></div>
					<button
						class="joycon-btn face face-a"
						onmousedown={() => handleButtonDown('a')}
						onmouseup={(e) => handleButtonUp('a', e)}
						onmouseleave={() => handleButtonLeave('a')}
						class:pressed={pressedButtons.has('a')}
					>A</button>
					<!-- Row 2: B -->
					<div></div>
					<button
						class="joycon-btn face face-b"
						onmousedown={() => handleButtonDown('b')}
						onmouseup={(e) => handleButtonUp('b', e)}
						onmouseleave={() => handleButtonLeave('b')}
						class:pressed={pressedButtons.has('b')}
					>B</button>
					<div></div>
				</div>
			</div>
		</div>
	</div>
</div>

<style>
	/* ── Root container ─────────────────────────────────────────────────────── */
	.software-controller {
		display: flex;
		flex-direction: column;
		gap: 3px;
		width: 100%;
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		user-select: none;
	}

	/* ── Row layout ─────────────────────────────────────────────────────────── */
	.joycon-row {
		display: flex;
		gap: 2px;
		align-items: stretch;
	}

	/* Section widths: L and R get ~38% each, center gets ~24% */
	.joycon-l-section {
		flex: 0 0 auto;
		display: flex;
		align-items: center;
		gap: 2px;
		padding: 2px;
		border: 1px solid #56CCF2;
		border-radius: 3px;
		background-color: rgba(86, 204, 242, 0.06);
	}

	.joycon-r-section {
		flex: 0 0 auto;
		display: flex;
		align-items: center;
		gap: 2px;
		padding: 2px;
		border: 1px solid #E9514E;
		border-radius: 3px;
		background-color: rgba(233, 81, 78, 0.06);
	}

	.joycon-c-section {
		flex: 1 1 auto;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 2px;
	}

	/* ── Generic button base ────────────────────────────────────────────────── */
	.joycon-btn {
		font-family: 'Segoe UI', system-ui, sans-serif;
		font-size: 11px;
		font-weight: 600;
		border: 1px solid #475569;
		border-radius: 3px;
		background-color: #1e293b;
		color: #e2e8f0;
		cursor: pointer;
		padding: 2px 6px;
		text-align: center;
		line-height: 1.3;
		transition: background-color 0.08s, color 0.08s, border-color 0.08s;
		user-select: none;
		touch-action: manipulation;
	}

	.joycon-btn:hover {
		background-color: #334155;
	}

	.joycon-btn:active,
	.joycon-btn.pressed {
		background-color: #FFD800 !important;
		color: #1e293b !important;
		border-color: #FFD800 !important;
		box-shadow: 0 0 4px rgba(255, 216, 0, 0.5);
	}

	/* ── Shoulder buttons ───────────────────────────────────────────────────── */
	.shoulder-row .joycon-l-section,
	.shoulder-row .joycon-r-section {
		gap: 3px;
	}

	.shoulder-gap {
		min-width: 8px;
	}

	.joycon-btn.shoulder {
		padding: 3px 10px;
		font-size: 10px;
		min-width: 28px;
		border-radius: 4px;
	}

	.joycon-l-section .joycon-btn.shoulder {
		border-color: #56CCF2;
		color: #bee9f8;
	}

	.joycon-r-section .joycon-btn.shoulder {
		border-color: #E9514E;
		color: #f5b7b5;
	}

	/* ── Analog stick area ──────────────────────────────────────────────────── */
	.stick-area {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		padding: 2px;
	}

	.stick-label {
		font-size: 9px;
		font-weight: 700;
		letter-spacing: 0.5px;
		opacity: 0.7;
	}

	.joycon-l-stick .stick-label {
		color: #56CCF2;
	}

	.joycon-r-stick .stick-label {
		color: #E9514E;
	}

	.stick-pad {
		position: relative;
		width: 56px;
		height: 56px;
		border-radius: 50%;
		border: 2px solid #475569;
		background-color: #0f172a;
		cursor: crosshair;
		overflow: hidden;
	}

	.joycon-l-stick .stick-pad {
		border-color: #56CCF2;
	}

	.joycon-r-stick .stick-pad {
		border-color: #E9514E;
	}

	.stick-marker {
		position: absolute;
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background-color: #FFD800;
		transform: translate(-50%, -50%);
		pointer-events: none;
		z-index: 3;
		box-shadow: 0 0 4px rgba(255, 216, 0, 0.6);
	}

	.stick-deadzone-ring {
		position: absolute;
		top: 50%;
		left: 50%;
		width: 60%;
		height: 60%;
		border-radius: 50%;
		border: 1px dashed rgba(255, 255, 255, 0.15);
		transform: translate(-50%, -50%);
		pointer-events: none;
		z-index: 1;
	}

	.stick-crosshair-x,
	.stick-crosshair-y {
		position: absolute;
		background-color: rgba(255, 255, 255, 0.08);
		pointer-events: none;
		z-index: 1;
	}

	.stick-crosshair-x {
		top: 50%;
		left: 0;
		right: 0;
		height: 1px;
		transform: translateY(-50%);
	}

	.stick-crosshair-y {
		left: 50%;
		top: 0;
		bottom: 0;
		width: 1px;
		transform: translateX(-50%);
	}

	.stick-coords {
		display: flex;
		align-items: center;
		gap: 3px;
		font-size: 9px;
		color: #94a3b8;
		font-family: 'Cascadia Code', 'Consolas', monospace;
	}

	.coord-val {
		color: #e2e8f0;
		min-width: 18px;
		text-align: right;
	}

	.stick-center-btn {
		background: none;
		border: none;
		color: #64748b;
		cursor: pointer;
		font-size: 10px;
		padding: 0 2px;
		line-height: 1;
	}

	.stick-center-btn:hover {
		color: #FFD800;
	}

	/* ── System buttons ─────────────────────────────────────────────────────── */
	.sys-buttons {
		display: flex;
		flex-direction: column;
		gap: 3px;
		align-items: center;
	}

	.sys-btn-row {
		display: flex;
		gap: 4px;
	}

	.joycon-btn.sys {
		min-width: 28px;
		padding: 3px 5px;
		font-size: 12px;
		border-radius: 50%;
		width: 28px;
		height: 28px;
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.joycon-btn.sys.capture {
		font-size: 10px;
	}

	.joycon-btn.sys.home {
		font-size: 13px;
	}

	/* ── D-Pad ──────────────────────────────────────────────────────────────── */
	.dpad-grid {
		display: grid;
		grid-template-columns: 24px 24px 24px;
		grid-template-rows: 24px 24px 24px;
		gap: 1px;
		place-items: center;
	}

	.dpad-center {
		width: 10px;
		height: 10px;
		border-radius: 50%;
		background-color: #475569;
		border: 1px solid #334155;
	}

	.joycon-btn.dpad {
		width: 22px;
		height: 22px;
		padding: 0;
		font-size: 11px;
		border-radius: 3px;
		display: flex;
		align-items: center;
		justify-content: center;
		line-height: 1;
	}

	.joycon-btn.dpad.dpad-up {
		grid-column: 2;
		grid-row: 1;
	}

	.joycon-btn.dpad.dpad-left {
		grid-column: 1;
		grid-row: 2;
	}

	.joycon-btn.dpad.dpad-right {
		grid-column: 3;
		grid-row: 2;
	}

	.joycon-btn.dpad.dpad-down {
		grid-column: 2;
		grid-row: 3;
	}

	/* ── Face buttons (ABXY) ────────────────────────────────────────────────── */
	.face-grid {
		display: grid;
		grid-template-columns: 22px 10px 22px;
		grid-template-rows: 22px 22px 22px;
		gap: 1px;
		place-items: center;
	}

	.face-center-gap {
		width: 1px;
	}

	.joycon-btn.face {
		width: 22px;
		height: 22px;
		padding: 0;
		font-size: 10px;
		font-weight: 700;
		border-radius: 50%;
		display: flex;
		align-items: center;
		justify-content: center;
		line-height: 1;
	}

	.joycon-btn.face.face-x {
		grid-column: 2;
		grid-row: 1;
		background-color: #3b82f6;
		border-color: #2563eb;
		color: #dbeafe;
	}

	.joycon-btn.face.face-y {
		grid-column: 1;
		grid-row: 2;
		background-color: #eab308;
		border-color: #ca8a04;
		color: #fefce8;
	}

	.joycon-btn.face.face-a {
		grid-column: 3;
		grid-row: 2;
		background-color: #22c55e;
		border-color: #16a34a;
		color: #f0fdf4;
	}

	.joycon-btn.face.face-b {
		grid-column: 2;
		grid-row: 3;
		background-color: #ef4444;
		border-color: #dc2626;
		color: #fef2f2;
	}

	.joycon-btn.face:active,
	.joycon-btn.face.pressed {
		background-color: #FFD800 !important;
		color: #1e293b !important;
		border-color: #FFD800 !important;
		box-shadow: 0 0 4px rgba(255, 216, 0, 0.5);
	}

	/* ── Touch screen ───────────────────────────────────────────────────────── */
	.touch-container {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2px;
		width: 100%;
	}

	.touch-label {
		font-size: 9px;
		color: #64748b;
		font-weight: 600;
		letter-spacing: 0.3px;
	}

	.touch-pad {
		position: relative;
		width: 100%;
		min-width: 80px;
		max-width: 120px;
		height: 52px;
		border: 1px solid #475569;
		border-radius: 3px;
		background-color: #0f172a;
		cursor: crosshair;
		overflow: hidden;
	}

	.touch-pad:hover {
		border-color: #64748b;
	}

	.touch-marker {
		position: absolute;
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background-color: #FFD800;
		transform: translate(-50%, -50%);
		pointer-events: none;
		z-index: 2;
		box-shadow: 0 0 4px rgba(255, 216, 0, 0.7);
	}

	.touch-coords {
		font-size: 9px;
		color: #64748b;
		font-family: 'Cascadia Code', 'Consolas', monospace;
		text-align: center;
		min-height: 12px;
	}
</style>
