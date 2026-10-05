import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
export default {
  preprocess: vitePreprocess(),
  kit: {
    // A static bundle the device serves; every route falls back to the app shell.
    adapter: adapter({ fallback: "index.html", precompress: true }),
  },
};
