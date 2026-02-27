import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, loadEnv } from 'vite';

export default defineConfig(({ mode }) => {
	const env = loadEnv(mode, process.cwd(), '');
	const proxyTarget =
		env.VITE_API_PROXY_TARGET?.trim() ||
		env.PUBLIC_API_BASE?.trim() ||
		'http://127.0.0.1:5801';

	return {
		plugins: [tailwindcss(), sveltekit()],
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
