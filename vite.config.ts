// `vitest/config` rather than `vite`: same function, but its type knows about
// the `test` block below. Importing it from `vite` typechecks everything else
// and then rejects that key.
import { defineConfig } from 'vitest/config';
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

  test: {
    // Only the pure modules — lane layout, branch-name checking, error
    // translation. Everything else in `src/` is either a Svelte component or a
    // store whose whole job is talking to Tauri, and neither is worth a DOM or
    // an IPC mock to assert: the logic that can be *silently wrong* lives in
    // these three files, and none of it needs a browser to run.
    include: ['src/**/*.test.ts'],
  },
});
