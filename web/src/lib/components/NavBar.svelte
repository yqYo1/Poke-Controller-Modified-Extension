<script lang="ts">
	import { page } from '$app/stores';
	import { resolve } from '$app/paths';

	const tabs = [
		{ label: 'カメラ', href: '/camera' },
		{ label: 'シリアル', href: '/serial' },
		{ label: '手動制御', href: '/manual' },
		{ label: 'コマンド', href: '/commands' },
		{ label: 'キーコンフィグ', href: '/keyconfig' },
		{ label: 'Pokemon Home', href: '/pokemonhome' },
		{ label: '通知', href: '/notification' },
		{ label: 'その他', href: '/others' },
	] as const;
</script>

<nav class="flex bg-gray-800 text-white">
	{#each tabs as { label, href } (href)}
		<a
			href={resolve(href)}
			class="px-4 py-3 text-sm font-medium transition-colors hover:bg-gray-700 aria-[current=page]:bg-blue-600 aria-[current=page]:text-white"
			aria-current={$page.url.pathname === resolve(href) ? 'page' : undefined}
		>
			{label}
		</a>
	{/each}
</nav>
