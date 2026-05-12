import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	compilerOptions: {
		// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
		runes: ({ filename }) => (filename.split(/[/\\\\]/).includes('node_modules') ? undefined : true)
	},
	kit: {
		adapter: adapter({
			// Output to dist for Tauri integration
			pages: 'dist',
			assets: 'dist',
			fallback: 'index.html',
			precompress: false,
			strict: true,
		}),
		paths: {
			// All UI routes are served under /ui/ prefix
			// The Rust backend handles / → /ui/ redirect
			base: '/ui',
		},
	}
};

export default config;
