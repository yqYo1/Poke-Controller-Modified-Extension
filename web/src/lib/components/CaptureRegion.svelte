<script lang="ts">
	import { onMount } from 'svelte';

	// ── Capture region state ───────────────────────────────────────────────────
	let canvasEl = $state<HTMLCanvasElement | null>(null);
	let isDrawing = $state(false);
	let startX = $state(0);
	let startY = $state(0);
	let currentX = $state(0);
	let currentY = $state(0);
	let hasSelection = $state(false);

	// Exposed region state (read-only from parent)
	let regionX = $state(0);
	let regionY = $state(0);
	let regionWidth = $state(0);
	let regionHeight = $state(0);

	// Preview image support
	let previewSrc = $state<string | null>(null);

	// Canvas dimensions
	let canvasWidth = $state(640);
	let canvasHeight = $state(480);

	// ── Exported props ─────────────────────────────────────────────────────────
	let {
		width = 640,
		height = 480,
		imageSrc = null,
	}: {
		width?: number;
		height?: number;
		imageSrc?: string | null;
	} = $props();

	$effect(() => {
		canvasWidth = width;
		canvasHeight = height;
		previewSrc = imageSrc;
		drawCanvas();
	});

	function getCanvasPos(e: MouseEvent): { x: number; y: number } {
		const rect = canvasEl!.getBoundingClientRect();
		const scaleX = canvasWidth / rect.width;
		const scaleY = canvasHeight / rect.height;
		return {
			x: (e.clientX - rect.left) * scaleX,
			y: (e.clientY - rect.top) * scaleY,
		};
	}

	function handleMouseDown(e: MouseEvent) {
		if (!canvasEl) return;
		const pos = getCanvasPos(e);
		isDrawing = true;
		startX = pos.x;
		startY = pos.y;
		currentX = pos.x;
		currentY = pos.y;
		hasSelection = false;
	}

	function handleMouseMove(e: MouseEvent) {
		if (!isDrawing || !canvasEl) return;
		const pos = getCanvasPos(e);
		currentX = Math.min(Math.max(pos.x, 0), canvasWidth);
		currentY = Math.min(Math.max(pos.y, 0), canvasHeight);

		const x = Math.min(startX, currentX);
		const y = Math.min(startY, currentY);
		const w = Math.abs(currentX - startX);
		const h = Math.abs(currentY - startY);

		regionX = Math.round(x);
		regionY = Math.round(y);
		regionWidth = Math.round(w);
		regionHeight = Math.round(h);

		drawCanvas();
	}

	function handleMouseUp(_e: MouseEvent) {
		if (!isDrawing) return;
		isDrawing = false;
		if (regionWidth > 5 && regionHeight > 5) {
			hasSelection = true;
		} else {
			clearSelection();
		}
		drawCanvas();
	}

	function clearSelection() {
		hasSelection = false;
		regionX = 0;
		regionY = 0;
		regionWidth = 0;
		regionHeight = 0;
		drawCanvas();
	}

	function drawCanvas() {
		const ctx = canvasEl?.getContext('2d');
		if (!ctx) return;

		// Clear
		ctx.clearRect(0, 0, canvasWidth, canvasHeight);

		// Draw preview image if available
		if (previewSrc) {
			const img = new Image();
			img.onload = () => {
				ctx.drawImage(img, 0, 0, canvasWidth, canvasHeight);
				drawOverlay(ctx);
			};
			img.src = previewSrc;
			// If image already loaded, the onload won't fire again, so draw overlay directly
			if (img.complete) {
				drawOverlay(ctx);
			}
		} else {
			// Draw checkerboard background
			drawCheckerboard(ctx);
			// Draw instructions
			ctx.fillStyle = 'rgba(255, 255, 255, 0.5)';
			ctx.font = '14px sans-serif';
			ctx.textAlign = 'center';
			ctx.fillText('キャプチャ範囲をドラッグして選択', canvasWidth / 2, canvasHeight / 2);
			drawOverlay(ctx);
		}
	}

	function drawCheckerboard(ctx: CanvasRenderingContext2D) {
		const tileSize = 16;
		for (let y = 0; y < canvasHeight; y += tileSize) {
			for (let x = 0; x < canvasWidth; x += tileSize) {
				ctx.fillStyle = (Math.floor(x / tileSize) + Math.floor(y / tileSize)) % 2 === 0
					? '#1f2937'
					: '#111827';
				ctx.fillRect(x, y, tileSize, tileSize);
			}
		}
	}

	function drawOverlay(ctx: CanvasRenderingContext2D) {
		if (!hasSelection && !isDrawing) return;

		const x = Math.min(startX, currentX);
		const y = Math.min(startY, currentY);
		const w = Math.abs(currentX - startX);
		const h = Math.abs(currentY - startY);

		if (w < 1 || h < 1) return;

		// Dimmed overlay outside selection
		ctx.fillStyle = 'rgba(0, 0, 0, 0.4)';
		// Top
		ctx.fillRect(0, 0, canvasWidth, y);
		// Left
		ctx.fillRect(0, y, x, h);
		// Right
		ctx.fillRect(x + w, y, canvasWidth - x - w, h);
		// Bottom
		ctx.fillRect(0, y + h, canvasWidth, canvasHeight - y - h);

		// Selection border
		ctx.strokeStyle = '#3b82f6';
		ctx.lineWidth = 2;
		ctx.strokeRect(x, y, w, h);

		// Corner handles
		const handleSize = 6;
		ctx.fillStyle = '#3b82f6';
		[
			[x, y],
			[x + w, y],
			[x, y + h],
			[x + w, y + h],
		].forEach(([hx, hy]) => {
			ctx.fillRect(hx - handleSize / 2, hy - handleSize / 2, handleSize, handleSize);
		});

		// Dimension label
		if (w > 60 && h > 20) {
			ctx.fillStyle = 'rgba(59, 130, 246, 0.9)';
			ctx.font = '11px monospace';
			ctx.textAlign = 'left';
			ctx.fillText(`${Math.round(w)}×${Math.round(h)}`, x + 4, y + 14);
		}
	}

	// Custom event helpers
	function emitCapture() {
		if (!hasSelection) return;
		window.dispatchEvent(
			new CustomEvent('capture-region-select', {
				detail: {
					x: regionX,
					y: regionY,
					width: regionWidth,
					height: regionHeight,
				},
			}),
		);
	}

	function emitCancel() {
		clearSelection();
		window.dispatchEvent(new CustomEvent('capture-region-cancel'));
	}

	onMount(() => {
		drawCanvas();
	});
</script>

<div class="space-y-2">
	<!-- Canvas area -->
	<div class="relative inline-block overflow-hidden rounded border border-gray-700">
		<canvas
			bind:this={canvasEl}
			width={canvasWidth}
			height={canvasHeight}
			onmousedown={handleMouseDown}
			onmousemove={handleMouseMove}
			onmouseup={handleMouseUp}
			onmouseleave={handleMouseUp}
			class="block cursor-crosshair"
			role="img"
			aria-label="キャプチャ範囲選択"
		/>
	</div>

	<!-- Selection info -->
	<div class="flex items-center justify-between text-xs text-gray-400">
		<div class="space-x-4">
			<span>
				範囲: ({regionX}, {regionY}) -
				{regionWidth}×{regionHeight}
			</span>
		</div>
		<div class="flex gap-2">
			{#if hasSelection}
				<button
					onclick={emitCapture}
					class="rounded bg-blue-600 px-3 py-1 text-xs font-medium text-white hover:bg-blue-500"
				>
					確定
				</button>
				<button
					onclick={emitCancel}
					class="rounded bg-gray-700 px-3 py-1 text-xs text-gray-300 hover:bg-gray-600"
				>
					キャンセル
				</button>
			{/if}
			<button
				onclick={clearSelection}
				class="rounded bg-gray-700 px-3 py-1 text-xs text-gray-300 hover:bg-gray-600"
			>
				クリア
			</button>
		</div>
	</div>
</div>
