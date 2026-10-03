import adapter from '@sveltejs/adapter-static';
import type { Config } from '@sveltejs/kit';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

const versionName = process.env.POKECON_WEB_VERSION ?? 'development';

const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      pages: 'dist',
      assets: 'dist',
      fallback: 'index.html',
      precompress: false,
      strict: true
    }),
    alias: {
      $components: 'src/lib/components',
      $stores: 'src/lib/stores'
    },
    version: {
      name: versionName,
      pollInterval: 0
    }
  }
} satisfies Config;

export default config;
