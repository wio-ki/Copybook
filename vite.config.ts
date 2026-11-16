import { resolve } from 'node:path';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Tauri serves the built files from ../dist; `tauri dev` loads http://localhost:1420.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: {
    target: 'chrome120', // WebView2 is evergreen Chromium
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        panel: resolve(import.meta.dirname, 'panel.html'),
        settings: resolve(import.meta.dirname, 'settings.html'),
      },
    },
  },
});
