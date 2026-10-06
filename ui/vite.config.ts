import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// `bun run dev` proxies /v1 to a device's helios-api: HELIOS_DEVICE=http://raze.local:5801
// (default http://127.0.0.1:5800). Add ?mock=1 to the URL to run without a device.
const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env ?? {};
const device = env.HELIOS_DEVICE ?? "http://127.0.0.1:5800";

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  server: {
    port: 5810,
    strictPort: true,
    proxy: { "/v1": { target: device, changeOrigin: true } },
  },
});
