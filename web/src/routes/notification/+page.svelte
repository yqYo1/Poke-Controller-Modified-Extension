<script lang="ts">
	import WindowsNotification from '$lib/components/WindowsNotification.svelte';
	import DiscordNotification from '$lib/components/DiscordNotification.svelte';
</script>

<div class="tab-content">
	<div class="flex items-center justify-between px-1 py-0.5">
		<h2 class="text-sm font-bold text-gray-200">Notification Settings</h2>
	</div>

	<div class="grid grid-cols-1 gap-3 lg:grid-cols-2">
		<!-- Windows Notification -->
		<div>
			<WindowsNotification />
		</div>

		<!-- Discord Notification -->
		<div>
			<DiscordNotification />
		</div>
	</div>
</div>
