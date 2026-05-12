<script lang="ts">
	import { onMount } from 'svelte';

	// ── Menu state ─────────────────────────────────────────────────────────────
	let activeMenu = $state<string | null>(null);
	let visible = $state(true);

	// ── Menu structure ─────────────────────────────────────────────────────────
	interface MenuItem {
		label: string;
		action?: () => void;
		separator?: boolean;
		disabled?: boolean;
		shortcut?: string;
	}

	interface Menu {
		label: string;
		id: string;
		items: MenuItem[];
	}

	const onToggleNav = () => {
		// Dispatch custom event for toggling navigation visibility
		window.dispatchEvent(new CustomEvent('toggle-nav'));
	};

	const onToggleLog = () => {
		window.dispatchEvent(new CustomEvent('toggle-log'));
	};

	const onToggleOutput = () => {
		window.dispatchEvent(new CustomEvent('toggle-output'));
	};

	const onToggleStatusBar = () => {
		window.dispatchEvent(new CustomEvent('toggle-statusbar'));
	};

	const onAbout = () => {
		window.dispatchEvent(new CustomEvent('show-about'));
	};

	const menus: Menu[] = [
		{
			label: 'ファイル',
			id: 'file',
			items: [
				{ label: '設定を保存', shortcut: 'Ctrl+S', action: () => window.dispatchEvent(new CustomEvent('save-config')) },
				{ label: '設定を読み込み', shortcut: 'Ctrl+O', action: () => window.dispatchEvent(new CustomEvent('load-config')) },
				{ separator: true, label: '' },
				{ label: '終了', shortcut: 'Ctrl+Q', action: () => window.dispatchEvent(new CustomEvent('quit-app')) },
			],
		},
		{
			label: '編集',
			id: 'edit',
			items: [
				{ label: '設定', action: () => window.dispatchEvent(new CustomEvent('open-settings')) },
				{ separator: true, label: '' },
				{ label: 'キーコンフィグ', action: () => window.location.href = '/ui/keyconfig' },
				{ label: 'マウススティック設定', action: () => window.location.href = '/ui/others' },
			],
		},
		{
			label: '表示',
			id: 'view',
			items: [
				{ label: 'ナビゲーションバー', action: onToggleNav, shortcut: 'Ctrl+1' },
				{ label: 'ログパネル', action: onToggleLog, shortcut: 'Ctrl+2' },
				{ label: '出力パネル', action: onToggleOutput, shortcut: 'Ctrl+3' },
				{ label: 'ステータスバー', action: onToggleStatusBar, shortcut: 'Ctrl+4' },
				{ separator: true, label: '' },
				{ label: 'メニューバーを隠す', action: () => (visible = !visible) },
			],
		},
		{
			label: 'ヘルプ',
			id: 'help',
			items: [
				{ label: 'バージョン情報', action: onAbout },
				{ separator: true, label: '' },
				{ label: 'ドキュメント', action: () => window.open('https://github.com/yqYo1/Poke-Controller-Modified-Extension', '_blank') },
				{ label: '更新を確認', action: () => window.dispatchEvent(new CustomEvent('check-updates')) },
			],
		},
	];

	function toggleMenu(id: string) {
		if (activeMenu === id) {
			activeMenu = null;
		} else {
			activeMenu = id;
		}
	}

	function handleMenuItemClick(item: MenuItem) {
		if (item.disabled || item.separator) return;
		activeMenu = null;
		item.action?.();
	}

	function closeMenu() {
		activeMenu = null;
	}

	onMount(() => {
		const handleKey = (e: KeyboardEvent) => {
			if (activeMenu) {
				if (e.key === 'Escape') {
					closeMenu();
				}
			}
		};
		window.addEventListener('keydown', handleKey);
		return () => window.removeEventListener('keydown', handleKey);
	});
</script>

{#if visible}
	<div class="relative select-none" role="menubar">
		<!-- Menu bar -->
		<div class="flex items-center bg-gray-800 text-xs text-gray-200">
			{#each menus as menu}
				<button
					onclick={() => toggleMenu(menu.id)}
					onmouseenter={() => { if (activeMenu) activeMenu = menu.id; }}
					class="px-3 py-1.5 transition-colors hover:bg-gray-700 {activeMenu === menu.id ? 'bg-gray-700' : ''}"
					role="menuitem"
					aria-haspopup="true"
					aria-expanded={activeMenu === menu.id}
				>
					{menu.label}
				</button>
			{/each}
		</div>

		<!-- Dropdown -->
		{#if activeMenu}
			<!-- svelte-ignore a11y_click_events_have_key_events -->
			<div
				class="fixed inset-0 z-40"
				onclick={closeMenu}
				oncontextmenu={(e) => e.preventDefault()}
				role="presentation"
			></div>
			<div
				class="absolute left-0 z-50 min-w-44 rounded border border-gray-700 bg-gray-800 shadow-xl"
				role="menu"
			>
				{@const menu = menus.find((m) => m.id === activeMenu)}
				{#if menu}
					{#each menu.items as item}
						{#if item.separator}
							<div class="my-1 border-t border-gray-700"></div>
						{:else}
							<button
								onclick={() => handleMenuItemClick(item)}
								disabled={item.disabled}
								class="flex w-full items-center justify-between px-3 py-1.5 text-left text-xs text-gray-200 transition-colors hover:bg-gray-700 disabled:opacity-40"
								role="menuitem"
							>
								<span>{item.label}</span>
								{#if item.shortcut}
									<span class="ml-4 text-[10px] text-gray-500">{item.shortcut}</span>
								{/if}
							</button>
						{/if}
					{/each}
				{/if}
			</div>
		{/if}
	</div>
{/if}

<style>
	/* Ensure dropdown appears above other content */
	:global(.menubar-dropdown) {
		z-index: 9999;
	}
</style>
