/* eslint-disable @typescript-eslint/no-unused-vars */
<script lang="ts">
	let {
		showPreview = true,
		showSimilarity = false,
		showGuide = false,
		guideType = 'crosshair' as 'crosshair' | 'grid' | 'both' | 'none',
		displaySize = 100,
		scaleMode = 'fit' as 'fit' | 'fill' | 'stretch',
		onShowPreviewChange = (_v: boolean) => {},
		onShowSimilarityChange = (_v: boolean) => {},
		onShowGuideChange = (_v: boolean) => {},
		onGuideTypeChange = (_v: string) => {},
		onDisplaySizeChange = (_v: number) => {},
		onScaleModeChange = (_v: string) => {},
	}: {
		showPreview?: boolean;
		showSimilarity?: boolean;
		showGuide?: boolean;
		guideType?: 'crosshair' | 'grid' | 'both' | 'none';
		displaySize?: number;
		scaleMode?: 'fit' | 'fill' | 'stretch';
		onShowPreviewChange?: (v: boolean) => void;
		onShowSimilarityChange?: (v: boolean) => void;
		onShowGuideChange?: (v: boolean) => void;
		onGuideTypeChange?: (v: string) => void;
		onDisplaySizeChange?: (v: number) => void;
		onScaleModeChange?: (v: string) => void;
	} = $props();

	let internalShowPreview = $state(showPreview);
	let internalShowSimilarity = $state(showSimilarity);
	let internalShowGuide = $state(showGuide);
	let internalGuideType = $state(guideType);
	let internalDisplaySize = $state(displaySize);
	let internalScaleMode = $state(scaleMode);

	function handleShowPreviewChange() {
		onShowPreviewChange(internalShowPreview);
	}

	function handleShowSimilarityChange() {
		onShowSimilarityChange(internalShowSimilarity);
	}

	function handleShowGuideChange() {
		onShowGuideChange(internalShowGuide);
	}

	function handleGuideTypeChange(e: Event) {
		const target = e.target as HTMLSelectElement;
		internalGuideType = target.value as typeof internalGuideType;
		onGuideTypeChange(internalGuideType);
	}

	function handleDisplaySizeChange(e: Event) {
		const target = e.target as HTMLInputElement;
		internalDisplaySize = Number(target.value);
		onDisplaySizeChange(internalDisplaySize);
	}

	function handleScaleModeChange(e: Event) {
		const target = e.target as HTMLSelectElement;
		internalScaleMode = target.value as typeof internalScaleMode;
		onScaleModeChange(internalScaleMode);
	}
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<h3 class="mb-3 text-sm font-medium text-gray-200">表示設定</h3>

	<!-- Real-time display toggle -->
	<div class="mb-3">
		<label class="flex cursor-pointer items-center gap-2">
			<input
				type="checkbox"
				bind:checked={internalShowPreview}
				onchange={handleShowPreviewChange}
				class="accent-blue-500"
			/>
			<span class="text-xs text-gray-300">リアルタイム表示</span>
		</label>
		<p class="mt-0.5 text-[10px] text-gray-500">カメラプレビューの表示/非表示を切り替えます</p>
	</div>

	<!-- Similarity display toggle -->
	<div class="mb-3">
		<label class="flex cursor-pointer items-center gap-2">
			<input
				type="checkbox"
				bind:checked={internalShowSimilarity}
				onchange={handleShowSimilarityChange}
				class="accent-blue-500"
			/>
			<span class="text-xs text-gray-300">類似度表示</span>
		</label>
		<p class="mt-0.5 text-[10px] text-gray-500">テンプレートマッチングの類似度スコアを表示します</p>
	</div>

	<!-- Guide display -->
	<div class="mb-3">
		<label class="flex cursor-pointer items-center gap-2">
			<input
				type="checkbox"
				bind:checked={internalShowGuide}
				onchange={handleShowGuideChange}
				class="accent-blue-500"
			/>
			<span class="text-xs text-gray-300">ガイド表示</span>
		</label>
	</div>

	{#if internalShowGuide}
		<div class="mb-3 ml-4">
			<label class="mb-1 block text-xs text-gray-400">ガイド種類</label>
			<select
				value={internalGuideType}
				onchange={handleGuideTypeChange}
				class="w-full rounded bg-gray-800 px-2 py-1.5 text-xs text-gray-200"
			>
				<option value="crosshair">クロスヘア</option>
				<option value="grid">グリッド</option>
				<option value="both">両方</option>
				<option value="none">なし</option>
			</select>
		</div>
	{/if}

	<!-- Display size adjustment -->
	<div class="mb-3">
		<label class="mb-1 block text-xs text-gray-400">
			表示サイズ: {internalDisplaySize}%
		</label>
		<input
			type="range"
			min="25"
			max="200"
			step="5"
			value={internalDisplaySize}
			oninput={handleDisplaySizeChange}
			class="w-full accent-blue-500"
		/>
		<div class="flex justify-between text-[10px] text-gray-500">
			<span>25%</span>
			<span>100%</span>
			<span>200%</span>
		</div>
	</div>

	<!-- Canvas scaling options -->
	<div>
		<label class="mb-1 block text-xs text-gray-400">スケーリングモード</label>
		<select
			value={internalScaleMode}
			onchange={handleScaleModeChange}
			class="w-full rounded bg-gray-800 px-2 py-1.5 text-xs text-gray-200"
		>
			<option value="fit">フィット（縦横比維持）</option>
			<option value="fill">フィル（領域を埋める）</option>
			<option value="stretch">ストレッチ（引き伸ばし）</option>
		</select>
		<p class="mt-0.5 text-[10px] text-gray-500">
			{#if internalScaleMode === 'fit'}
				プレビュー領域に収まるよう、縦横比を維持して表示します
			{:else if internalScaleMode === 'fill'}
				プレビュー領域を埋めるよう拡大します（はみ出し部分はクリップ）
			{:else}
				縦横比を無視してプレビュー領域に合わせます
			{/if}
		</p>
	</div>
</div>
