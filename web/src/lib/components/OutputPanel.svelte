<script lang="ts">
	let { title = '出力' }: { title?: string } = $props();

	let selectedWidget = $state<'テキスト' | '画像' | '表'>('テキスト');

	const widgetOptions = ['テキスト', '画像', '表'] as const;
</script>

<div class="flex flex-col border border-gray-700 rounded bg-gray-900">
	<div class="flex items-center justify-between border-b border-gray-700 px-3 py-2">
		<span class="text-sm font-medium text-gray-200">{title}</span>
		<div class="flex gap-1">
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
		{/if}
	</div>
</div>
