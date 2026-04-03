import adapter from '@sveltejs/adapter-static';
import { resolve as resolvePath } from 'node:path';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

const viteScriptPreprocess = vitePreprocess({ script: true });
const stripTsLangPreprocess = {
	async script({ attributes, content, filename }) {
		const transformed = await viteScriptPreprocess.script?.({ attributes, content, filename });
		if (!transformed) {
			return transformed;
		}

		return {
			...transformed,
			attributes: Object.fromEntries(Object.entries(attributes).filter(([name]) => name !== 'lang'))
		};
	}
};

/** @type {import('@sveltejs/kit').Config} */
const config = {
	// Consult https://svelte.dev/docs/kit/integrations
	// for more information about preprocessors
	preprocess: [stripTsLangPreprocess, vitePreprocess()],
	compilerOptions: {
		runes: true
	},
	kit: {
		alias: {
			$generated: resolvePath('src/generated')
		},
		adapter: adapter({
			fallback: 'index.html'
		}),
		version: {
			pollInterval: 15000
		},
		prerender: {
			entries: ['*']
		}
	}
};

export default config;
