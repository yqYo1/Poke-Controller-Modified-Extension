<script lang="ts">
	let { title = '出力' }: { title?: string } = $props();

	type WidgetMode = 'テキスト' | '画像' | '表' | 'グラフ' | 'ゲージ' | 'ログ' | 'カスタム';

	let selectedWidget = $state<WidgetMode>('テキスト');

	const widgetOptions: WidgetMode[] = ['テキスト', '画像', '表', 'グラフ', 'ゲージ', 'ログ', 'カスタム'];

	// ── Simulated data for graph/gauge demo ────────────────────────────────
	let sampleData = $state([30, 45, 25, 60, 40, 55, 35]);
	let gaugeValue = $state(67);
	let logEntries = $state([
		{ level: 'info', msg: 'システム初期化完了' },
		{ level: 'info', msg: 'シリアルポート接続済み' },
		{ level: 'warn', msg: 'フレームレート低下検出' },
		{ level: 'info', msg: 'コマンド「キャプチャ」開始' },
		{ level: 'error', msg: 'カメラデバイスが見つかりません' },
		{ level: 'info', msg: '自動再試行中...' },
	]);
	let logFilter = $state('all');
	let customContent = $state('');

	$derived(() => {
		if (logFilter === 'all') return logEntries;
		return logEntries.filter(e => e.level === logFilter);
	});

	// ── Bar chart dimensions ───────────────────────────────────────────────
	const barMax = 70;
	const barHeight = 80;
</script>

<div class="flex flex-col border border-gray-700 rounded bg-gray-900">
	<div class="flex items-center justify-between border-b border-gray-700 px-3 py-2">
		<span class="text-sm font-medium text-gray-200">{title}</span>
		<div class="flex flex-wrap gap-1">
			{#each widgetOptions as option (option)}
				<button
					onclick={() => (selectedWidget = option)}
					class="rounded px-2 py-0.5 text-xs transition-colors {selectedWidget === option ? 'bg-blue-600 text-white' : 'bg-gray-700 text-gray-300 hover:bg-gray-600'}"
				>
					{option}
				</button>
			{/each}
		</div>
	</div>
	<div class="min-h-[120px] overflow-auto p-3 font-mono text-xs text-gray-300">
		{#if selectedWidget === 'テキスト'}
			<p class="italic text-gray-500">テキスト出力はここに表示されます</p>
		{:else if selectedWidget === '画像'}
			<div class="flex items-center justify-center h-full text-gray-500">
				<span>画像プレビュー</span>
			</div>
		{:else if selectedWidget === '表'}
			<table class="w-full text-left text-xs">
				<thead>
					<tr class="border-b border-gray-700 text-gray-400">
						<th class="py-1 pr-2">項目</th>
						<th class="py-1">値</th>
					</tr>
				</thead>
				<tbody>
					<tr class="border-b border-gray-800">
						<td class="py-1 pr-2 text-gray-400">データ</td>
						<td class="py-1 text-gray-300">---</td>
					</tr>
				</tbody>
			</table>
		{:else if selectedWidget === 'グラフ'}
			<div class="flex flex-col gap-2">
				<div class="flex items-end gap-1" style="height: {barHeight}px;">
					{#each sampleData as val, i (i)}
						<div
							class="flex-1 rounded-t transition-all duration-300"
							style="height: {(val / barMax) * barHeight}px; background-color: hsl({(i / sampleData.length) * 360}, 70%, 55%); min-width: 20px;"
							title="{val}"
						>
						</div>
					{/each}
				</div>
				<div class="flex gap-1 text-[9px] text-gray-500">
					{#each sampleData as val, i (i)}
						<div class="flex-1 text-center">{val}</div>
					{/each}
				</div>
			</div>
		{:else if selectedWidget === 'ゲージ'}
			<div class="flex flex-col items-center justify-center gap-2 py-2">
				<!-- Circular gauge -->
				<div class="relative flex items-center justify-center" style="width: 80px; height: 80px;">
					<svg viewBox="0 0 36 36" class="h-full w-full -rotate-90">
						<path
							d="M18 2.0845
								a 15.9155 15.9155 0 0 1 0 31.831
								a 15.9155 15.9155 0 0 1 0 -31.831"
							fill="none"
							stroke="currentColor"
							stroke-width="3"
							class="text-gray-700"
						/>
						<path
							d="M18 2.0845
								a 15.9155 15.9155 0 0 1 0 31.831
								a 15.9155 15.9155 0 0 1 0 -31.831"
							fill="none"
							stroke="currentColor"
							stroke-width="3"
							stroke-dasharray="{gaugeValue}, 100"
							class="text-blue-500"
						/>
					</svg>
					<span class="absolute text-sm font-bold text-gray-200">{gaugeValue}%</span>
				</div>
				<input
					type="range"
					min="0"
					max="100"
					step="1"
					bind:value={gaugeValue}
					class="w-32 accent-blue-500"
				/>
			</div>
		{:else if selectedWidget === 'ログ'}
			<div class="flex flex-col gap-2">
				<div class="flex gap-1">
					<button
						onclick={() => (logFilter = 'all')}
						class="rounded px-1.5 py-0.5 text-[10px] {logFilter === 'all' ? 'bg-blue-600 text-white' : 'bg-gray-700 text-gray-400'}"
					>すべて</button>
					<button
						onclick={() => (logFilter = 'info')}
						class="rounded px-1.5 py-0.5 text-[10px] {logFilter === 'info' ? 'bg-blue-600 text-white' : 'bg-gray-700 text-gray-400'}"
					>INFO</button>
					<button
						onclick={() => (logFilter = 'warn')}
						class="rounded px-1.5 py-0.5 text-[10px] {logFilter === 'warn' ? 'bg-blue-600 text-white' : 'bg-gray-700 text-gray-400'}"
					>WARN</button>
					<button
						onclick={() => (logFilter = 'error')}
						class="rounded px-1.5 py-0.5 text-[10px] {logFilter === 'error' ? 'bg-blue-600 text-white' : 'bg-gray-700 text-gray-400'}"
					>ERROR</button>
				</div>
				<div class="space-y-0.5 max-h-24 overflow-y-auto">
					{#each logFilter === 'all' ? logEntries : logEntries.filter(e => e.level === logFilter) as entry (entry.msg)}
						<div class="flex gap-1.5 text-[10px] leading-relaxed">
							<span class="font-medium shrink-0 w-10
								{entry.level === 'info' ? 'text-blue-400' : ''}
								{entry.level === 'warn' ? 'text-yellow-400' : ''}
								{entry.level === 'error' ? 'text-red-400' : ''}
							">
								{entry.level.toUpperCase()}
							</span>
							<span class="text-gray-300">{entry.msg}</span>
						</div>
					{/each}
				</div>
			</div>
		{:else if selectedWidget === 'カスタム'}
			<div class="flex flex-col gap-2">
				<p class="italic text-gray-500 text-[10px]">カスタムコンテンツを以下に入力:</p>
				<textarea
					class="w-full rounded border border-gray-700 bg-gray-800 p-2 text-xs text-gray-200 resize-y"
					rows={4}
					bind:value={customContent}
					placeholder="自由に記述してください..."
				></textarea>
				{#if customContent}
					<div class="rounded bg-gray-800 p-2 text-xs text-gray-300 whitespace-pre-wrap">
						{customContent}
					</div>
				{/if}
			</div>
		{/if}
	</div>
</div>