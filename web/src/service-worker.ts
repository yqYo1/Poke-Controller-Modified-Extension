/// <reference types="@sveltejs/kit" />
/// <reference no-default-lib="true"/>
/// <reference lib="esnext" />
/// <reference lib="webworker" />

import { build, files, prerendered, version } from '$service-worker';

const sw = self as unknown as ServiceWorkerGlobalScope;

const CACHE_NAME = `pokecon-cache-${version}`;
const ASSET_CACHE = `${CACHE_NAME}-assets`;
const NAV_CACHE = `${CACHE_NAME}-nav`;
const OFFLINE_URL = '/ui/offline.html';

const ASSETS = [
	...build,
	...files,
	...prerendered,
	'/ui/manifest.json',
	'/ui/icon.svg',
	OFFLINE_URL,
];

// --- Install: cache all static assets ---
sw.addEventListener('install', (event: ExtendableEvent) => {
	event.waitUntil(
		(async () => {
			const cache = await caches.open(ASSET_CACHE);
			await cache.addAll(ASSETS);
			await sw.skipWaiting();
		})(),
	);
});

// --- Activate: clean up old caches ---
sw.addEventListener('activate', (event: ExtendableEvent) => {
	event.waitUntil(
		(async () => {
			const keys = await caches.keys();
			await Promise.all(
				keys.map((key) => {
					if (key !== ASSET_CACHE && key !== NAV_CACHE) {
						return caches.delete(key);
					}
				}),
			);
			await sw.clients.claim();
		})(),
	);
});

// --- Fetch: cache-first for static assets, network-first for navigation ---
sw.addEventListener('fetch', (event: FetchEvent) => {
	const { request } = event;
	const url = new URL(request.url);

	// Only handle same-origin requests
	if (url.origin !== sw.location.origin) return;

	// Skip non-GET and browser extension requests
	if (request.method !== 'GET') return;

	// API and WebSocket requests — network-only, no caching
	if (url.pathname.startsWith('/api/') || url.pathname.startsWith('/ws')) {
		return;
	}

	// Navigation requests — network-first with offline fallback
	if (request.mode === 'navigate') {
		event.respondWith(navStrategy(request));
		return;
	}

	// Static asset requests — cache-first
	event.respondWith(cacheFirstStrategy(request));
});

/**
 * Cache-first strategy for static assets.
 * Serves from cache if available, falls back to network and caches the response.
 */
async function cacheFirstStrategy(request: Request): Promise<Response> {
	const cached = await caches.match(request);
	if (cached) return cached;

	try {
		const response = await fetch(request);
		if (response.ok) {
			const cache = await caches.open(ASSET_CACHE);
			// Don't cache opaque responses
			if (response.type === 'basic') {
				cache.put(request, response.clone());
			}
		}
		return response;
	} catch {
		// If the request looks like a page navigation fallback to offline
		if (request.destination === 'document') {
			return caches.match(OFFLINE_URL) ?? new Response('Offline', { status: 503 });
		}
		return new Response('Offline', { status: 503 });
	}
}

/**
 * Network-first strategy for navigation requests.
 * Tries network first, falls back to cache, then to offline page.
 */
async function navStrategy(request: Request): Promise<Response> {
	try {
		const response = await fetch(request);
		if (response.ok) {
			const cache = await caches.open(NAV_CACHE);
			cache.put(request, response.clone());
		}
		return response;
	} catch {
		// Try cache first
		const cached = await caches.match(request);
		if (cached) return cached;

		// Fallback to offline page
		const offline = await caches.match(OFFLINE_URL);
		return offline ?? new Response('Offline', { status: 503 });
	}
}
