import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// `bun run dev` proxies /v1 to a device's helios-api: HELIOS_DEVICE=http://raze.local:5801
// (default http://127.0.0.1:5800). Add ?mock=1 to the URL to run without a device.
const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env ?? {};
const device = env.HELIOS_DEVICE ?? "http://127.0.0.1:5800";

export default defineConfig({
  plugins: [
    tailwindcss(),
    // SvelteKit 3 reads its config here (svelte.config.js is no longer used).
    sveltekit({
      preprocess: vitePreprocess(),
      // A static bundle the device serves; every route falls back to the app shell.
      adapter: adapter({ fallback: "index.html", precompress: true }),
      // SvelteKit 3 dropped the built-in `$lib` alias in favour of `#lib`; keep `$lib`.
      alias: { $lib: "src/lib" },
    }),
  ],
  server: {
    port: 5810,
    strictPort: true,
    proxy: { "/v1": { target: device, changeOrigin: true } },
  },
});
