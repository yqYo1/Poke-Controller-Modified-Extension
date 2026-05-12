<script lang="ts">
	import { page } from '$app/stores';
	import { base } from '$app/paths';

	const tabs = [
		{ label: 'カメラ', href: `${base}/camera` },
		{ label: 'シリアル', href: `${base}/serial` },
		{ label: '手動制御', href: `${base}/manual` },
		{ label: 'コマンド', href: `${base}/commands` },
		{ label: 'キーコンフィグ', href: `${base}/keyconfig` },
		{ label: 'Pokemon Home', href: `${base}/pokemonhome` },
		{ label: '通知', href: `${base}/notification` },
		{ label: 'その他', href: `${base}/others` },
	];
</script>

<nav class="flex bg-gray-800 text-white">
	{#each tabs as { label, href }}
		<a
			{href}
			class="px-4 py-3 text-sm font-medium transition-colors hover:bg-gray-700 aria-[current=page]:bg-blue-600 aria-[current=page]:text-white"
			aria-current={$page.url.pathname === href ? 'page' : undefined}
		>
			{label}
		</a>
	{/each}
</nav>
