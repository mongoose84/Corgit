import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri sets this when developing against a device on the network.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte()],

  // Tauri's own output is more useful than Vite's, so don't wipe it.
  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      // Rust rebuilds are driven by cargo, not Vite.
      ignored: ['**/src-tauri/**'],
    },
  },

  build: {
    // WebView2 is evergreen, so there is nothing old to transpile down to.
    target: 'esnext',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    // Vite 8 minifies through rolldown/oxc; naming esbuild here would pull in
    // a dependency that no longer ships with Vite.
    minify: !process.env.TAURI_ENV_DEBUG,
  },
});
