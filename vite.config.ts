import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

// Tauri sets TAURI_ENV_* while running `tauri dev` / `tauri build`.
const platform = process.env.TAURI_ENV_PLATFORM;
const debug = !!process.env.TAURI_ENV_DEBUG;

export default defineConfig({
  plugins: [svelte()],
  // Keep Rust errors visible in the terminal.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/target/**', '**/tools/**'] },
  },
  // Only these reach import.meta.env. Not plain TAURI_: that would also match
  // TAURI_SIGNING_PRIVATE_KEY and friends.
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  // The root holds target/ (rustdoc HTML etc.): only scan our entry for dependencies.
  optimizeDeps: { entries: ['index.html'] },
  build: {
    // WebView2 on Windows is Chromium; macOS 11 ships WebKit 14; WebKitGTK is close to it.
    target: platform === 'windows' ? 'chrome105' : 'safari14',
    minify: !debug,
    sourcemap: debug,
    outDir: 'dist',
    emptyOutDir: true,
  },
});
