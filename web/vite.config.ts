import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import type { ProxyOptions } from 'vite';
import { defineConfig } from 'vitest/config';

const DEFAULT_BIND_ADDRESS = '127.0.0.1';
const DEFAULT_PORT = '8020';

export function backendTarget(
  environment: Readonly<Record<string, string | undefined>> = process.env
): string {
  const address = environment.POKECON_BIND_ADDRESS ?? DEFAULT_BIND_ADDRESS;
  const port = environment.POKECON_PORT ?? DEFAULT_PORT;
  if (!/^(?:[1-9][0-9]{0,4})$/u.test(port) || Number(port) > 65_535) {
    throw new Error('POKECON_PORT must be an integer from 1 through 65535');
  }
  const host = address.includes(':') && !address.startsWith('[') ? `[${address}]` : address;
  return `http://${host}:${port}`;
}

function proxyOptions(target: string, websocket: boolean): ProxyOptions {
  return {
    changeOrigin: true,
    configure(proxy) {
      proxy.on('proxyReq', (proxyRequest) => {
        proxyRequest.setHeader('Origin', target);
      });
    },
    rewriteWsOrigin: true,
    target,
    ws: websocket
  };
}

const target = backendTarget();

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  resolve: {
    conditions: ['browser']
  },
  server: {
    proxy: {
      '/api': proxyOptions(target, false),
      '/ws': proxyOptions(target, true)
    }
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
    setupFiles: ['./vitest.setup.ts']
  }
});
