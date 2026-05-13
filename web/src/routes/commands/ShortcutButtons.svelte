<script lang="ts">
	import { api } from "$lib/api/client";

	let buttons = $state(["A", "B", "X", "Y", "L", "R", "ZL", "ZR", "START", "SELECT", "HOME", "CAPTURE"]);
</script>

<div class="rounded border border-gray-700 bg-gray-900 p-3">
	<h3 class="mb-2 text-sm font-medium text-gray-200">ショートカットボタン</h3>
	<div class="flex flex-wrap gap-2">
		{#each buttons as btn (btn)}
			<button
				onclick={() => api.sendInput("press", { buttons: [btn], duration: 100 }).catch(console.warn)}
				class="rounded bg-gray-700 px-3 py-1 text-xs text-gray-200 hover:bg-gray-600 active:bg-blue-600"
			>
				{btn}
			</button>
		{/each}
	</div>
</div>
