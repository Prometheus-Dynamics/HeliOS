import { resolve as resolvePath } from 'node:path';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
	const env = loadEnv(mode, process.cwd(), '');
	const proxyTarget =
		env.VITE_API_PROXY_TARGET?.trim() ||
		env.PUBLIC_API_BASE?.trim() ||
		'http://127.0.0.1:5801';
	const isKnownVendorNoise = (message: string) =>
		(message.includes('@zag-js/svelte') || message.includes('@xyflow/svelte')) &&
		message.includes('never used');

	return {
		plugins: [tailwindcss(), sveltekit()],
		resolve: {
			alias: [
				{
					find: '$lib/ts-bindings/http/client',
					replacement: resolvePath(process.cwd(), 'src/lib/ts-bindings/http/client/index.ts')
				}
			]
		},
		build: {
			rollupOptions: {
				onwarn(warning, warn) {
					const message = typeof warning === 'string' ? warning : (warning.message ?? '');
					if (isKnownVendorNoise(message)) {
						return;
					}
					warn(warning);
				},
				output: {
					manualChunks(id) {
						if (id.includes('/src/lib/api/httpClient') || id.includes('/src/lib/ts-bindings/http/client')) {
							return 'api-client';
						}
						if (id.includes('node_modules/three-stdlib')) {
							return 'vendor-three-stdlib';
						}
						if (id.includes('node_modules/three/examples/jsm/loaders')) {
							return 'vendor-three-loaders';
						}
						if (id.includes('node_modules/three/examples/jsm/geometries')) {
							return 'vendor-three-geometry';
						}
						if (id.includes('node_modules/three/examples/jsm/')) {
							return 'vendor-three-extras';
						}
						if (id.includes('node_modules/@xyflow')) {
							return 'vendor-xyflow';
						}
						if (id.includes('node_modules/xterm')) {
							return 'vendor-xterm';
						}
						if (id.includes('node_modules/@zag-js')) {
							return 'vendor-zag';
						}
						if (id.includes('node_modules/@fortawesome')) {
							return 'vendor-icons';
						}
					}
				}
			}
		},
		server: {
			strictPort: true,
			proxy: {
				'/v1': {
					target: proxyTarget,
					changeOrigin: true,
					secure: false,
					ws: true
				}
			},
			watch: {
				usePolling: process.env.CHOKIDAR_USEPOLLING === '1' || process.env.VITE_USE_POLLING === '1',
				interval: 100
			}
		}
	};
});
